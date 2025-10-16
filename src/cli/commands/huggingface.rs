use clap::{Args, Subcommand};

use crate::cli::commands::output::OutputArgs;

use super::github::GitHubOutputFormat;

/// Top-level Hugging Face command group
#[derive(Args, Debug)]
pub struct HuggingFaceArgs {
    #[command(subcommand)]
    pub command: HuggingFaceCommand,
}

#[derive(Subcommand, Debug)]
pub enum HuggingFaceCommand {
    /// Interact with Hugging Face repositories
    #[command(subcommand)]
    Repos(HuggingFaceReposCommand),
}

#[derive(Subcommand, Debug)]
pub enum HuggingFaceReposCommand {
    /// List Hugging Face repositories
    List(HuggingFaceReposListArgs),
}

#[derive(Args, Debug, Clone)]
pub struct HuggingFaceReposListArgs {
    #[command(flatten)]
    pub repo_specifiers: HuggingFaceRepoSpecifiers,

    #[command(flatten)]
    pub output_args: OutputArgs<HuggingFaceOutputFormat>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct HuggingFaceRepoSpecifiers {
    /// Models, datasets, and Spaces owned by these users
    #[arg(long = "huggingface-user")]
    pub user: Vec<String>,

    /// Models, datasets, and Spaces owned by these organizations
    #[arg(long = "huggingface-organization", alias = "huggingface-org")]
    pub organization: Vec<String>,

    /// Specific models to scan (format: owner/name or full URL)
    #[arg(long = "huggingface-model")]
    pub model: Vec<String>,

    /// Specific datasets to scan (format: owner/name or full URL)
    #[arg(long = "huggingface-dataset")]
    pub dataset: Vec<String>,

    /// Specific Spaces to scan (format: owner/name or full URL)
    #[arg(long = "huggingface-space")]
    pub space: Vec<String>,

    /// Skip specific repositories during enumeration (accepts optional prefixes like model:, dataset:, or space:)
    #[arg(long = "huggingface-exclude", value_name = "IDENTIFIER")]
    pub exclude: Vec<String>,
}

impl HuggingFaceRepoSpecifiers {
    pub fn is_empty(&self) -> bool {
        self.user.is_empty()
            && self.organization.is_empty()
            && self.model.is_empty()
            && self.dataset.is_empty()
            && self.space.is_empty()
    }
}

pub type HuggingFaceOutputFormat = GitHubOutputFormat;
