//! CLI definitions for the zkVM benchmarker

use anyhow::{Result as AnyhowResult, bail};
use benchmark_runner::{runner::Action, stateless_validator};
use clap::{Parser, Subcommand, ValueEnum};
use ere_dockerized::{ProverResource, RemoteProverConfig, zkVMKind};
use std::path::PathBuf;
use std::time::Duration;

/// Command line interface for the zkVM benchmarker
#[derive(Parser)]
#[command(name = "zkvm-benchmarker")]
#[command(about = "Benchmark different Ere compatible zkVMs")]
#[command(version)]
#[derive(Debug)]
pub struct Cli {
    /// Resource type for proving
    #[arg(short, long, value_enum, default_value = "cpu")]
    pub resource: Resource,

    /// Endpoint URL of the proving cluster (required when --resource cluster)
    #[arg(long, required_if_eq("resource", "cluster"))]
    pub cluster_endpoint: Option<String>,

    /// Action to perform
    #[arg(short, long, value_enum, default_value = "execute")]
    pub action: BenchmarkAction,

    /// zkVM instances to benchmark
    #[arg(long, required(true), value_parser = <zkVMKind as std::str::FromStr>::from_str)]
    pub zkvms: Vec<zkVMKind>,

    /// Rerun the benchmarks even if the output folder already contains results
    #[arg(long, default_value_t = false)]
    pub force_rerun: bool,

    /// Guest program to benchmark
    #[command(subcommand)]
    pub guest_program: GuestProgramCommand,

    /// Output folder for benchmark results
    #[arg(short, long, default_value = "zkevm-metrics")]
    pub output_folder: PathBuf,

    /// Output folder for dumping input files used in benchmarks
    #[arg(long)]
    pub dump_inputs: Option<PathBuf>,

    /// Save generated proofs to the specified folder (only valid with --action prove)
    #[arg(long)]
    pub save_proofs: Option<PathBuf>,

    /// Folder containing saved proofs (used with --action verify)
    #[arg(
        long,
        default_value = "zkevm-fixtures-proofs",
        conflicts_with = "proofs_url"
    )]
    pub proofs_folder: PathBuf,

    /// URL to a .tar.gz archive containing proofs (used with --action verify).
    #[arg(long, conflicts_with = "proofs_folder")]
    pub proofs_url: Option<String>,

    /// Base path for pre-compiled guest program binaries. If not set, they will be downloaded
    /// from the resolved ere-guests release or commit artifacts.
    #[arg(long, conflicts_with = "guest_artifact_base_url")]
    pub bin_path: Option<PathBuf>,

    /// Base URL for pre-compiled guest program artifacts.
    #[arg(long, conflicts_with = "bin_path")]
    pub guest_artifact_base_url: Option<String>,

    /// Timeout for the selected action only, for example `15m`, `5m`, or `2s`.
    #[arg(long, value_name = "DURATION", value_parser = parse_duration)]
    pub timeout: Option<Duration>,

    /// Enable Zisk profiling (requires --zkvms zisk, --action execute)
    #[arg(long)]
    pub zisk_profile: bool,

    /// Output folder for Zisk profile results
    #[arg(long, default_value = "zisk-profiles")]
    pub zisk_profile_output: PathBuf,
}

/// Subcommands for different guest programs
#[derive(Subcommand, Clone, Debug)]
pub enum GuestProgramCommand {
    /// Ethereum Stateless Validator
    StatelessValidator {
        /// EEST fixture file or folder (required for execute and prove; ignored for verify)
        #[arg(short, long)]
        input_folder: Option<PathBuf>,
        /// Fixture name prefix to run. Repeat to select multiple prefixes.
        #[arg(long, value_name = "PREFIX")]
        fixture: Option<Vec<String>>,
        /// Execution client to benchmark
        #[arg(short, long)]
        execution_client: ExecutionClient,
    },
}

/// Execution clients for the stateless validator
#[derive(Debug, Copy, Clone, ValueEnum)]
pub enum ExecutionClient {
    /// Reth execution client
    Reth,
    /// Ethrex execution client
    Ethrex,
    /// Zesu execution client
    Zesu,
}

/// Prover resource types
#[derive(Debug, Clone, ValueEnum)]
pub enum Resource {
    /// CPU resource
    Cpu,
    /// GPU resource
    Gpu,
    /// Proving cluster (requires --cluster-endpoint)
    Cluster,
}

/// Benchmark actions
#[derive(Debug, Clone, ValueEnum)]
pub enum BenchmarkAction {
    /// Only do zkVM execution
    Execute,
    /// Create a zkVM proof
    Prove,
    /// Verify proofs loaded from disk
    Verify,
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    humantime::parse_duration(value).map_err(|err| err.to_string())
}

impl Cli {
    /// Validate arguments whose requirements depend on the selected action.
    pub fn validate(&self) -> AnyhowResult<()> {
        if matches!(self.action, BenchmarkAction::Verify) {
            return Ok(());
        }

        let GuestProgramCommand::StatelessValidator { input_folder, .. } = &self.guest_program;
        let Some(input_folder) = input_folder else {
            bail!("--input-folder is required with --action execute or --action prove");
        };
        if !input_folder.exists() {
            bail!("input path does not exist: {}", input_folder.display());
        }

        Ok(())
    }

    /// Build the Ere [`ProverResource`] from parsed CLI args.
    pub fn prover_resource(&self) -> ProverResource {
        match self.resource {
            Resource::Cpu => ProverResource::Cpu,
            Resource::Gpu => ProverResource::Gpu,
            Resource::Cluster => ProverResource::Cluster(RemoteProverConfig {
                endpoint: self
                    .cluster_endpoint
                    .clone()
                    .expect("clap required_if_eq should guarantee cluster_endpoint set"),
                api_key: None,
            }),
        }
    }
}

impl From<BenchmarkAction> for Action {
    fn from(action: BenchmarkAction) -> Self {
        match action {
            BenchmarkAction::Execute => Self::Execute,
            BenchmarkAction::Prove => Self::Prove,
            BenchmarkAction::Verify => Self::Verify,
        }
    }
}

impl From<ExecutionClient> for stateless_validator::ExecutionClient {
    fn from(client: ExecutionClient) -> Self {
        match client {
            ExecutionClient::Reth => Self::Reth,
            ExecutionClient::Ethrex => Self::Ethrex,
            ExecutionClient::Zesu => Self::Zesu,
        }
    }
}
