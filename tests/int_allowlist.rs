use std::{
    fs,
    sync::{Arc, Mutex},
};

use anyhow::Result;
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
    rule_loader::RuleLoader,
    rules_database::RulesDatabase,
    scanner::run_async_scan,
};
use tempfile::TempDir;
use tokio::runtime::Runtime;
use url::Url;

fn run_skiplist(skip_regex: Vec<String>, skip_skipword: Vec<String>) -> Result<usize> {
    let rt = Runtime::new().unwrap();
    let work = TempDir::new()?;
    let rules_dir = work.path().join("rules");
    fs::create_dir_all(&rules_dir)?;
    let inputs_dir = work.path().join("in");
    fs::create_dir_all(&inputs_dir)?;

    fs::write(
        rules_dir.join("demo.yml"),
        r#"rules:
  - id: demo.token
    name: Demo token
    pattern: 'token_(\w+)'
    confidence: low
"#,
    )?;

    fs::write(inputs_dir.join("a.txt"), "token_realvalue\ntoken_testvalue\n")?;

    let scan_args = ScanArgs {
        num_jobs: 2,
        rules: RuleSpecifierArgs {
            rules_path: vec![rules_dir.clone()],
            rule: vec!["all".into()],
            load_builtins: false,
        },
        input_specifier_args: InputSpecifierArgs {
            path_inputs: vec![inputs_dir.join("a.txt")],
            git_url: Vec::new(),
            github_user: Vec::new(),
            github_organization: Vec::new(),
            github_exclude: Vec::new(),
            all_github_organizations: false,
            github_api_url: Url::parse("https://api.github.com/").unwrap(),
            github_repo_type: GitHubRepoType::Source,
            gitlab_user: Vec::new(),
            gitlab_group: Vec::new(),
            gitlab_exclude: Vec::new(),
            all_gitlab_groups: false,
            gitlab_api_url: Url::parse("https://gitlab.com/").unwrap(),
            gitlab_repo_type: GitLabRepoType::Owner,
            gitlab_include_subgroups: false,
            gitea_user: Vec::new(),
            gitea_organization: Vec::new(),
            gitea_exclude: Vec::new(),
            all_gitea_organizations: false,
            gitea_api_url: Url::parse("https://gitea.com/api/v1/").unwrap(),
            gitea_repo_type: GiteaRepoType::Source,
            bitbucket_user: Vec::new(),
            bitbucket_workspace: Vec::new(),
            bitbucket_project: Vec::new(),
            bitbucket_exclude: Vec::new(),
            all_bitbucket_workspaces: false,
            bitbucket_api_url: Url::parse("https://api.bitbucket.org/2.0/").unwrap(),
            bitbucket_repo_type: BitbucketRepoType::Source,
            bitbucket_auth: BitbucketAuthArgs::default(),
            azure_organization: Vec::new(),
            azure_project: Vec::new(),
            azure_exclude: Vec::new(),
            all_azure_projects: false,
            azure_base_url: Url::parse("https://dev.azure.com/").unwrap(),
            azure_repo_type: AzureRepoType::Source,
            jira_url: None,
            jql: None,
            confluence_url: None,
            cql: None,
            slack_query: None,
            slack_api_url: Url::parse("https://slack.com/api/").unwrap(),
            max_results: 100,
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
        extra_ignore_comments: Vec::new(),
        content_filtering_args: ContentFilteringArgs {
            max_file_size_mb: 5.0,
            exclude: Vec::new(),
            no_extract_archives: false,
            extraction_depth: 1,
            no_binary: true,
        },
        confidence: ConfidenceLevel::Low,
        no_validate: true,
        rule_stats: false,
        only_valid: false,
        min_entropy: Some(0.0),
        redact: false,
        git_repo_timeout: 1800,
        output_args: OutputArgs { output: None, format: ReportOutputFormat::Pretty },
        no_dedup: false,
        baseline_file: None,
        manage_baseline: false,
        skip_regex: skip_regex,
        skip_word: skip_skipword,
        no_base64: false,
        no_inline_ignore: false,
    };

    let global_args = GlobalArgs {
        verbose: 0,
        quiet: true,
        color: Mode::Never,
        progress: Mode::Never,
        no_update_check: true,
        self_update: false,
        ignore_certs: false,
        user_agent_suffix: None,
    };

    let loaded = RuleLoader::from_rule_specifiers(&scan_args.rules).load(&scan_args)?;
    let resolved = loaded.resolve_enabled_rules()?;
    let rules_db = Arc::new(RulesDatabase::from_rules(resolved.into_iter().cloned().collect())?);

    let datastore = Arc::new(Mutex::new(FindingsStore::new(work.path().join("store"))));

    rt.block_on(run_async_scan(&global_args, &scan_args, Arc::clone(&datastore), &rules_db))?;

    let x = Ok(datastore.lock().unwrap().get_matches().len());
    x
}

#[test]
fn skip_regex_filters_match() -> Result<()> {
    let count = run_skiplist(vec!["token_realvalue".into()], Vec::new())?;
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn skip_skipword_filters_match() -> Result<()> {
    let count = run_skiplist(Vec::new(), vec!["test".into()])?;
    assert_eq!(count, 1);
    Ok(())
}
