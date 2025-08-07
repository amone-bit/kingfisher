use super::*;

impl DetailsReporter {
    pub fn json_format<W: std::io::Write>(
        &self,
        mut writer: W,
        args: &cli::commands::scan::ScanArgs,
    ) -> Result<()> {
        let records = self.build_finding_records(args)?;
        if !records.is_empty() {
            serde_json::to_writer_pretty(&mut writer, &records)?;
            writeln!(writer)?;
        }
        Ok(())
    }

    pub fn jsonl_format<W: std::io::Write>(
        &self,
        mut writer: W,
        args: &cli::commands::scan::ScanArgs,
    ) -> Result<()> {
        let records = self.build_finding_records(args)?;
        for record in records {
            serde_json::to_writer(&mut writer, &record)?;
            writeln!(writer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::github::GitCloneMode;
    use crate::cli::commands::github::GitHistoryMode;
    use crate::cli::commands::rules::RuleSpecifierArgs;
    use crate::matcher::{SerializableCapture, SerializableCaptures};
    use crate::util::intern;
    use crate::{
        blob::BlobId,
        cli::commands::github::GitHubRepoType,
        cli::commands::inputs::ContentFilteringArgs,
        cli::commands::inputs::InputSpecifierArgs,
        cli::commands::output::{OutputArgs, ReportOutputFormat},
        cli::commands::scan::ConfidenceLevel,
        findings_store::FindingsStore,
        location::{Location, OffsetSpan, SourcePoint, SourceSpan},
        matcher::Match,
        origin::Origin,
        reporter::styles::Styles,
    };
    use std::{
        io::Cursor,
        path::PathBuf,
        sync::{Arc, Mutex},
    };
    use url::Url;
    fn create_default_args() -> cli::commands::scan::ScanArgs {
        use crate::cli::commands::gitlab::GitLabRepoType; // bring enum into scope

        cli::commands::scan::ScanArgs {
            num_jobs: 1,
            no_dedup: false,
            rules: RuleSpecifierArgs {
                rules_path: Vec::new(),
                rule: vec!["all".into()],
                load_builtins: true,
            },
            input_specifier_args: InputSpecifierArgs {
                // local path / git URL inputs
                path_inputs: Vec::new(),
                git_url: Vec::new(),

                // GitHub
                github_user: Vec::new(),
                github_organization: Vec::new(),
                all_github_organizations: false,
                github_api_url: Url::parse("https://api.github.com/").unwrap(),
                github_repo_type: GitHubRepoType::All,

                // GitLab
                gitlab_user: Vec::new(),
                gitlab_group: Vec::new(),
                all_gitlab_groups: false,
                gitlab_api_url: Url::parse("https://gitlab.com/").unwrap(),
                gitlab_repo_type: GitLabRepoType::All,
                // Jira options
                jira_url: None,
                jql: None,
                max_results: 100,
                // Slack options
                slack_query: None,
                slack_api_url: Url::parse("https://slack.com/api/").unwrap(),
                // s3
                s3_bucket: None,
                s3_prefix: None,
                role_arn: None,
                aws_local_profile: None,

                docker_image: Vec::new(),
                // clone / history options
                git_clone: GitCloneMode::Bare,
                git_history: GitHistoryMode::Full,
                scan_nested_repos: true,
                commit_metadata: true,
            },
            content_filtering_args: ContentFilteringArgs {
                max_file_size_mb: 25.0,
                no_extract_archives: false,
                extraction_depth: 2,
                exclude: Vec::new(), // Exclude patterns
                no_binary: true,
            },
            confidence: ConfidenceLevel::Medium,
            no_validate: false,
            rule_stats: false,
            only_valid: false,
            min_entropy: None,
            redact: false,
            git_repo_timeout: 1800, // 30 minutes
            output_args: OutputArgs { output: None, format: ReportOutputFormat::Pretty },
            snippet_length: 256,
            baseline_file: None,
            manage_baseline: false,
        }
    }

    fn create_mock_match(
        rule_name: &str,
        rule_text_id: &str,
        rule_finding_fingerprint: &str,
        validation_success: bool,
    ) -> Match {
        Match {
            location: Location {
                offset_span: OffsetSpan { start: 10, end: 20 },
                source_span: SourceSpan {
                    start: SourcePoint { line: 5, column: 10 },
                    end: SourcePoint { line: 5, column: 20 },
                },
            },
            groups: SerializableCaptures {
                captures: vec![SerializableCapture {
                    name: Some("token".to_string()),
                    match_number: 1,
                    start: 10,
                    end: 20,
                    value: "mock_token".into(),
                }],
            },
            blob_id: BlobId::new(b"mock_blob"),
            finding_fingerprint: 0123,
            rule_finding_fingerprint: intern(rule_finding_fingerprint),
            rule_text_id: intern(rule_text_id),
            rule_name: intern(rule_name),
            rule_confidence: Confidence::Medium,
            validation_response_body: "validation response".to_string(),
            validation_response_status: 200,
            validation_success,
            calculated_entropy: 4.5,
            visible: true,
        }
    }

    fn setup_mock_reporter(matches: Vec<ReportMatch>) -> DetailsReporter {
        let mut datastore = FindingsStore::new(PathBuf::from("/tmp"));
        if !matches.is_empty() {
            let blob_metadata = BlobMetadata {
                id: BlobId::new(b"mock_blob"),
                num_bytes: 1024,
                mime_essence: Some("text/plain".to_string()),
                charset: Some("UTF-8".to_string()),
                language: Some("Rust".to_string()),
            };
            let dedup = true;
            for m in matches.clone() {
                datastore.record(
                    vec![(
                        Arc::new(OriginSet::new(
                            Origin::from_file(PathBuf::from("/mock/path/file.rs")),
                            vec![],
                        )),
                        Arc::new(blob_metadata.clone()),
                        m.m.clone(),
                    )],
                    dedup,
                );
            }
        }
        DetailsReporter {
            datastore: Arc::new(Mutex::new(datastore)),
            styles: Styles::new(false),
            only_valid: false,
        }
    }

    #[test]
    fn test_json_format() -> Result<()> {
        let mock_match =
            create_mock_match("MockRule", "mock_rule_1", "mock_finding_fingerprint", true);
        let matches = vec![ReportMatch {
            origin: OriginSet::new(Origin::from_file(PathBuf::from("/mock/path/file.rs")), vec![]),
            blob_metadata: BlobMetadata {
                id: BlobId::new(b"mock_blob"),
                num_bytes: 1024,
                mime_essence: Some("text/plain".to_string()),
                charset: Some("UTF-8".to_string()),
                language: Some("Rust".to_string()),
            },
            m: mock_match,
            comment: None,
            match_confidence: Confidence::Medium,
            visible: true,
            validation_response_body: "validation response".to_string(),
            validation_response_status: 200,
            validation_success: true,
        }];
        let reporter = setup_mock_reporter(matches);
        let mut output = Cursor::new(Vec::new());
        reporter.json_format(&mut output, &create_default_args())?;
        let json_output: Vec<serde_json::Value> = serde_json::from_slice(&output.into_inner())?;
        assert!(!json_output.is_empty(), "JSON output should not be empty");
        let first = &json_output[0];
        assert_eq!(first["rule"]["name"], "MockRule");
        assert_eq!(first["finding"]["language"], "Rust");
        Ok(())
    }

    #[test]
    fn test_validation_status_in_json() -> Result<()> {
        let test_cases = vec![(true, "Active Credential"), (false, "Inactive Credential")];
        for (validation_success, expected_status) in test_cases {
            let mock_match = create_mock_match(
                "MockRule",
                "mock_rule_1",
                "mock_finding_fingerprint",
                validation_success,
            );
            let matches = vec![ReportMatch {
                origin: OriginSet::new(
                    Origin::from_file(PathBuf::from("/mock/path/file.rs")),
                    vec![],
                ),
                blob_metadata: BlobMetadata {
                    id: BlobId::new(b"mock_blob"),
                    num_bytes: 1024,
                    mime_essence: Some("text/plain".to_string()),
                    charset: Some("UTF-8".to_string()),
                    language: Some("Rust".to_string()),
                },
                m: mock_match,
                comment: None,
                match_confidence: Confidence::Medium,
                visible: true,
                validation_response_body: "validation response".to_string(),
                validation_response_status: 200,
                validation_success,
            }];
            let reporter = setup_mock_reporter(matches);
            let mut output = Cursor::new(Vec::new());
            reporter.json_format(&mut output, &create_default_args())?;
            let json_output: Vec<serde_json::Value> = serde_json::from_slice(&output.into_inner())?;
            assert!(!json_output.is_empty(), "JSON output should not be empty");
            let first = &json_output[0];
            let validation_status = first["finding"]["validation"]["status"].as_str().unwrap();
            assert_eq!(validation_status, expected_status);
        }
        Ok(())
    }
}
