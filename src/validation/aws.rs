use std::{collections::HashSet, sync::RwLock, time::Duration};

use anyhow::{anyhow, Result};
use aws_config::{retry::RetryConfig, BehaviorVersion, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_sts::{
    config::Builder as StsConfigBuilder, error::SdkError,
    operation::get_caller_identity::GetCallerIdentityError, Client as StsClient,
};
use aws_smithy_http_client::{
    proxy::ProxyConfig, tls, Builder as HttpClientBuilder, ConnectorBuilder,
};
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        http::SharedHttpClient,
        interceptors::{context::BeforeTransmitInterceptorContextMut, Intercept},
        runtime_components::RuntimeComponents,
    },
};
use aws_smithy_types::config_bag::ConfigBag;
use aws_types::region::Region;
use base32::Alphabet;
use byteorder::{BigEndian, ByteOrder};
use http::{
    header::{HeaderValue, USER_AGENT},
    StatusCode,
};
use once_cell::sync::{Lazy, OnceCell};
use rand::{rng, Rng};
use regex::Regex;
use tokio::{
    sync::Semaphore,
    time::{sleep, timeout},
};

use crate::validation::GLOBAL_USER_AGENT;

static AWS_VALIDATION_SEMAPHORE: OnceCell<Semaphore> = OnceCell::new();
const BUILTIN_SKIP_ACCOUNT_IDS: &[&str] = &[
    "052310077262",
    "171436882533",
    "528757803018",
    "534261010715",
    "595918472158",
    "729780141977",
    "893192397702",
    "992382622183",
];

static AWS_SKIP_ACCOUNT_IDS: Lazy<RwLock<HashSet<String>>> = Lazy::new(|| {
    let mut set = HashSet::new();
    set.extend(BUILTIN_SKIP_ACCOUNT_IDS.iter().map(|id| id.to_string()));
    RwLock::new(set)
});

fn build_http_client() -> SharedHttpClient {
    HttpClientBuilder::new().build_with_connector_fn(|settings, runtime_components| {
        let mut conn_builder = ConnectorBuilder::default()
            .tls_provider(tls::Provider::Rustls(tls::rustls_provider::CryptoMode::AwsLc));

        conn_builder.set_connector_settings(settings.cloned());
        if let Some(components) = runtime_components {
            conn_builder.set_sleep_impl(components.sleep_impl());
        }
        conn_builder.set_proxy_config(Some(ProxyConfig::from_env()));
        conn_builder.build()
    })
}

async fn build_base_config(credentials: Credentials) -> SdkConfig {
    let retry_config = RetryConfig::adaptive().with_max_attempts(3);
    aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .http_client(build_http_client())
        .retry_config(retry_config)
        .load()
        .await
}

fn extract_account_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.len() == 12 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }

    static ACCOUNT_ID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{12})").expect("valid regex"));
    ACCOUNT_ID_RE.captures(trimmed).and_then(|caps| caps.get(1)).map(|m| m.as_str().to_string())
}

/// Set the maximum number of concurrent AWS validations. Call before first use.
pub fn set_aws_validation_concurrency(max: usize) {
    AWS_VALIDATION_SEMAPHORE.set(Semaphore::new(max)).ok();
}

fn aws_validation_semaphore() -> &'static Semaphore {
    AWS_VALIDATION_SEMAPHORE.get_or_init(|| Semaphore::new(15))
}

pub fn set_aws_skip_account_ids<I, S>(ids: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut guard = match AWS_SKIP_ACCOUNT_IDS.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clear();

    guard.extend(BUILTIN_SKIP_ACCOUNT_IDS.iter().map(|id| id.to_string()));

    for raw in ids.into_iter() {
        let value = raw.into();
        if value.trim().is_empty() {
            continue;
        }
        if let Some(normalized) = extract_account_id(&value) {
            guard.insert(normalized);
        } else {
            tracing::warn!("Ignoring invalid AWS account ID in skip list: {value}");
        }
    }
}

pub fn should_skip_aws_validation(access_key_id: &str) -> Option<String> {
    let guard = AWS_SKIP_ACCOUNT_IDS.read().ok()?;
    if guard.is_empty() {
        return None;
    }

    let account = aws_key_to_account_number(access_key_id).ok()?;
    if guard.contains(&account) {
        Some(account)
    } else {
        None
    }
}

#[derive(Debug)]
struct UaInterceptor;

impl Intercept for UaInterceptor {
    fn name(&self) -> &'static str {
        "ua"
    }

    fn modify_before_transmit(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _rc: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> std::result::Result<(), BoxError> {
        let req = context.request_mut();
        req.headers_mut().insert(
            USER_AGENT,
            HeaderValue::from_str(GLOBAL_USER_AGENT.as_str())
                .map_err(|e| format!("invalid USER_AGENT header: {e}"))?,
        );
        Ok(())
    }
}

/// Generate a standardized cache key for AWS validation attempts
pub fn generate_aws_cache_key(aws_access_key_id: &str, aws_secret_access_key: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(aws_access_key_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(aws_secret_access_key.as_bytes());
    format!("AWS:{:x}", hasher.finalize())
}

// Validate AWS credentials before attempting validation
pub fn validate_aws_credentials_input(access_key_id: &str, secret_key: &str) -> Result<(), String> {
    // Validate access key ID format (typically starts with "AKIA" and is 20 chars)
    if !access_key_id.starts_with("AKIA") || access_key_id.len() != 20 {
        return Err("Invalid AWS access key ID format".to_string());
    }
    // Validate secret key format (should be at least 40 chars)
    if secret_key.len() < 40 {
        return Err("Invalid AWS secret key format".to_string());
    }
    // Check for invalid characters
    if !access_key_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("AWS access key ID contains invalid characters".to_string());
    }
    if !secret_key.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '+') {
        return Err("AWS secret key contains invalid characters".to_string());
    }
    Ok(())
}

fn is_throttling_or_transient(e: &SdkError<GetCallerIdentityError>) -> bool {
    match e {
        SdkError::ServiceError(ctx) => {
            let code = ctx.err().meta().code().unwrap_or_default();
            let status: StatusCode = ctx.raw().status().into();
            code.contains("Throttl")
                || status == StatusCode::TOO_MANY_REQUESTS
                || status == StatusCode::SERVICE_UNAVAILABLE
        }
        SdkError::DispatchFailure(df) => df.is_timeout() || df.is_io(),
        SdkError::ResponseError(ctx) => {
            let status: StatusCode = ctx.raw().status().into();
            status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE
        }
        _ => false,
    }
}

pub async fn validate_aws_credentials(
    aws_access_key_id: &str,
    aws_secret_access_key: &str,
) -> Result<(bool, String)> {
    let _permit = aws_validation_semaphore().acquire().await.expect("semaphore closed");

    // Create static credentials
    let credentials = Credentials::new(
        aws_access_key_id,
        aws_secret_access_key,
        None,     // session token
        None,     // expiry
        "static", // provider name
    );
    let config = build_base_config(credentials).await;

    // Create STS client
    let sts_config = StsConfigBuilder::from(&config).interceptor(UaInterceptor).build();
    let sts_client = StsClient::from_conf(sts_config);

    const MAX_ATTEMPTS: usize = 3;
    const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

    for attempt in 1..=MAX_ATTEMPTS {
        let result = timeout(ATTEMPT_TIMEOUT, sts_client.get_caller_identity().send()).await;
        match result {
            Ok(Ok(identity)) => {
                let arn = identity.arn.unwrap_or_else(|| "Unknown".to_string());
                return Ok((true, arn));
            }
            Ok(Err(e)) => {
                if is_throttling_or_transient(&e) {
                    if attempt == MAX_ATTEMPTS {
                        return Err(anyhow!("AWS validation failed: {}", e));
                    }
                } else {
                    return Ok((false, e.to_string()));
                }
            }
            Err(_) => {
                if attempt == MAX_ATTEMPTS {
                    return Err(anyhow!("AWS validation timed out"));
                }
            }
        }
        let max_delay = 100u64 * 2u64.pow((attempt - 1) as u32);
        let sleep_ms = rng().random_range(0..=max_delay);
        sleep(Duration::from_millis(sleep_ms)).await;
    }
    Err(anyhow!("AWS validation failed"))
}

/// Converts an AWS Key ID to an AWS Account Number.
/// It assumes that the Key ID has a specific format and extracts the account
/// number encoded within it. Reference: https://medium.com/@TalBeerySec/a-short-note-on-aws-key-id-f88cc4317489
pub fn aws_key_to_account_number(aws_key_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Ensure the AWS Key ID is at least 5 characters long (since we'll access index
    // 4)
    if aws_key_id.len() < 5 {
        return Err("AWSKeyID is too short".into());
    }
    // Check if the 5th character is 'I' or 'J'
    let fifth_char = aws_key_id.as_bytes()[4] as char;
    if fifth_char == 'I' || fifth_char == 'J' {
        let err_msg =
            format!("Not possible to retrieve account number for {} keys", &aws_key_id[..5]);
        return Err(err_msg.into());
    }
    // Remove the Key ID prefix (first 4 characters)
    let trimmed_aws_key_id = &aws_key_id[4..];
    // Decode the trimmed Key ID from base32, ensuring it's in uppercase
    let decoded =
        base32::decode(Alphabet::Rfc4648 { padding: false }, &trimmed_aws_key_id.to_uppercase())
            .ok_or("Error decoding AWSKeyID")?;
    if decoded.len() < 6 {
        return Err("Decoded AWSKeyID is too short".into());
    }
    // Create an 8-byte array initialized to zeros
    let mut data = [0u8; 8];
    // Copy decoded[0..6] into data[2..8]
    data[2..8].copy_from_slice(&decoded[0..6]);
    // Interpret data as a big-endian u64
    let z = BigEndian::read_u64(&data);
    // Define the mask
    const MASK: u64 = 0x7FFFFFFFFF80;
    // Calculate the account number
    let account_num = (z & MASK) >> 7;
    // Return the account number formatted as a 12-digit string
    Ok(format!("{:012}", account_num))
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    static TEST_GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn skip_account_list_normalizes_inputs() {
        let _lock = TEST_GUARD.lock().unwrap();

        set_aws_skip_account_ids([
            " 052310077262 ",
            "arn:aws:iam::171436882533:role/demo",
            "invalid",
        ]);

        let guard = AWS_SKIP_ACCOUNT_IDS.read().unwrap();
        assert!(guard.contains("052310077262"));
        assert!(guard.contains("171436882533"));
        assert_eq!(guard.len(), BUILTIN_SKIP_ACCOUNT_IDS.len());
        drop(guard);

        set_aws_skip_account_ids(Vec::<String>::new());
    }

    #[test]
    fn should_skip_when_account_matches() {
        let _lock = TEST_GUARD.lock().unwrap();

        set_aws_skip_account_ids(["534261010715"]);
        assert_eq!(
            should_skip_aws_validation("AKIAXYZDQCEN4B6JSJQI"),
            Some("534261010715".to_string())
        );

        set_aws_skip_account_ids(Vec::<String>::new());
    }

    #[test]
    fn builtin_canary_accounts_are_preseeded() {
        let _lock = TEST_GUARD.lock().unwrap();

        set_aws_skip_account_ids(Vec::<String>::new());
        assert_eq!(
            should_skip_aws_validation("AKIAXYZDQCEN4B6JSJQI"),
            Some("534261010715".to_string())
        );

        set_aws_skip_account_ids(Vec::<String>::new());
    }

    #[test]
    fn duplicate_accounts_are_deduplicated() {
        let _lock = TEST_GUARD.lock().unwrap();

        set_aws_skip_account_ids([
            "534261010715",
            "arn:aws:iam::534261010715:user/canarytokens",
            " 534261010715 ",
        ]);

        let guard = AWS_SKIP_ACCOUNT_IDS.read().unwrap();
        assert_eq!(guard.iter().filter(|id| id.as_str() == "534261010715").count(), 1);
        drop(guard);

        set_aws_skip_account_ids(Vec::<String>::new());
    }
}
