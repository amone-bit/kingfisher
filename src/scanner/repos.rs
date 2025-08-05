use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use indicatif::{HumanCount, ProgressBar, ProgressStyle};
use tokio::time::Duration;
use tracing::{debug, error, info};

use crate::blob::BlobIdMap;
use crate::{
    blob::BlobMetadata,
    cli::{
        commands::{
            github::{GitCloneMode, GitHistoryMode},
            scan,
        },
        global,
    },
    findings_store,
    git_binary::{CloneMode, Git},
    git_url::GitUrl,
    github, gitlab,
    guesser::Guesser,
    jira,
    matcher::{Match, Matcher, MatcherStats},
    origin::{Origin, OriginSet},
    rules_database::RulesDatabase,
    s3,
    scanner::processing::BlobProcessor,
    scanner_pool::ScannerPool,
    slack, PathBuf,
};

pub type DatastoreMessage = (OriginSet, BlobMetadata, Vec<(Option<f64>, Match)>);

pub fn clone_or_update_git_repos(
    args: &scan::ScanArgs,
    global_args: &global::GlobalArgs,
    repo_urls: &[GitUrl],
    datastore: &Arc<Mutex<findings_store::FindingsStore>>,
) -> Result<Vec<PathBuf>> {
    let mut input_roots = args.input_specifier_args.path_inputs.clone();
    if repo_urls.is_empty() || args.input_specifier_args.git_history == GitHistoryMode::None {
        return Ok(input_roots);
    }
    info!("{} Git URLs to fetch", repo_urls.len());
    for repo_url in repo_urls {
        debug!("Need to fetch {repo_url}")
    }
    let clone_mode = match args.input_specifier_args.git_clone {
        GitCloneMode::Mirror => CloneMode::Mirror,
        GitCloneMode::Bare => CloneMode::Bare,
    };
    let git = Git::new(global_args.ignore_certs);

    let progress = if global_args.use_progress() {
        let style = ProgressStyle::with_template(
            "{msg} {bar} {percent:>3}% {pos}/{len} [{elapsed_precise}]",
        )
        .expect("progress bar style template should compile");
        let pb = ProgressBar::new(repo_urls.len() as u64)
            .with_style(style)
            .with_message("Fetching Git repos");
        pb.enable_steady_tick(Duration::from_millis(500));
        pb
    } else {
        ProgressBar::hidden()
    };
    for repo_url in repo_urls {
        let output_dir = {
            let datastore = datastore.lock().unwrap();
            datastore.clone_destination(repo_url)
        };
        if output_dir.is_dir() {
            progress.suspend(|| info!("Updating clone of {repo_url}..."));
            match git.update_clone(repo_url, &output_dir) {
                Ok(()) => {
                    input_roots.push(output_dir);
                    progress.inc(1);
                    continue;
                }
                Err(e) => {
                    progress.suspend(|| {
                        debug!(
                            "Failed to update clone of {repo_url} at {}: {e}",
                            output_dir.display()
                        )
                    });
                    if let Err(e) = std::fs::remove_dir_all(&output_dir) {
                        progress.suspend(|| {
                            debug!(
                                "Failed to remove clone directory at {}: {e}",
                                output_dir.display()
                            )
                        });
                    }
                }
            }
        }
        progress.suspend(|| info!("Cloning {repo_url}..."));
        if let Err(e) = git.create_fresh_clone(repo_url, &output_dir, clone_mode) {
            progress.suspend(|| {
                error!("Failed to clone {repo_url} to {}: {e}", output_dir.display());
                debug!("Skipping scan of {repo_url}");
            });
            progress.inc(1);
            continue;
        }
        input_roots.push(output_dir);
        progress.inc(1);
    }
    progress.finish();
    Ok(input_roots)
}

pub async fn enumerate_github_repos(
    args: &scan::ScanArgs,
    global_args: &global::GlobalArgs,
) -> Result<Vec<GitUrl>> {
    let repo_specifiers = github::RepoSpecifiers {
        user: args.input_specifier_args.github_user.clone(),
        organization: args.input_specifier_args.github_organization.clone(),
        all_organizations: args.input_specifier_args.all_github_organizations,
        repo_filter: args.input_specifier_args.github_repo_type.into(),
    };
    let mut repo_urls = args.input_specifier_args.git_url.clone();
    if !repo_specifiers.is_empty() {
        let mut progress = if global_args.use_progress() {
            let style =
                ProgressStyle::with_template("{spinner} {msg} {human_len} [{elapsed_precise}]")
                    .expect("progress bar style template should compile");
            let pb = ProgressBar::new_spinner()
                .with_style(style)
                .with_message("Enumerating GitHub repositories...");
            pb.enable_steady_tick(Duration::from_millis(500));
            pb
        } else {
            ProgressBar::hidden()
        };
        let mut num_found: u64 = 0;
        let api_url = args.input_specifier_args.github_api_url.clone();
        let repo_strings = github::enumerate_repo_urls(
            &repo_specifiers,
            api_url,
            global_args.ignore_certs,
            Some(&mut progress),
        )
        .await
        .context("Failed to enumerate GitHub repositories")?;
        for repo_string in repo_strings {
            match GitUrl::from_str(&repo_string) {
                Ok(repo_url) => {
                    repo_urls.push(repo_url);
                    num_found += 1;
                }
                Err(e) => {
                    progress.suspend(|| {
                        error!("Failed to parse repo URL from {repo_string}: {e}");
                    });
                }
            }
        }
        progress.finish_with_message(format!(
            "Found {} repositories from GitHub",
            HumanCount(num_found)
        ));
    }
    repo_urls.sort();
    repo_urls.dedup();
    Ok(repo_urls)
}

pub async fn enumerate_gitlab_repos(
    args: &scan::ScanArgs,
    global_args: &global::GlobalArgs,
) -> Result<Vec<GitUrl>> {
    let repo_specifiers = gitlab::RepoSpecifiers {
        user: args.input_specifier_args.gitlab_user.clone(),
        group: args.input_specifier_args.gitlab_group.clone(),
        all_groups: args.input_specifier_args.all_gitlab_groups,
        repo_filter: args.input_specifier_args.gitlab_repo_type.into(),
    };

    let mut repo_urls = args.input_specifier_args.git_url.clone();
    if !repo_specifiers.is_empty() {
        let mut progress = if global_args.use_progress() {
            let style =
                ProgressStyle::with_template("{spinner} {msg} {human_len} [{elapsed_precise}]")
                    .expect("progress bar style template should compile");
            let pb = ProgressBar::new_spinner()
                .with_style(style)
                .with_message("Enumerating GitLab repositories...");
            pb.enable_steady_tick(Duration::from_millis(500));
            pb
        } else {
            ProgressBar::hidden()
        };

        let mut num_found: u64 = 0;
        let api_url = args.input_specifier_args.gitlab_api_url.clone();
        let gitlab_repos = gitlab::enumerate_repo_urls(
            &repo_specifiers,
            api_url,
            global_args.ignore_certs,
            Some(&mut progress),
        )
        .await
        .context("Failed to enumerate GitLab repositories")?;

        for repo_string in gitlab_repos {
            match GitUrl::from_str(&repo_string) {
                Ok(repo_url) => {
                    repo_urls.push(repo_url);
                    num_found += 1;
                }
                Err(e) => {
                    progress.suspend(|| {
                        error!("Failed to parse repo URL from {repo_string}: {e}");
                    });
                }
            }
        }

        progress.finish_with_message(format!(
            "Found {} repositories from GitLab",
            HumanCount(num_found)
        ));
    }
    repo_urls.sort();
    repo_urls.dedup();
    Ok(repo_urls)
}

pub async fn fetch_jira_issues(
    args: &scan::ScanArgs,
    global_args: &global::GlobalArgs,
    datastore: &Arc<Mutex<findings_store::FindingsStore>>,
) -> Result<Vec<PathBuf>> {
    let Some(jira_url) = args.input_specifier_args.jira_url.clone() else {
        return Ok(Vec::new());
    };
    let Some(jql) = args.input_specifier_args.jql.as_deref() else {
        return Ok(Vec::new());
    };
    let max_results = args.input_specifier_args.max_results;
    let output_dir = {
        let ds = datastore.lock().unwrap();
        ds.clone_root()
    };
    let output_dir = output_dir.join("jira_issues");
    let _paths = jira::download_issues_to_dir(
        jira_url,
        jql,
        max_results,
        global_args.ignore_certs,
        &output_dir,
    )
    .await?;
    Ok(vec![output_dir])
}

pub async fn fetch_slack_messages(
    args: &scan::ScanArgs,
    global_args: &global::GlobalArgs,
    datastore: &Arc<Mutex<findings_store::FindingsStore>>,
) -> Result<Vec<PathBuf>> {
    let Some(query) = args.input_specifier_args.slack_query.as_deref() else {
        return Ok(Vec::new());
    };
    let api_url = args.input_specifier_args.slack_api_url.clone();
    let max_results = args.input_specifier_args.max_results;
    let output_root = {
        let ds = datastore.lock().unwrap();
        ds.clone_root()
    };
    let output_dir = output_root.join("slack_messages");
    let paths = slack::download_messages_to_dir(
        api_url,
        query,
        max_results,
        global_args.ignore_certs,
        &output_dir,
    )
    .await?;
    {
        let mut ds = datastore.lock().unwrap();
        for (path, link) in &paths {
            ds.register_slack_message(path.clone(), link.clone());
        }
    }
    Ok(vec![output_dir])
}

pub async fn fetch_s3_objects(
    args: &scan::ScanArgs,
    datastore: &Arc<Mutex<findings_store::FindingsStore>>,
    rules_db: &RulesDatabase,
    matcher_stats: &Mutex<MatcherStats>,
    enable_profiling: bool,
    shared_profiler: Arc<crate::rule_profiling::ConcurrentRuleProfiler>,
    progress_enabled: bool,
) -> Result<()> {
    let Some(bucket) = args.input_specifier_args.s3_bucket.as_deref() else {
        return Ok(());
    };
    let prefix = args.input_specifier_args.s3_prefix.as_deref();
    let role_arn = args.input_specifier_args.role_arn.as_deref();
    let profile = args.input_specifier_args.aws_local_profile.as_deref();

    let scanner_pool = Arc::new(ScannerPool::new(Arc::new(rules_db.vsdb.clone())));
    let seen_blobs = BlobIdMap::new();
    let matcher = Matcher::new(
        rules_db,
        scanner_pool,
        &seen_blobs,
        Some(matcher_stats),
        enable_profiling,
        Some(shared_profiler.clone()),
    )?;
    let guesser = Guesser::new().expect("should be able to create filetype guesser");
    let mut processor = BlobProcessor { matcher, guesser };

    let progress = if progress_enabled {
        let style =
            ProgressStyle::with_template("{spinner} {msg} ({pos} objects) [{elapsed_precise}]")
                .expect("progress bar style template should compile");
        let pb = ProgressBar::new_spinner().with_style(style).with_message("Fetching S3 objects");
        pb.enable_steady_tick(Duration::from_millis(500));
        pb
    } else {
        ProgressBar::hidden()
    };

    let pb = progress.clone();


    let bucket_name = bucket.to_string();

    s3::visit_bucket_objects(bucket, prefix, role_arn, profile, move |key, bytes| {
        let origin = OriginSet::new(
            Origin::from_extended(serde_json::json!({
                "path": format!("s3://{}/{}", bucket_name, key)
            })),
            Vec::new(),
        );
        let blob = crate::blob::Blob::from_bytes(bytes);

        if let Some((origin, blob_md, scored_matches)) =
            processor.run(origin, blob, args.no_dedup)?
        {
            // Wrap origin & metadata once:
            let origin_arc = Arc::new(origin);
            let blob_arc = Arc::new(blob_md);

            // Now build a batch of exactly one FindingsStoreMessage per Match
            let mut batch = Vec::with_capacity(scored_matches.len());
            for (_score, m) in scored_matches {
                batch.push((origin_arc.clone(), blob_arc.clone(), m));
            }

            // Call record with the right type
            let added = datastore.lock().unwrap().record(batch, !args.no_dedup);
            debug!("Added {} new S3 blobs", added);
        }
        pb.inc(1);
        Ok(())
    })
    .await?;

    let total = progress.position();
    progress.finish_with_message(format!("Fetched {} S3 objects", total));

    Ok(())
}
