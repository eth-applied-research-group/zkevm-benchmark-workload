use std::{fs, path::PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::time;
use tracing::{info, warn};
use witness_generator_spec_cli::{
    BlockSelector, GeneratedInput, NetworkWitnessClient, NetworkWitnessConfig,
};

use crate::{
    artifact::{
        self, ArtifactWriteResult, StatelessInputArtifact, append_index_entry, write_json_atomic,
    },
    config::CollectorConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistedArtifact {
    pub(crate) artifact: StatelessInputArtifact,
    pub(crate) write: ArtifactWriteResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectorState {
    last_head_hash: String,
    last_block_number: u64,
    last_slot_number: u64,
    updated_at: String,
}

pub(crate) async fn collect(config: CollectorConfig, once: bool) -> anyhow::Result<()> {
    let mut network_config =
        NetworkWitnessConfig::new(config.cl_url.clone(), config.el_url.clone());
    network_config.timeout = config.request_timeout;
    let client = NetworkWitnessClient::new(network_config)?;
    let mut last_head_hash = read_state(&config.state_path())?.map(|state| state.last_head_hash);

    if once {
        collect_head_once(&client, &config, &mut last_head_hash).await?;
        return Ok(());
    }

    loop {
        match collect_head_once(&client, &config, &mut last_head_hash).await {
            Ok(Some(persisted)) => {
                info!(
                    block_number = persisted.artifact.block_number,
                    block_hash = persisted.artifact.block_hash,
                    path = %persisted.write.path.display(),
                    "collected stateless EEST fixture",
                );
            }
            Ok(None) => {}
            Err(error) => {
                warn!(?error, "failed to collect stateless EEST fixture");
            }
        }
        time::sleep(config.poll_interval).await;
    }
}

async fn collect_head_once(
    client: &NetworkWitnessClient,
    config: &CollectorConfig,
    last_head_hash: &mut Option<String>,
) -> anyhow::Result<Option<PersistedArtifact>> {
    let generated = client
        .stateless_input_bytes(BlockSelector::Head)
        .await
        .context("failed to generate stateless input bytes for head")?;
    let persisted = collect_generated(config, generated, last_head_hash.as_deref())?;
    if let Some(persisted) = &persisted {
        *last_head_hash = Some(persisted.artifact.block_hash.clone());
    }
    Ok(persisted)
}

pub(crate) fn collect_generated(
    config: &CollectorConfig,
    generated: GeneratedInput,
    last_head_hash: Option<&str>,
) -> anyhow::Result<Option<PersistedArtifact>> {
    let block_hash = generated.block_hash.to_string();
    if last_head_hash == Some(block_hash.as_str()) {
        return Ok(None);
    }

    let artifact = StatelessInputArtifact::from_generated(&config.network, "head", &generated)?;
    let write = artifact::write_artifact_atomic(&config.blocks_root(), &artifact)?;
    if write.created {
        let index_entry = artifact.index_entry(&PathBuf::from("blocks").join(&write.relative_path));
        append_index_entry(&config.index_path(), &index_entry)?;
    }
    write_state(config, &artifact)?;

    Ok(Some(PersistedArtifact { artifact, write }))
}

fn read_state(path: &std::path::Path) -> anyhow::Result<Option<CollectorState>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        fs::read(path).with_context(|| format!("failed to read state {}", path.display()))?;
    serde_json::from_slice(&contents)
        .with_context(|| format!("failed to decode state {}", path.display()))
}

fn write_state(config: &CollectorConfig, artifact: &StatelessInputArtifact) -> anyhow::Result<()> {
    let state = CollectorState {
        last_head_hash: artifact.block_hash.clone(),
        last_block_number: artifact.block_number,
        last_slot_number: artifact.slot_number,
        updated_at: artifact::utc_now_rfc3339()?,
    };
    write_json_atomic(&config.state_path(), &state)
}

#[cfg(test)]
mod tests {
    use alloy_primitives::B256;

    use crate::artifact::test_generated_input;

    use super::*;

    #[test]
    fn collect_generated_dedupes_unchanged_head() {
        let config = test_config("dedupe");
        let generated = generated_input(42, B256::repeat_byte(0xaa));

        let persisted = collect_generated(&config, generated.clone(), None)
            .unwrap()
            .unwrap();
        let skipped =
            collect_generated(&config, generated, Some(&persisted.artifact.block_hash)).unwrap();

        assert!(skipped.is_none());
    }

    #[test]
    fn collect_generated_preserves_reorg_variants() {
        let config = test_config("reorg");
        let first = collect_generated(&config, generated_input(42, B256::repeat_byte(0xaa)), None)
            .unwrap()
            .unwrap();
        let second = collect_generated(
            &config,
            generated_input(42, B256::repeat_byte(0xbb)),
            Some(&first.artifact.block_hash),
        )
        .unwrap()
        .unwrap();

        assert_ne!(first.write.path, second.write.path);
        assert!(first.write.path.exists());
        assert!(second.write.path.exists());
    }

    fn test_config(name: &str) -> CollectorConfig {
        let out_root = std::env::temp_dir().join(format!(
            "witness-generator-spec-cli-collector-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&out_root);
        CollectorConfig {
            network: "glamsterdam-devnet-8".to_owned(),
            cl_url: "http://cl".to_owned(),
            el_url: "http://el".to_owned(),
            out_root,
            poll_interval: std::time::Duration::from_secs(4),
            request_timeout: std::time::Duration::from_secs(30),
            batch_size: 500,
            r2: None,
        }
    }

    fn generated_input(block_number: u64, block_hash: B256) -> GeneratedInput {
        test_generated_input(block_number, block_hash)
    }
}
