use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use kingfisher::{
    cli::{
        commands::{
            azure::AzureRepoType,
            bitbucket::{BitbucketAuthArgs, BitbucketRepoType},
            gitea::GiteaRepoType,
            github::{GitCloneMode, GitHistoryMode, GitHubRepoType},
            gitlab::GitLabRepoType,
            inputs::{ContentFilteringArgs, InputSpecifierArgs},
            output::{OutputArgs, ReportOutputFormat},
            rules::RuleSpecifierArgs,
            scan::{ConfidenceLevel, ScanArgs},
        },
        global::Mode,
        GlobalArgs,
    },
    findings_store::FindingsStore,
    git_url::GitUrl,
    scanner::{load_and_record_rules, run_scan},
};
use tempfile::TempDir;
use tokio::runtime::Runtime;
use url::Url;

fn determine_exit_code(total: usize, validated: usize) -> i32 {
    match (total, validated) {
        (0, _) => 0,
        (_, v) if v > 0 => 205,
        _ => 200,
    }
}

#[test]
fn test_bitbucket_remote_scan() -> Result<()> {
    let temp_dir = TempDir::new().context("tmp dir")?;
    let clone_dir = temp_dir.path().to_path_buf();

    let repo_url = "https://bitbucket.org/hashashash/secretstest.git";
    let git_url = GitUrl::from_str(repo_url).expect("parse Bitbucket URL");

    let scan_args = ScanArgs {
        num_jobs: 2,
        rules: RuleSpecifierArgs {
            rules_path: Vec::new(),
            rule: vec!["all".into()],
            load_builtins: true,
        },
        input_specifier_args: InputSpecifierArgs {
            path_inputs: Vec::new(),
            git_url: vec![git_url],
            github_user: Vec::new(),
            github_organization: Vec::new(),
            github_exclude: Vec::new(),
            all_github_organizations: false,
            github_api_url: Url::parse("https://api.github.com/")?,
            github_repo_type: GitHubRepoType::Source,
            gitlab_user: Vec::new(),
            gitlab_group: Vec::new(),
            gitlab_exclude: Vec::new(),
            all_gitlab_groups: false,
            gitlab_api_url: Url::parse("https://gitlab.com/")?,
            gitlab_repo_type: GitLabRepoType::Owner,
            gitlab_include_subgroups: false,

            gitea_user: Vec::new(),
            gitea_organization: Vec::new(),
            gitea_exclude: Vec::new(),
            all_gitea_organizations: false,
            gitea_api_url: Url::parse("https://gitea.com/api/v1/")?,
            gitea_repo_type: GiteaRepoType::Source,

            bitbucket_user: Vec::new(),
            bitbucket_workspace: Vec::new(),
            bitbucket_project: Vec::new(),
            bitbucket_exclude: Vec::new(),
            all_bitbucket_workspaces: false,
            bitbucket_api_url: Url::parse("https://api.bitbucket.org/2.0/")?,
            bitbucket_repo_type: BitbucketRepoType::Source,
            bitbucket_auth: BitbucketAuthArgs::default(),

            azure_organization: Vec::new(),
            azure_project: Vec::new(),
            azure_exclude: Vec::new(),
            all_azure_projects: false,
            azure_base_url: Url::parse("https://dev.azure.com/")?,
            azure_repo_type: AzureRepoType::Source,

            jira_url: None,
            jql: None,
            confluence_url: None,
            cql: None,
            max_results: 100,
            slack_query: None,
            slack_api_url: Url::parse("https://slack.com/api/").unwrap(),
            s3_bucket: None,
            s3_prefix: None,
            role_arn: None,
            aws_local_profile: None,
            docker_image: Vec::new(),
            git_clone: GitCloneMode::Bare,
            git_history: GitHistoryMode::Full,
            commit_metadata: true,
            repo_artifacts: false,
            scan_nested_repos: true,
            since_commit: None,
            branch: None,
        },
        compat_ignore_comments: false,
        content_filtering_args: ContentFilteringArgs {
            max_file_size_mb: 25.0,
            no_extract_archives: false,
            extraction_depth: 2,
            no_binary: true,
            exclude: Vec::new(),
        },
        confidence: ConfidenceLevel::Medium,
        no_validate: false,
        rule_stats: false,
        only_valid: false,
        min_entropy: None,
        redact: false,
        git_repo_timeout: 1800,
        output_args: OutputArgs { output: None, format: ReportOutputFormat::Pretty },
        no_dedup: true,
        baseline_file: None,
        manage_baseline: false,
        skip_regex: Vec::new(),
        skip_word: Vec::new(),
        no_base64: false,
    };

    let global_args = GlobalArgs {
        verbose: 0,
        quiet: false,
        color: Mode::Auto,
        progress: Mode::Auto,
        no_update_check: false,
        self_update: false,
        ignore_certs: false,
        user_agent_suffix: None,
    };

    let datastore = Arc::new(Mutex::new(FindingsStore::new(clone_dir)));
    let runtime = Runtime::new()?;
    let rules_db = Arc::new(load_and_record_rules(&scan_args, &datastore)?);

    runtime.block_on(async {
        run_scan(&global_args, &scan_args, &rules_db, Arc::clone(&datastore)).await
    })?;

    let ds = datastore.lock().unwrap();
    let findings = ds.get_matches();
    let total = findings.len();
    let validated = findings.iter().filter(|m| m.as_ref().2.validation_success).count();

    assert!(total >= 5, "expected at least 5 findings from Bitbucket repo, got {total}");

    let exit_code = determine_exit_code(total, validated);
    assert!(
        exit_code >= 200,
        "expected findings from Bitbucket repo (exit_code >= 200), got {exit_code}"
    );

    drop(runtime);
    Ok(())
}
