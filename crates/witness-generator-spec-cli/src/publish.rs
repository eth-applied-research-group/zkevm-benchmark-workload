use std::{path::PathBuf, process::Command};

use anyhow::{Context, bail};
use tracing::info;

use crate::{
    catalog,
    config::{CollectorConfig, R2PublishConfig},
};

const STALE_PUBLIC_OBJECTS: &[&str] = &["blocks.jsonl", "index.jsonl"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AwsCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

pub(crate) fn publish_r2(config: &CollectorConfig) -> anyhow::Result<()> {
    let commands = build_publish_commands(config)?;
    for command in commands {
        info!(program = command.program, args = ?command.args, "running R2 publish command");
        let status = Command::new(&command.program)
            .args(&command.args)
            .status()
            .with_context(|| format!("failed to run {}", command.program))?;
        if !status.success() {
            bail!("{} failed with status {status}", command.program);
        }
    }
    Ok(())
}

pub(crate) fn build_publish_commands(config: &CollectorConfig) -> anyhow::Result<Vec<AwsCommand>> {
    let r2 = config
        .r2
        .as_ref()
        .context("r2 config is required for publish-r2")?;
    if !config.batches_root().is_dir() {
        bail!(
            "batch export directory {} does not exist; run export first",
            config.batches_root().display()
        );
    }
    let catalog_files = catalog::required_catalog_files(config);
    for (_, path) in &catalog_files {
        if !path.is_file() {
            bail!(
                "public catalog file {} does not exist; run export first",
                path.display()
            );
        }
    }

    let endpoint_url = r2.endpoint_url();
    let mut commands = vec![aws_s3_sync_command(
        config.batches_root(),
        r2_uri(r2, &config.network, "exports/batches"),
        &endpoint_url,
    )];

    for (name, path) in catalog_files {
        commands.push(aws_s3_cp_command(
            path,
            r2_uri(r2, &config.network, name),
            &endpoint_url,
        ));
    }

    for stale_object in STALE_PUBLIC_OBJECTS {
        commands.push(aws_s3_rm_command(
            r2_uri(r2, &config.network, stale_object),
            &endpoint_url,
        ));
    }

    Ok(commands)
}

fn aws_s3_sync_command(source: PathBuf, destination: String, endpoint_url: &str) -> AwsCommand {
    AwsCommand {
        program: "aws".to_owned(),
        args: vec![
            "s3".to_owned(),
            "sync".to_owned(),
            source.display().to_string(),
            destination,
            "--exclude".to_owned(),
            "*.part".to_owned(),
            "--endpoint-url".to_owned(),
            endpoint_url.to_owned(),
        ],
    }
}

fn aws_s3_cp_command(source: PathBuf, destination: String, endpoint_url: &str) -> AwsCommand {
    AwsCommand {
        program: "aws".to_owned(),
        args: vec![
            "s3".to_owned(),
            "cp".to_owned(),
            source.display().to_string(),
            destination,
            "--endpoint-url".to_owned(),
            endpoint_url.to_owned(),
        ],
    }
}

fn aws_s3_rm_command(destination: String, endpoint_url: &str) -> AwsCommand {
    AwsCommand {
        program: "aws".to_owned(),
        args: vec![
            "s3".to_owned(),
            "rm".to_owned(),
            destination,
            "--endpoint-url".to_owned(),
            endpoint_url.to_owned(),
        ],
    }
}

fn r2_uri(r2: &R2PublishConfig, network: &str, suffix: &str) -> String {
    let suffix = suffix.trim_matches('/');
    if r2.prefix.is_empty() {
        format!("s3://{}/{network}/{suffix}", r2.bucket)
    } else {
        format!("s3://{}/{}/{network}/{suffix}", r2.bucket, r2.prefix)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use crate::config::R2PublishConfig;

    use super::*;

    #[test]
    fn builds_r2_publish_commands_without_invoking_aws() {
        let config = test_config("commands");
        fs::create_dir_all(config.batches_root()).unwrap();
        fs::write(config.index_path(), "{}\n").unwrap();
        write_catalog_files(&config);

        let commands = build_publish_commands(&config).unwrap();

        assert_eq!(commands.len(), 7);
        assert_eq!(commands[0].program, "aws");
        assert_eq!(
            commands[0].args,
            vec![
                "s3",
                "sync",
                config.batches_root().to_str().unwrap(),
                "s3://stateless-inputs/devnets/glamsterdam-devnet-8/exports/batches",
                "--exclude",
                "*.part",
                "--endpoint-url",
                "https://abc123.r2.cloudflarestorage.com",
            ]
        );
        assert_eq!(
            commands[1].args,
            vec![
                "s3",
                "cp",
                config.network_root().join("index.html").to_str().unwrap(),
                "s3://stateless-inputs/devnets/glamsterdam-devnet-8/index.html",
                "--endpoint-url",
                "https://abc123.r2.cloudflarestorage.com",
            ]
        );
        assert_eq!(
            commands[2].args,
            vec![
                "s3",
                "cp",
                config
                    .network_root()
                    .join("manifest.json")
                    .to_str()
                    .unwrap(),
                "s3://stateless-inputs/devnets/glamsterdam-devnet-8/manifest.json",
                "--endpoint-url",
                "https://abc123.r2.cloudflarestorage.com",
            ]
        );
        assert_eq!(
            commands[3].args,
            vec![
                "s3",
                "cp",
                config
                    .network_root()
                    .join("batches.jsonl")
                    .to_str()
                    .unwrap(),
                "s3://stateless-inputs/devnets/glamsterdam-devnet-8/batches.jsonl",
                "--endpoint-url",
                "https://abc123.r2.cloudflarestorage.com",
            ]
        );
        assert_eq!(
            commands[4].args,
            vec![
                "s3",
                "cp",
                config.network_root().join("SHA256SUMS").to_str().unwrap(),
                "s3://stateless-inputs/devnets/glamsterdam-devnet-8/SHA256SUMS",
                "--endpoint-url",
                "https://abc123.r2.cloudflarestorage.com",
            ]
        );
        assert_eq!(
            commands[5].args,
            vec![
                "s3",
                "rm",
                "s3://stateless-inputs/devnets/glamsterdam-devnet-8/blocks.jsonl",
                "--endpoint-url",
                "https://abc123.r2.cloudflarestorage.com",
            ]
        );
        assert_eq!(
            commands[6].args,
            vec![
                "s3",
                "rm",
                "s3://stateless-inputs/devnets/glamsterdam-devnet-8/index.jsonl",
                "--endpoint-url",
                "https://abc123.r2.cloudflarestorage.com",
            ]
        );
    }

    #[test]
    fn publish_requires_generated_catalog_files() {
        let config = test_config("missing_catalog");
        fs::create_dir_all(config.batches_root()).unwrap();

        let error = build_publish_commands(&config).unwrap_err();

        assert!(
            error.to_string().contains("public catalog file"),
            "{error:?}"
        );
        assert!(error.to_string().contains("run export first"));
    }

    fn write_catalog_files(config: &CollectorConfig) {
        for name in catalog::REQUIRED_CATALOG_FILES {
            fs::write(config.network_root().join(name), b"catalog").unwrap();
        }
    }

    fn test_config(name: &str) -> CollectorConfig {
        let out_root = std::env::temp_dir().join(format!(
            "witness-generator-spec-cli-publish-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&out_root);
        CollectorConfig {
            network: "glamsterdam-devnet-8".to_owned(),
            cl_url: "http://cl".to_owned(),
            el_url: "http://el".to_owned(),
            out_root,
            poll_interval: Duration::from_secs(4),
            request_timeout: Duration::from_secs(30),
            batch_size: 500,
            r2: Some(R2PublishConfig {
                bucket: "stateless-inputs".to_owned(),
                prefix: "devnets".to_owned(),
                account_id: "abc123".to_owned(),
            }),
        }
    }
}
