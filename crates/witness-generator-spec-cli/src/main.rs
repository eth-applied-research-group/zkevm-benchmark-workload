//! CLI entry point for generating and collecting canonical stateless fixtures.

#![allow(
    unused_crate_dependencies,
    reason = "library dependencies are compiled separately from this CLI target"
)]

mod artifact;
mod catalog;
mod collector;
mod config;
mod export;
mod publish;

use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use config::CollectorConfig;
use tracing::info;
use tracing_subscriber::EnvFilter;
use witness_generator_spec_cli::{BlockSelector, NetworkWitnessClient, NetworkWitnessConfig};

#[derive(Debug, Parser)]
#[command(
    name = "witness-generator-spec-cli",
    about = "Generate and collect canonical Amsterdam stateless guest fixtures from CL/EL RPC endpoints.",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Consensus-layer Beacon API endpoint for legacy one-shot generation.
    #[arg(long)]
    cl_url: Option<String>,
    /// Execution-layer JSON-RPC endpoint for legacy one-shot generation.
    #[arg(long)]
    el_url: Option<String>,
    /// Beacon API block id: head, finalized, slot, or block root. Defaults to head.
    #[arg(long)]
    block_id: Option<String>,
    /// Execution block number to resolve to a CL slot via block timestamp.
    #[arg(long, conflicts_with = "block_id")]
    execution_block_number: Option<u64>,
    /// Output file. Stdout is used when omitted.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate one benchmark-ready EEST fixture.
    Generate(GenerateArgs),
    /// Poll the live network head and store one artifact per observed block.
    Collect(CollectArgs),
    /// Package complete local block ranges into downloadable batch archives.
    Export(ExportArgs),
    /// Publish exported batches and indexes to Cloudflare R2.
    PublishR2(PublishR2Args),
}

#[derive(Debug, Clone, Args)]
struct GenerateArgs {
    /// Consensus-layer Beacon API endpoint.
    #[arg(long)]
    cl_url: String,
    /// Execution-layer JSON-RPC endpoint.
    #[arg(long)]
    el_url: String,
    /// Beacon API block id: head, finalized, slot, or block root. Defaults to head.
    #[arg(long)]
    block_id: Option<String>,
    /// Execution block number to resolve to a CL slot via block timestamp.
    #[arg(long, conflicts_with = "block_id")]
    execution_block_number: Option<u64>,
    /// Output file. Stdout is used when omitted.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
struct CollectArgs {
    /// TOML config path.
    #[arg(long)]
    config: PathBuf,
    /// Collect one head block and exit.
    #[arg(long)]
    once: bool,
}

#[derive(Debug, Clone, Args)]
struct ExportArgs {
    /// TOML config path.
    #[arg(long)]
    config: PathBuf,
    /// Rebuild existing batch archives.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Clone, Args)]
struct PublishR2Args {
    /// TOML config path.
    #[arg(long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Command::Generate(args)) => run_generate(args).await,
        Some(Command::Collect(args)) => {
            let config = CollectorConfig::from_path(args.config)?;
            collector::collect(config, args.once).await
        }
        Some(Command::Export(args)) => {
            let config = CollectorConfig::from_path(args.config)?;
            let exported = export::export_batches(&config, args.force)?;
            let catalog = catalog::generate_catalog(&config, &exported)?;
            info!(count = exported.len(), "exported batch archives");
            info!(
                artifacts = catalog.artifact_count,
                batches = catalog.batch_count,
                fresh = catalog.fresh_batch_count,
                cached = catalog.cached_batch_count,
                seeded = catalog.seeded_batch_count,
                inspected = catalog.inspected_batch_count,
                "generated public catalog"
            );
            Ok(())
        }
        Some(Command::PublishR2(args)) => {
            let config = CollectorConfig::from_path(args.config)?;
            publish::publish_r2(&config)
        }
        None => run_generate(cli.into_generate_args()?).await,
    }
}

impl Cli {
    fn into_generate_args(self) -> anyhow::Result<GenerateArgs> {
        Ok(GenerateArgs {
            cl_url: self
                .cl_url
                .context("--cl-url is required when no subcommand is used")?,
            el_url: self
                .el_url
                .context("--el-url is required when no subcommand is used")?,
            block_id: self.block_id,
            execution_block_number: self.execution_block_number,
            out: self.out,
        })
    }
}

async fn run_generate(args: GenerateArgs) -> anyhow::Result<()> {
    let selector = block_selector(args.block_id.as_deref(), args.execution_block_number);
    let client = NetworkWitnessClient::new(NetworkWitnessConfig::new(args.cl_url, args.el_url))?;
    let generated = client.stateless_input_bytes(selector).await?;
    let output = artifact::one_shot_fixture_json(&generated)?;

    if let Some(path) = args.out {
        fs::write(path, output)?;
    } else {
        io::stdout().write_all(&output)?;
    }

    Ok(())
}

fn block_selector(block_id: Option<&str>, execution_block_number: Option<u64>) -> BlockSelector {
    match execution_block_number {
        Some(number) => BlockSelector::ExecutionBlockNumber(number),
        None => match block_id.unwrap_or("head") {
            "head" => BlockSelector::Head,
            other => BlockSelector::BeaconBlockId(other.to_owned()),
        },
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn parses_legacy_generate_invocation() {
        let cli = Cli::try_parse_from([
            "witness-generator-spec-cli",
            "--cl-url",
            "http://cl",
            "--el-url",
            "http://el",
        ])
        .unwrap();

        assert!(cli.command.is_none());
        let args = cli.into_generate_args().unwrap();
        assert_eq!(args.cl_url, "http://cl");
        assert_eq!(args.el_url, "http://el");
    }

    #[test]
    fn parses_generate_subcommand() {
        let cli = Cli::try_parse_from([
            "witness-generator-spec-cli",
            "generate",
            "--cl-url",
            "http://cl",
            "--el-url",
            "http://el",
        ])
        .unwrap();

        let Some(Command::Generate(args)) = cli.command else {
            panic!("expected generate subcommand");
        };
        assert_eq!(args.cl_url, "http://cl");
        assert_eq!(args.el_url, "http://el");
    }

    #[test]
    fn parses_operational_subcommands() {
        for command in ["collect", "export", "publish-r2"] {
            Cli::try_parse_from([
                "witness-generator-spec-cli",
                command,
                "--config",
                "/etc/witness-generator-spec-cli/glamsterdam-devnet-5.toml",
            ])
            .unwrap();
        }
    }

    #[test]
    fn clap_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
