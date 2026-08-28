//! Build canonical Amsterdam stateless guest input and expected output bytes from live RPC data.

mod builder;
mod rpc;
mod serde_helpers;

use std::time::Duration;

use alloy_primitives::B256;
use anyhow::{Context, ensure};
pub use builder::GeneratedInput;
use reqwest::Client;

// These dependencies are used by this package's CLI target.
#[cfg(test)]
use benchmark_runner as _;
use clap as _;
use humantime as _;
use sha2 as _;
use tar as _;
use time as _;
use tokio as _;
use toml as _;
use tracing as _;
use tracing_subscriber as _;
use zstd as _;

/// Configuration for consensus-layer and execution-layer RPC access.
#[derive(Debug, Clone)]
pub struct NetworkWitnessConfig {
    /// Consensus-layer Beacon API endpoint.
    pub cl_endpoint: String,
    /// Execution-layer JSON-RPC endpoint.
    pub el_endpoint: String,
    /// Request timeout.
    pub timeout: Duration,
    /// Extra HTTP headers sent to the consensus-layer endpoint.
    pub cl_headers: Vec<(String, String)>,
    /// Extra HTTP headers sent to the execution-layer endpoint.
    pub el_headers: Vec<(String, String)>,
}

impl NetworkWitnessConfig {
    /// Creates a config with a conservative default timeout and no custom headers.
    pub fn new(cl_endpoint: impl Into<String>, el_endpoint: impl Into<String>) -> Self {
        Self {
            cl_endpoint: cl_endpoint.into(),
            el_endpoint: el_endpoint.into(),
            timeout: Duration::from_secs(30),
            cl_headers: Vec::new(),
            el_headers: Vec::new(),
        }
    }
}

/// Block selection for canonical stateless input generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BlockSelector {
    /// The current consensus head block.
    #[default]
    Head,
    /// A Beacon API block id such as `head`, `finalized`, a slot, or a block root.
    BeaconBlockId(String),
    /// An execution block number. The matching CL slot is derived from the EL block timestamp.
    ExecutionBlockNumber(u64),
}

/// Client for fetching network data and building canonical stateless input bytes.
#[derive(Debug, Clone)]
pub struct NetworkWitnessClient {
    rpc: rpc::RpcClient,
}

impl NetworkWitnessClient {
    /// Creates a new network witness client.
    pub fn new(config: NetworkWitnessConfig) -> anyhow::Result<Self> {
        let http = Client::builder()
            .timeout(config.timeout)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            rpc: rpc::RpcClient::new(config, http),
        })
    }

    /// Fetches network data and returns canonical spec guest input and expected output bytes.
    pub async fn stateless_input_bytes(
        &self,
        selector: BlockSelector,
    ) -> anyhow::Result<GeneratedInput> {
        let envelope = self.fetch_payload_envelope(selector).await?;
        let block_hash = envelope.payload.block_hash;

        let witness = self
            .rpc
            .debug_execution_witness_by_block_hash(block_hash)
            .await
            .with_context(|| format!("failed to fetch execution witness for {block_hash}"))?;
        let chain_id = self.rpc.eth_chain_id().await?;

        builder::build_generated_input(envelope, witness, chain_id)
    }

    async fn fetch_payload_envelope(
        &self,
        selector: BlockSelector,
    ) -> anyhow::Result<rpc::ExecutionPayloadEnvelope> {
        match selector {
            BlockSelector::Head => self.rpc.execution_payload_envelope("head").await,
            BlockSelector::BeaconBlockId(block_id) => {
                self.rpc.execution_payload_envelope(&block_id).await
            }
            BlockSelector::ExecutionBlockNumber(number) => {
                self.fetch_payload_envelope_for_execution_number(number)
                    .await
            }
        }
    }

    async fn fetch_payload_envelope_for_execution_number(
        &self,
        number: u64,
    ) -> anyhow::Result<rpc::ExecutionPayloadEnvelope> {
        let el_block = self.rpc.eth_block_by_number(number).await?;
        ensure!(
            el_block.number == number,
            "EL returned block number {} for requested block number {}",
            el_block.number,
            number,
        );

        let genesis = self.rpc.beacon_genesis().await?;
        let spec = self.rpc.beacon_spec().await?;
        ensure!(
            spec.seconds_per_slot > 0,
            "CL spec SECONDS_PER_SLOT must be greater than zero",
        );
        ensure!(
            el_block.timestamp >= genesis.genesis_time,
            "EL block timestamp {} is before CL genesis time {}",
            el_block.timestamp,
            genesis.genesis_time,
        );
        let slot = (el_block.timestamp - genesis.genesis_time) / spec.seconds_per_slot;

        let envelope = self
            .rpc
            .execution_payload_envelope(&slot.to_string())
            .await?;
        ensure_matching_hash(envelope.payload.block_hash, el_block.hash, number, slot)?;
        Ok(envelope)
    }
}

fn ensure_matching_hash(
    cl_hash: B256,
    el_hash: B256,
    number: u64,
    slot: u64,
) -> anyhow::Result<()> {
    ensure!(
        cl_hash == el_hash,
        "CL payload envelope at slot {slot} has execution block hash {cl_hash}, expected EL block #{number} hash {el_hash}",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn live_generation_from_env() -> anyhow::Result<()> {
        let (Ok(cl_endpoint), Ok(el_endpoint)) =
            (std::env::var("CL_RPC_URL"), std::env::var("EL_RPC_URL"))
        else {
            return Ok(());
        };

        let client =
            NetworkWitnessClient::new(NetworkWitnessConfig::new(cl_endpoint, el_endpoint))?;
        let generated = client.stateless_input_bytes(BlockSelector::Head).await?;

        assert!(generated.stateless_input_bytes.starts_with(&[0x15, 0x01]));
        assert!(generated.block_number > 0);
        Ok(())
    }
}
