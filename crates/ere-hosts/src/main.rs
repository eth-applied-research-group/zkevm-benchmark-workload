//! Binary for benchmarking different Ere compatible zkVMs

#![cfg_attr(not(test), warn(unused_crate_dependencies))]

use anyhow::{Context, Result, bail};
use benchmark_runner::{
    runner::{
        Action, GuestProgramSource, ProfileConfig, RunConfig, benchmark_output_dir,
        get_el_zkvm_instances, run_benchmark_iter,
    },
    stateless_validator::{self},
    verification::{download_and_extract_proofs, resolve_extracted_root, run_verify_from_disk},
};
use ere_dockerized::{DockerizedzkVMConfig, ProverResource, zkVMKind};

use clap::Parser;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, GuestProgramCommand};

pub mod cli;

const DEFAULT_EXECUTE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const DEFAULT_PROVE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const DEFAULT_VERIFY_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    cli.validate()?;

    if cli.zisk_profile {
        if !matches!(cli.action, cli::BenchmarkAction::Execute) {
            bail!(
                "--zisk-profile requires --action execute, but got {:?}",
                cli.action
            );
        }
        if cli.zkvms.len() != 1 || cli.zkvms[0] != zkVMKind::Zisk {
            let zkvm_names: Vec<_> = cli.zkvms.iter().map(|z| z.as_str()).collect();
            bail!(
                "--zisk-profile requires --zkvms zisk only, but got: {}",
                zkvm_names.join(", ")
            );
        }
    }

    let resource: ProverResource = cli.prover_resource();
    let action: Action = cli.action.into();
    let zkvm_config = build_zkvm_config(action, cli.timeout);
    info!(
        "Running benchmarks with resource={:?} and action={:?}",
        resource, action
    );

    let zisk_profile_config = cli
        .zisk_profile
        .then(|| ProfileConfig::new(cli.zisk_profile_output.clone()));

    // Validate: --save-proofs is only valid with --action prove
    if cli.save_proofs.is_some() && !matches!(action, Action::Prove) {
        anyhow::bail!("--save-proofs is only valid with --action prove");
    }

    // Validate: --proofs-url is only valid with --action verify
    if cli.proofs_url.is_some() && !matches!(action, Action::Verify) {
        anyhow::bail!("--proofs-url is only valid with --action verify");
    }

    // Validate: --cluster-endpoint is only valid with --resource cluster
    if cli.cluster_endpoint.is_some() && !matches!(cli.resource, cli::Resource::Cluster) {
        anyhow::bail!("--cluster-endpoint is only valid with --resource cluster");
    }

    // Validate: --resource cluster currently only supports zisk zkVM and not support --action execute
    if matches!(cli.resource, cli::Resource::Cluster) {
        if cli.zkvms.iter().any(|z| *z != zkVMKind::Zisk) {
            anyhow::bail!("--resource cluster is only implemented for --zkvms zisk");
        }
        if matches!(action, Action::Execute) {
            anyhow::bail!("--resource cluster is not implemented for --action execute");
        }
    }

    // Resolve proofs source: download from URL or use local folder.
    // _proofs_tmpdir must live until verification completes (drop = cleanup).
    let (_proofs_tmpdir, proofs_folder) = if let Some(ref url) = cli.proofs_url {
        let tmp = download_and_extract_proofs(url).await?;
        let resolved = resolve_extracted_root(tmp.path())?;
        (Some(tmp), resolved)
    } else {
        (None, cli.proofs_folder)
    };
    let guest_source = match (cli.bin_path, cli.guest_artifact_base_url) {
        (Some(path), None) => GuestProgramSource::LocalPath(path),
        (None, Some(url)) => GuestProgramSource::ArtifactBaseUrl(url),
        (None, None) => GuestProgramSource::Default,
        (Some(_), Some(_)) => unreachable!("clap conflicts_with should reject this combination"),
    };
    let config_base = RunConfig {
        output_folder: cli.output_folder,
        sub_folder: None,
        action,
        force_rerun: cli.force_rerun,
        dump_inputs_folder: cli.dump_inputs,
        zisk_profile_config,
        save_proofs_folder: cli.save_proofs,
    };

    match cli.guest_program {
        GuestProgramCommand::StatelessValidator {
            input_folder,
            fixture,
            execution_client,
        } => {
            let el: stateless_validator::ExecutionClient = execution_client.into();
            validate_guest_compatibility(el, &cli.zkvms, &guest_source)?;

            let el_name = el.as_ref().to_lowercase();
            let el_version = if matches!(el, stateless_validator::ExecutionClient::Zesu) {
                guest_source
                    .version_label()
                    .unwrap_or_else(|| el.version().to_string())
            } else {
                el.version().to_string()
            };
            let el_str = format!("{}-{}", el_name, el_version);
            let zkvms =
                get_el_zkvm_instances(el, &cli.zkvms, resource, zkvm_config.clone(), &guest_source)
                    .await
                    .context("Failed to get EL zkvm instances")?;

            let config = RunConfig {
                sub_folder: Some(el_str),
                ..config_base
            };

            match action {
                Action::Verify => {
                    for instance in &zkvms {
                        run_verify_from_disk(instance, &config, &proofs_folder)?;
                    }
                }
                _ => {
                    let input_folder = input_folder
                        .expect("CLI validation requires an input folder for execute and prove");
                    info!(
                        "Running stateless-validator benchmark for input folder: {}",
                        input_folder.display()
                    );
                    for zkvm in &zkvms {
                        let existing_output_dir =
                            (!config.force_rerun).then(|| benchmark_output_dir(zkvm, &config));
                        let guest_io = stateless_validator::stateless_validator_input_iter(
                            input_folder.as_path(),
                            fixture.as_deref(),
                            el,
                            existing_output_dir.as_deref(),
                        )?
                        .map(|input| input.context("Failed to get stateless validator input"));
                        run_benchmark_iter(zkvm, &config, guest_io)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn validate_guest_compatibility(
    el: stateless_validator::ExecutionClient,
    zkvms: &[zkVMKind],
    guest_source: &GuestProgramSource,
) -> Result<()> {
    if !matches!(el, stateless_validator::ExecutionClient::Zesu)
        || !matches!(guest_source, GuestProgramSource::Default)
    {
        return Ok(());
    }

    let unsupported = zkvms
        .iter()
        .filter(|zkvm| **zkvm != zkVMKind::Zisk)
        .map(|zkvm| zkvm.as_str())
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        bail!(
            "the default Zesu {} artifact is available only for ZisK; unsupported --zkvms: {}",
            el.version(),
            unsupported.join(", ")
        );
    }

    Ok(())
}

const fn build_zkvm_config(
    action: Action,
    timeout_override: Option<Duration>,
) -> DockerizedzkVMConfig {
    let mut config = DockerizedzkVMConfig {
        execute_timeout: Some(DEFAULT_EXECUTE_TIMEOUT),
        prove_timeout: Some(DEFAULT_PROVE_TIMEOUT),
        verify_timeout: Some(DEFAULT_VERIFY_TIMEOUT),
    };

    if let Some(timeout) = timeout_override {
        match action {
            Action::Execute => config.execute_timeout = Some(timeout),
            Action::Prove => config.prove_timeout = Some(timeout),
            Action::Verify => config.verify_timeout = Some(timeout),
        }
    }

    config
}
