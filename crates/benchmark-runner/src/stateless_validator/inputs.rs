use crate::{
    guest_programs::{GenericGuestFixture, GuestFixture},
    stateless_validator::{eest::EestStatelessFixture, ExecutionClient},
};
use anyhow::{bail, Context, Result};
use ere_dockerized::Input;
use serde::Serialize;
use stateless_validator_common::guest::input::{ProtocolFork, StatelessInput};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
struct EestBlockMetadata {
    fixture_format: &'static str,
    original_test_name: String,
    source_path: String,
    block_index: usize,
    network: String,
    chain_id: u64,
    block_number: Option<u64>,
    block_used_gas: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    opcode_count: Option<BTreeMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_opcode: Option<String>,
}

pub(crate) fn stateless_validator_input_from_fixture(
    fixture: EestStatelessFixture,
    el: ExecutionClient,
) -> Result<Box<dyn GuestFixture>> {
    match el {
        ExecutionClient::Reth | ExecutionClient::Ethrex => raw_eest_input_from_fixture(fixture),
        ExecutionClient::Zesu => zesu_input_from_fixture(fixture),
    }
}

fn zesu_input_from_fixture(fixture: EestStatelessFixture) -> Result<Box<dyn GuestFixture>> {
    let (fork, _input) = StatelessInput::from_schema_prefixed_ssz(&fixture.stateless_input_bytes)
        .with_context(|| {
        format!(
            "failed to decode canonical stateless input for Zesu fixture {}",
            fixture.name
        )
    })?;
    validate_zesu_fork(fork, &fixture.name)?;

    raw_eest_input_from_fixture(fixture)
}

fn validate_zesu_fork(fork: ProtocolFork, fixture_name: &str) -> Result<()> {
    if fork != ProtocolFork::Amsterdam {
        bail!(
            "Zesu {} supports only Glamsterdam inputs (ProtocolFork::Amsterdam), but fixture {} targets {fork:?}",
            ExecutionClient::Zesu.version(),
            fixture_name
        );
    }

    Ok(())
}

fn raw_eest_input_from_fixture(fixture: EestStatelessFixture) -> Result<Box<dyn GuestFixture>> {
    let metadata = EestBlockMetadata {
        fixture_format: "eest",
        original_test_name: fixture.original_test_name,
        source_path: fixture.source_path,
        block_index: fixture.block_index,
        network: fixture.network,
        chain_id: fixture.chain_id,
        block_number: fixture.block_number,
        block_used_gas: fixture.block_used_gas,
        opcode_count: fixture.opcode_count,
        target_opcode: fixture.target_opcode,
    };
    let fixture = GenericGuestFixture::<EestBlockMetadata> {
        name: fixture.name,
        input: Input::new().with_stdin(fixture.stateless_input_bytes),
        expected_public_values: fixture.stateless_output_bytes,
        metadata,
    };

    Ok(fixture.into_boxed())
}
