// tests/integration_scan.rs

use std::{
    path::{Path, PathBuf},
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
    rule_loader::RuleLoader,
    rules_database::RulesDatabase,
    scanner::run_async_scan,
};
use tempfile::TempDir;
use url::Url;

#[derive(Debug)]
struct TestCase {
    file_name: &'static str,
    min_expected_findings: usize,
}

struct TestContext {
    rules_db: Arc<RulesDatabase>,
}

fn root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

impl TestContext {
    fn new() -> Result<Self> {
        let scan_args = ScanArgs {
            num_jobs: 2,
            rules: RuleSpecifierArgs {
                rules_path: Vec::new(),
                rule: vec!["all".into()],
                load_builtins: true,
            },
            input_specifier_args: InputSpecifierArgs {
                path_inputs: Vec::new(),
                git_url: Vec::new(),
                github_user: Vec::new(),
                github_organization: Vec::new(),
                github_exclude: Vec::new(),
                all_github_organizations: false,
                github_api_url: Url::parse("https://api.github.com/").unwrap(),
                github_repo_type: GitHubRepoType::Source,
                // new GitLab defaults
                gitlab_user: Vec::new(),
                gitlab_group: Vec::new(),
                gitlab_exclude: Vec::new(),
                all_gitlab_groups: false,
                gitlab_api_url: Url::parse("https://gitlab.com/").unwrap(),
                gitlab_repo_type: GitLabRepoType::Owner,
                gitlab_include_subgroups: false,

                huggingface_user: Vec::new(),
                huggingface_organization: Vec::new(),
                huggingface_model: Vec::new(),
                huggingface_dataset: Vec::new(),
                huggingface_space: Vec::new(),
                huggingface_exclude: Vec::new(),

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
                max_results: 100,
                slack_query: None,
                slack_api_url: Url::parse("https://slack.com/api/").unwrap(),
                // s3
                s3_bucket: None,
                s3_prefix: None,
                role_arn: None,
                aws_local_profile: None,
                gcs_bucket: None,
                gcs_prefix: None,
                gcs_service_account: None,
                // Docker image scanning
                docker_image: Vec::new(),
                // git clone / history options
                git_clone: GitCloneMode::Bare,
                git_history: GitHistoryMode::Full,
                commit_metadata: true,
                repo_artifacts: false,
                scan_nested_repos: true,
                since_commit: None,
                branch: None,
            },
            content_filtering_args: ContentFilteringArgs {
                max_file_size_mb: 25.0,
                extraction_depth: 2,
                no_binary: true,
                no_extract_archives: false,
                exclude: Vec::new(), // Exclude patterns
            },
            confidence: ConfidenceLevel::Low,
            no_validate: true,
            rule_stats: false,
            only_valid: false,
            min_entropy: Some(0.0),
            redact: false,
            git_repo_timeout: 1800, // 30 minutes
            output_args: OutputArgs { output: None, format: ReportOutputFormat::Pretty },
            no_dedup: true,
            baseline_file: None,
            manage_baseline: false,
            skip_regex: Vec::new(),
            skip_word: Vec::new(),
            skip_aws_account: Vec::new(),
            skip_aws_account_file: None,
            no_base64: false,
            extra_ignore_comments: Vec::new(),
            no_inline_ignore: false,
        };

        let loaded = RuleLoader::from_rule_specifiers(&scan_args.rules)
            .load(&scan_args)
            .context("Failed to load rules")?;

        let resolved = loaded.resolve_enabled_rules().context("Failed to resolve rules")?;

        let rules_db = RulesDatabase::from_rules(resolved.into_iter().cloned().collect())
            .context("Failed to compile rules")?;

        Ok(Self { rules_db: Arc::new(rules_db) })
    }

    async fn scan_file(&self, file_path: &Path) -> Result<usize> {
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let clone_dir = temp_dir.path().to_path_buf();

        let scan_args = ScanArgs {
            num_jobs: 2,
            rules: RuleSpecifierArgs {
                rules_path: Vec::new(),
                rule: vec!["all".into()],
                load_builtins: true,
            },
            input_specifier_args: InputSpecifierArgs {
                path_inputs: vec![file_path.to_path_buf()],
                git_url: Vec::new(),
                github_user: Vec::new(),
                github_organization: Vec::new(),
                github_exclude: Vec::new(),
                all_github_organizations: false,
                github_api_url: Url::parse("https://api.github.com/").unwrap(),
                github_repo_type: GitHubRepoType::Source,
                // new GitLab defaults
                gitlab_user: Vec::new(),
                gitlab_group: Vec::new(),
                gitlab_exclude: Vec::new(),
                all_gitlab_groups: false,
                gitlab_api_url: Url::parse("https://gitlab.com/").unwrap(),
                gitlab_repo_type: GitLabRepoType::Owner,
                gitlab_include_subgroups: false,

                huggingface_user: Vec::new(),
                huggingface_organization: Vec::new(),
                huggingface_model: Vec::new(),
                huggingface_dataset: Vec::new(),
                huggingface_space: Vec::new(),
                huggingface_exclude: Vec::new(),

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
                max_results: 100,
                slack_query: None,
                slack_api_url: Url::parse("https://slack.com/api/").unwrap(),
                // s3
                s3_bucket: None,
                s3_prefix: None,
                role_arn: None,
                aws_local_profile: None,
                // Docker image scanning
                docker_image: Vec::new(),
                // git clone / history options
                git_clone: GitCloneMode::Bare,
                git_history: GitHistoryMode::Full,
                commit_metadata: true,
                repo_artifacts: false,
                scan_nested_repos: true,
                since_commit: None,
                branch: None,

                gcs_bucket: None,
                gcs_prefix: None,
                gcs_service_account: None,
            },
            extra_ignore_comments: Vec::new(),
            content_filtering_args: ContentFilteringArgs {
                max_file_size_mb: 25.0,
                extraction_depth: 2,
                no_binary: true,
                no_extract_archives: false,
                exclude: Vec::new(), // Exclude patterns
            },
            confidence: ConfidenceLevel::Low,
            no_validate: true,
            rule_stats: false,
            only_valid: false,
            min_entropy: Some(0.0),
            redact: false,
            git_repo_timeout: 1800, // 30 minutes
            output_args: OutputArgs { output: None, format: ReportOutputFormat::Pretty },
            no_dedup: true,
            baseline_file: None,
            manage_baseline: false,
            skip_regex: Vec::new(),
            skip_word: Vec::new(),
            skip_aws_account: Vec::new(),
            skip_aws_account_file: None,
            no_base64: false,
            no_inline_ignore: false,
        };

        let global_args = GlobalArgs {
            verbose: 0,
            quiet: true,
            color: Mode::Auto,
            no_update_check: false,
            self_update: false,
            progress: Mode::Never,
            ignore_certs: false,
            user_agent_suffix: None,
        };

        let datastore = Arc::new(Mutex::new(FindingsStore::new(clone_dir)));

        run_async_scan(&global_args, &scan_args, Arc::clone(&datastore), &self.rules_db).await?;

        let findings = {
            let ds = datastore.lock().unwrap();
            ds.get_matches().len()
        };

        Ok(findings)
    }
}

#[tokio::test]
async fn test_scan_vulnerable_files() -> Result<()> {
    let test_context = TestContext::new()?;

    let test_cases = vec![
        TestCase { file_name: "testdata/c_vulnerable.c", min_expected_findings: 3 },
        TestCase { file_name: "testdata/cpp_vulnerable.cpp", min_expected_findings: 3 },
        TestCase { file_name: "testdata/csharp_vulnerable.cs", min_expected_findings: 4 },
        TestCase { file_name: "testdata/elixir_vulnerable.exs", min_expected_findings: 1 },
        TestCase { file_name: "testdata/generic_secrets.py", min_expected_findings: 9 },
        TestCase { file_name: "testdata/go_vulnerable.go", min_expected_findings: 4 },
        TestCase { file_name: "testdata/java_vulnerable.java", min_expected_findings: 4 },
        TestCase { file_name: "testdata/javascript_vulnerable.js", min_expected_findings: 4 },
        TestCase { file_name: "testdata/json_vulnerable.json", min_expected_findings: 4 },
        TestCase { file_name: "testdata/kotlin_vulnerable.kt", min_expected_findings: 7 },
        TestCase { file_name: "testdata/objc_vulnerable.m", min_expected_findings: 4 },
        TestCase { file_name: "testdata/php_vulnerable.php", min_expected_findings: 5 },
        TestCase { file_name: "testdata/python_vulnerable.py", min_expected_findings: 10 },
        TestCase { file_name: "testdata/python2_vulnerable.py", min_expected_findings: 4 },
        TestCase { file_name: "testdata/ruby_vulnerable.rb", min_expected_findings: 6 },
        TestCase { file_name: "testdata/rust_vulnerable.rs", min_expected_findings: 3 },
        TestCase { file_name: "testdata/scala_vulnerable.scala", min_expected_findings: 3 },
        TestCase { file_name: "testdata/shell_vulnerable.sh", min_expected_findings: 2 },
        TestCase { file_name: "testdata/slack_tokens.properties", min_expected_findings: 17 },
        TestCase { file_name: "testdata/swift_vulnerable.swift", min_expected_findings: 2 },
        TestCase { file_name: "testdata/toml_vulnerable.toml", min_expected_findings: 4 },
        TestCase { file_name: "testdata/tsx_vulnerable.tsx", min_expected_findings: 1 },
        TestCase { file_name: "testdata/typescript_vulnerable.ts", min_expected_findings: 1 },
        TestCase { file_name: "testdata/yaml_vulnerable.yaml", min_expected_findings: 4 },
    ];

    let root = root_dir();

    for test_case in test_cases {
        let test_file = root.join(test_case.file_name);
        println!("Testing file: {}", test_case.file_name);

        let findings = test_context.scan_file(&test_file).await?;

        assert!(
            findings >= test_case.min_expected_findings,
            "File: {} - Expected >= {} findings, got {}",
            test_case.file_name,
            test_case.min_expected_findings,
            findings
        );
    }

    Ok(())
}
