use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};

use alloy_primitives::{B256, hex};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stateless_validator_common::{
    HashTreeRoot as _, Sha2Hasher, SszDecode as _,
    guest::{StatelessInput, StatelessValidationResult, input::ProtocolFork},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use witness_generator_spec_cli::GeneratedInput;

pub(crate) const ARTIFACT_SCHEMA_VERSION: u64 = 2;
pub(crate) const BATCH_MANIFEST_SCHEMA_VERSION: u64 = 2;
pub(crate) const BATCH_MANIFEST_PATH: &str = ".meta/manifest.json";
const EEST_NETWORK: &str = "Amsterdam";
const ZSTD_LEVEL: i32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct EestFixture {
    tests: BTreeMap<String, EestBlockchainTest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EestBlockchainTest {
    network: String,
    config: EestConfig,
    blocks: Vec<EestBlock>,
    #[serde(rename = "_info")]
    info: EestInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EestConfig {
    chainid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EestBlock {
    stateless_input_bytes: String,
    stateless_output_bytes: String,
    block_header: EestBlockHeader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EestBlockHeader {
    number: String,
    gas_used: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EestInfo {
    metadata: EestMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EestMetadata {
    witness_generator: WitnessGeneratorMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WitnessGeneratorMetadata {
    schema_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<String>,
    block_hash: String,
    slot_number: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    collection_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    collected_at: Option<String>,
    generator_package: String,
    generator_version: String,
    generator_git_commit: String,
    stateless_input_schema_id: String,
    stateless_input_byte_length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatelessInputArtifact {
    fixture: EestFixture,
    pub(crate) schema_version: u64,
    pub(crate) network: String,
    pub(crate) chain_id: u64,
    pub(crate) block_number: u64,
    pub(crate) block_hash: String,
    pub(crate) gas_used: u64,
    pub(crate) slot_number: u64,
    pub(crate) collection_mode: String,
    pub(crate) collected_at: String,
    pub(crate) stateless_input_byte_length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactIndexEntry {
    pub(crate) schema_version: u64,
    pub(crate) network: String,
    pub(crate) chain_id: u64,
    pub(crate) block_number: u64,
    pub(crate) block_hash: String,
    pub(crate) gas_used: u64,
    pub(crate) slot_number: u64,
    pub(crate) collection_mode: String,
    pub(crate) collected_at: String,
    pub(crate) stateless_input_byte_length: usize,
    pub(crate) path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactWriteResult {
    pub(crate) path: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) created: bool,
}

#[derive(Debug, Clone, Default)]
struct FixtureProvenance {
    network: Option<String>,
    collection_mode: Option<String>,
    collected_at: Option<String>,
    generator_git_commit: String,
}

impl EestFixture {
    fn from_generated(
        generated: &GeneratedInput,
        provenance: FixtureProvenance,
    ) -> anyhow::Result<Self> {
        let schema_id = generated
            .stateless_input_bytes
            .get(..2)
            .context("generated stateless input is missing its two-byte schema id")?;
        ensure!(
            !generated.stateless_output_bytes.is_empty(),
            "generated stateless output is empty"
        );

        let block_hash = generated.block_hash.to_string();
        let test_name = fixture_test_name(generated.block_number, &block_hash);
        let metadata = WitnessGeneratorMetadata {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            network: provenance.network,
            block_hash,
            slot_number: generated.slot_number,
            collection_mode: provenance.collection_mode,
            collected_at: provenance.collected_at,
            generator_package: env!("CARGO_PKG_NAME").to_owned(),
            generator_version: env!("CARGO_PKG_VERSION").to_owned(),
            generator_git_commit: provenance.generator_git_commit,
            stateless_input_schema_id: format!("0x{}", hex::encode(schema_id)),
            stateless_input_byte_length: generated.stateless_input_bytes.len(),
        };
        let test = EestBlockchainTest {
            network: EEST_NETWORK.to_owned(),
            config: EestConfig {
                chainid: hex_quantity(generated.chain_id),
            },
            blocks: vec![EestBlock {
                stateless_input_bytes: hex_bytes(&generated.stateless_input_bytes),
                stateless_output_bytes: hex_bytes(&generated.stateless_output_bytes),
                block_header: EestBlockHeader {
                    number: hex_quantity(generated.block_number),
                    gas_used: hex_quantity(generated.gas_used),
                },
            }],
            info: EestInfo {
                metadata: EestMetadata {
                    witness_generator: metadata,
                },
            },
        };

        Ok(Self {
            tests: BTreeMap::from([(test_name, test)]),
        })
    }

    pub(crate) fn to_pretty_json(&self) -> anyhow::Result<Vec<u8>> {
        let mut bytes =
            serde_json::to_vec_pretty(self).context("failed to serialize EEST fixture JSON")?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

impl StatelessInputArtifact {
    pub(crate) fn from_generated(
        network: &str,
        collection_mode: &str,
        generated: &GeneratedInput,
    ) -> anyhow::Result<Self> {
        Self::from_generated_at(
            network,
            collection_mode,
            generated,
            &utc_now_rfc3339()?,
            generator_git_commit(),
        )
    }

    pub(crate) fn from_generated_at(
        network: &str,
        collection_mode: &str,
        generated: &GeneratedInput,
        collected_at: &str,
        generator_git_commit: String,
    ) -> anyhow::Result<Self> {
        let fixture = EestFixture::from_generated(
            generated,
            FixtureProvenance {
                network: Some(network.to_owned()),
                collection_mode: Some(collection_mode.to_owned()),
                collected_at: Some(collected_at.to_owned()),
                generator_git_commit,
            },
        )?;
        Self::from_fixture(fixture)
    }

    fn from_fixture(fixture: EestFixture) -> anyhow::Result<Self> {
        ensure!(
            fixture.tests.len() == 1,
            "schema-v2 collected fixture must contain exactly one EEST test"
        );
        let (test_name, test) = fixture.tests.first_key_value().unwrap();
        ensure!(
            test.network == EEST_NETWORK,
            "schema-v2 collected fixture network must be {EEST_NETWORK}"
        );
        ensure!(
            test.blocks.len() == 1,
            "schema-v2 collected fixture must contain exactly one block"
        );
        let block = &test.blocks[0];
        let metadata = &test.info.metadata.witness_generator;
        ensure!(
            metadata.schema_version == ARTIFACT_SCHEMA_VERSION,
            "unsupported collected artifact schema version {}; expected {}",
            metadata.schema_version,
            ARTIFACT_SCHEMA_VERSION
        );

        let network = required_metadata("network", metadata.network.as_deref())?.to_owned();
        let collection_mode =
            required_metadata("collectionMode", metadata.collection_mode.as_deref())?.to_owned();
        let collected_at =
            required_metadata("collectedAt", metadata.collected_at.as_deref())?.to_owned();
        let chain_id = parse_hex_quantity("config.chainid", &test.config.chainid)?;
        let block_number = parse_hex_quantity("blockHeader.number", &block.block_header.number)?;
        let gas_used = parse_hex_quantity("blockHeader.gasUsed", &block.block_header.gas_used)?;
        let block_hash = metadata
            .block_hash
            .parse::<B256>()
            .context("witness_generator.blockHash is not a valid B256")?;
        let expected_test_name = fixture_test_name(block_number, &metadata.block_hash);
        ensure!(
            test_name == &expected_test_name,
            "schema-v2 EEST test name does not match its block number and hash"
        );

        let input_bytes = decode_hex_bytes("statelessInputBytes", &block.stateless_input_bytes)?;
        let output_bytes = decode_hex_bytes("statelessOutputBytes", &block.stateless_output_bytes)?;
        ensure!(!input_bytes.is_empty(), "statelessInputBytes is empty");
        ensure!(!output_bytes.is_empty(), "statelessOutputBytes is empty");
        let schema_id = input_bytes
            .get(..2)
            .context("statelessInputBytes is missing its two-byte schema id")?;
        ensure!(
            metadata.stateless_input_schema_id == format!("0x{}", hex::encode(schema_id)),
            "stateless input schema id metadata does not match statelessInputBytes"
        );
        ensure!(
            metadata.stateless_input_byte_length == input_bytes.len(),
            "stateless input byte length metadata does not match statelessInputBytes"
        );
        let (fork, input) = StatelessInput::from_schema_prefixed_ssz(&input_bytes)
            .context("failed to decode statelessInputBytes")?;
        ensure!(
            fork == ProtocolFork::Amsterdam,
            "schema-v2 collected fixture must contain Amsterdam stateless input bytes"
        );
        let output = StatelessValidationResult::from_ssz_bytes(&output_bytes)
            .map_err(|err| anyhow::anyhow!("failed to decode statelessOutputBytes: {err:?}"))?;
        ensure!(
            output.successful_validation,
            "schema-v2 statelessOutputBytes must expect successful validation"
        );
        ensure!(
            output.chain_config == input.chain_config,
            "stateless output chain configuration does not match stateless input"
        );
        ensure!(
            output.chain_config.chain_id == chain_id,
            "config.chainid does not match the stateless input chain configuration"
        );
        ensure!(
            output.new_payload_request_root
                == input.new_payload_request.hash_tree_root(&Sha2Hasher),
            "stateless output request root does not match stateless input"
        );
        ensure!(
            input.new_payload_request.block_number() == block_number,
            "blockHeader.number does not match the stateless input payload"
        );
        ensure!(
            input.new_payload_request.gas_used() == gas_used,
            "blockHeader.gasUsed does not match the stateless input payload"
        );
        ensure!(
            input.new_payload_request.block_hash() == block_hash.0,
            "witness_generator.blockHash does not match the stateless input payload"
        );

        let schema_version = metadata.schema_version;
        let block_hash = metadata.block_hash.clone();
        let slot_number = metadata.slot_number;
        let stateless_input_byte_length = metadata.stateless_input_byte_length;

        Ok(Self {
            schema_version,
            network,
            chain_id,
            block_number,
            block_hash,
            gas_used,
            slot_number,
            collection_mode,
            collected_at,
            stateless_input_byte_length,
            fixture,
        })
    }

    pub(crate) fn fixture_json(&self) -> anyhow::Result<Vec<u8>> {
        self.fixture.to_pretty_json()
    }

    pub(crate) fn index_entry(&self, path: &Path) -> ArtifactIndexEntry {
        ArtifactIndexEntry {
            schema_version: self.schema_version,
            network: self.network.clone(),
            chain_id: self.chain_id,
            block_number: self.block_number,
            block_hash: self.block_hash.clone(),
            gas_used: self.gas_used,
            slot_number: self.slot_number,
            collection_mode: self.collection_mode.clone(),
            collected_at: self.collected_at.clone(),
            stateless_input_byte_length: self.stateless_input_byte_length,
            path: path_to_slash_string(path),
        }
    }
}

pub(crate) fn one_shot_fixture_json(generated: &GeneratedInput) -> anyhow::Result<Vec<u8>> {
    EestFixture::from_generated(
        generated,
        FixtureProvenance {
            generator_git_commit: generator_git_commit(),
            ..Default::default()
        },
    )?
    .to_pretty_json()
}

pub(crate) fn write_artifact_atomic(
    blocks_root: &Path,
    artifact: &StatelessInputArtifact,
) -> anyhow::Result<ArtifactWriteResult> {
    let relative_path = relative_artifact_path(artifact);
    let path = blocks_root.join(&relative_path);
    if path.exists() {
        read_artifact_with_json(&path)?;
        return Ok(ArtifactWriteResult {
            path,
            relative_path,
            created: false,
        });
    }

    let json = artifact.fixture_json()?;
    let compressed =
        zstd::bulk::compress(&json, ZSTD_LEVEL).context("failed to compress artifact")?;
    write_bytes_atomic(&path, &compressed)?;

    Ok(ArtifactWriteResult {
        path,
        relative_path,
        created: true,
    })
}

#[cfg(test)]
pub(crate) fn read_artifact(path: &Path) -> anyhow::Result<StatelessInputArtifact> {
    Ok(read_artifact_with_json(path)?.0)
}

pub(crate) fn read_artifact_with_json(
    path: &Path,
) -> anyhow::Result<(StatelessInputArtifact, Vec<u8>)> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open artifact {}", path.display()))?;
    let json = zstd::stream::decode_all(file)
        .with_context(|| format!("failed to decompress artifact {}", path.display()))?;
    let fixture: EestFixture = serde_json::from_slice(&json).with_context(|| {
        format!(
            "failed to decode schema-v2 EEST artifact JSON {}",
            path.display()
        )
    })?;
    let artifact = StatelessInputArtifact::from_fixture(fixture)
        .with_context(|| format!("invalid schema-v2 EEST artifact {}", path.display()))?;
    Ok((artifact, json))
}

pub(crate) fn append_index_entry(
    index_path: &Path,
    entry: &ArtifactIndexEntry,
) -> anyhow::Result<()> {
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create index directory {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(index_path)
        .with_context(|| format!("failed to open index {}", index_path.display()))?;
    serde_json::to_writer(&mut file, entry).context("failed to serialize index entry")?;
    file.write_all(b"\n")
        .context("failed to write index entry")?;
    Ok(())
}

pub(crate) fn write_json_atomic<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value).context("failed to serialize JSON")?;
    write_bytes_atomic(path, &bytes)
}

pub(crate) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    let part_path = part_path(path);
    fs::write(&part_path, bytes)
        .with_context(|| format!("failed to write partial file {}", part_path.display()))?;
    fs::rename(&part_path, path).with_context(|| {
        format!(
            "failed to atomically rename {} to {}",
            part_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

pub(crate) fn relative_artifact_path(artifact: &StatelessInputArtifact) -> PathBuf {
    relative_artifact_path_from_parts(artifact.block_number, &artifact.block_hash)
}

pub(crate) fn relative_artifact_path_from_parts(block_number: u64, block_hash: &str) -> PathBuf {
    let chunk = block_number / 1_000;
    PathBuf::from(format!("{chunk:06}")).join(format!(
        "{}-{}.json.zst",
        block_number,
        filename_hash(block_hash)
    ))
}

pub(crate) fn fixture_archive_path(artifact: &StatelessInputArtifact) -> PathBuf {
    let chunk = artifact.block_number / 1_000;
    PathBuf::from("blockchain_tests")
        .join(format!("{chunk:06}"))
        .join(format!(
            "{}-{}.json",
            artifact.block_number,
            filename_hash(&artifact.block_hash)
        ))
}

pub(crate) fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(Sha256::digest(bytes)))
}

pub(crate) fn file_sha256_hex(path: &Path) -> anyhow::Result<String> {
    let file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("0x{}", hex::encode(hasher.finalize())))
}

pub(crate) fn utc_now_rfc3339() -> anyhow::Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format current timestamp")
}

fn decode_hex_bytes(label: &str, value: &str) -> anyhow::Result<Vec<u8>> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    ensure!(
        value.len().is_multiple_of(2),
        "{label} must have an even hex length"
    );
    hex::decode(value).with_context(|| format!("{label} is not valid hex"))
}

fn parse_hex_quantity(label: &str, value: &str) -> anyhow::Result<u64> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .context("schema-v2 EEST quantities must be 0x-prefixed")?;
    u64::from_str_radix(value, 16).with_context(|| format!("{label} is not a valid hex quantity"))
}

fn required_metadata<'a>(label: &str, value: Option<&'a str>) -> anyhow::Result<&'a str> {
    value
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("schema-v2 collected fixture is missing {label} metadata"))
}

fn fixture_test_name(block_number: u64, block_hash: &str) -> String {
    format!(
        "witness-generator-spec-cli::block_{block_number}_{}",
        filename_hash(block_hash)
    )
}

fn hex_bytes(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn hex_quantity(value: u64) -> String {
    format!("0x{value:x}")
}

fn part_path(path: &Path) -> PathBuf {
    path.with_extension(
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension) => format!("{extension}.part"),
            None => "part".to_owned(),
        },
    )
}

fn filename_hash(hash: &str) -> &str {
    hash.strip_prefix("0x").unwrap_or(hash)
}

fn generator_git_commit() -> String {
    std::env::var("WITNESS_GENERATOR_SPEC_GIT_COMMIT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("GIT_COMMIT").map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
pub(crate) fn test_generated_input(block_number: u64, block_hash: B256) -> GeneratedInput {
    use stateless_validator_common::{
        SszEncode as _,
        guest::input::{
            ChainConfig, ExecutionWitness, ForkActivation, ForkConfig, StatelessInput,
            new_payload_request::{
                ExecutionPayloadV4, ExecutionRequestsGloas, NewPayloadRequest,
                NewPayloadRequestGloas,
            },
        },
    };

    let chain_config = ChainConfig {
        chain_id: 1,
        active_fork: ForkConfig::new(ForkActivation::new(None, Some(0))),
    };
    let new_payload_request = NewPayloadRequest::Gloas(NewPayloadRequestGloas {
        execution_payload: ExecutionPayloadV4 {
            parent_hash: [0; 32],
            fee_recipient: [0; 20],
            state_root: [0; 32],
            receipts_root: [0; 32],
            logs_bloom: [0; 256],
            prev_randao: [0; 32],
            block_number,
            gas_limit: 30_000_000,
            gas_used: 21_000,
            timestamp: 1,
            extra_data: Default::default(),
            base_fee_per_gas: [0; 32],
            block_hash: block_hash.0,
            transactions: Default::default(),
            withdrawals: Default::default(),
            blob_gas_used: 0,
            excess_blob_gas: 0,
            block_access_list: Default::default(),
            slot_number: 64,
        },
        versioned_hashes: Default::default(),
        parent_beacon_block_root: [0; 32],
        execution_requests: ExecutionRequestsGloas::default(),
    });
    let input = StatelessInput {
        new_payload_request: new_payload_request.clone(),
        witness: ExecutionWitness::default(),
        chain_config: chain_config.clone(),
        public_keys: Default::default(),
    };
    let output = StatelessValidationResult::new(
        new_payload_request.hash_tree_root(&Sha2Hasher),
        true,
        chain_config,
    );
    GeneratedInput {
        stateless_input_bytes: input.to_schema_prefixed_ssz(ProtocolFork::Amsterdam),
        stateless_output_bytes: output.to_ssz(),
        block_hash,
        block_number,
        slot_number: 64,
        chain_id: 1,
        gas_used: 21_000,
    }
}

#[cfg(test)]
mod tests {
    use benchmark_runner::stateless_validator::{ExecutionClient, stateless_validator_input_iter};

    use super::*;

    #[test]
    fn artifact_serializes_as_eest_and_roundtrips_through_zstd() {
        let dir = temp_dir("artifact_roundtrip");
        let generated = test_generated_input(42, B256::repeat_byte(0xaa));
        let artifact = StatelessInputArtifact::from_generated_at(
            "glamsterdam-devnet-5",
            "head",
            &generated,
            "2026-06-11T00:00:00Z",
            "test-commit".to_owned(),
        )
        .unwrap();

        let result = write_artifact_atomic(&dir, &artifact).unwrap();
        let decoded = read_artifact(&result.path).unwrap();

        assert!(result.created);
        assert_eq!(decoded, artifact);
        assert_eq!(decoded.schema_version, 2);
        assert_eq!(
            decoded.stateless_input_byte_length,
            generated.stateless_input_bytes.len()
        );
        assert_eq!(decoded.collection_mode, "head");

        let file = fs::File::open(&result.path).unwrap();
        let json = zstd::stream::decode_all(file).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        let test = value.as_object().unwrap().values().next().unwrap();
        assert_eq!(test["network"], "Amsterdam");
        assert_eq!(
            test["blocks"][0]["statelessInputBytes"],
            hex_bytes(&generated.stateless_input_bytes)
        );
        assert_eq!(
            test["blocks"][0]["statelessOutputBytes"],
            hex_bytes(&generated.stateless_output_bytes)
        );
        assert_eq!(
            test["_info"]["metadata"]["witness_generator"]["schemaVersion"],
            2
        );
        assert!(
            test["_info"]["metadata"]["witness_generator"]
                .get("statelessOutputByteLength")
                .is_none()
        );
        assert!(
            test["_info"]["metadata"]["witness_generator"]
                .get("statelessOutputSha256")
                .is_none()
        );
        assert!(
            test["_info"]["metadata"]["witness_generator"]
                .get("statelessInputSha256")
                .is_none()
        );
    }

    #[test]
    fn artifact_paths_include_block_number_and_hash() {
        let first = artifact_for(2_381, B256::repeat_byte(0xaa));
        let second = artifact_for(2_381, B256::repeat_byte(0xbb));

        assert_ne!(
            relative_artifact_path(&first),
            relative_artifact_path(&second)
        );
        assert_eq!(
            fixture_archive_path(&first),
            PathBuf::from("blockchain_tests/000002").join(format!("2381-{}.json", "aa".repeat(32)))
        );
    }

    #[test]
    fn one_shot_fixture_uses_eest_shape_without_collection_metadata() {
        let generated = test_generated_input(42, B256::repeat_byte(0xaa));
        let json = one_shot_fixture_json(&generated).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        let test = value.as_object().unwrap().values().next().unwrap();
        let metadata = &test["_info"]["metadata"]["witness_generator"];

        assert_eq!(test["config"]["chainid"], "0x1");
        assert_eq!(test["blocks"][0]["blockHeader"]["number"], "0x2a");
        assert_eq!(test["blocks"][0]["blockHeader"]["gasUsed"], "0x5208");
        assert!(metadata.get("network").is_none());
        assert!(metadata.get("collectionMode").is_none());
        assert!(metadata.get("collectedAt").is_none());
    }

    #[test]
    fn one_shot_fixture_loads_through_benchmark_runner() {
        let dir = temp_dir("one_shot_benchmark_loader");
        let fixture_dir = dir.join("blockchain_tests");
        fs::create_dir_all(&fixture_dir).unwrap();
        let generated = test_generated_input(42, B256::repeat_byte(0xaa));
        fs::write(
            fixture_dir.join("one-shot.json"),
            one_shot_fixture_json(&generated).unwrap(),
        )
        .unwrap();

        let fixtures = stateless_validator_input_iter(&dir, None, ExecutionClient::Reth, None)
            .unwrap()
            .collect::<anyhow::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].input().unwrap().stdin(),
            generated.stateless_input_bytes
        );
        assert_eq!(
            fixtures[0].expected_public_values().unwrap(),
            generated.stateless_output_bytes
        );
        assert_eq!(fixtures[0].metadata()["chain_id"], generated.chain_id);
        assert_eq!(
            fixtures[0].metadata()["block_number"],
            generated.block_number
        );
        assert_eq!(fixtures[0].metadata()["block_used_gas"], generated.gas_used);
    }

    #[test]
    fn rejects_legacy_artifact_shape() {
        let dir = temp_dir("legacy_rejection");
        let path = dir.join("legacy.json.zst");
        let legacy = serde_json::json!({
            "schemaVersion": 1,
            "statelessInputBytes": "0x1501"
        });
        let compressed = zstd::bulk::compress(&serde_json::to_vec(&legacy).unwrap(), 3).unwrap();
        fs::write(&path, compressed).unwrap();

        let error = read_artifact(&path).unwrap_err();

        assert!(error.to_string().contains("schema-v2 EEST artifact"));
    }

    #[test]
    fn rejects_schema_v2_fixture_without_output_bytes() {
        let dir = temp_dir("missing_output");
        let path = dir.join("missing-output.json.zst");
        let artifact = artifact_for(42, B256::repeat_byte(0xaa));
        let mut value = serde_json::to_value(&artifact.fixture).unwrap();
        let test = value.as_object_mut().unwrap().values_mut().next().unwrap();
        test["blocks"][0]
            .as_object_mut()
            .unwrap()
            .remove("statelessOutputBytes");
        write_compressed_json(&path, &value);

        let error = read_artifact(&path).unwrap_err();

        assert!(error.to_string().contains("schema-v2 EEST artifact"));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let dir = temp_dir("unsupported_schema");
        let path = dir.join("unsupported.json.zst");
        let artifact = artifact_for(42, B256::repeat_byte(0xaa));
        let mut value = serde_json::to_value(&artifact.fixture).unwrap();
        let test = value.as_object_mut().unwrap().values_mut().next().unwrap();
        test["_info"]["metadata"]["witness_generator"]["schemaVersion"] = serde_json::json!(1);
        write_compressed_json(&path, &value);

        let error = read_artifact(&path).unwrap_err();

        assert!(format!("{error:#}").contains("unsupported collected artifact"));
    }

    fn artifact_for(block_number: u64, block_hash: B256) -> StatelessInputArtifact {
        StatelessInputArtifact::from_generated_at(
            "glamsterdam-devnet-5",
            "head",
            &test_generated_input(block_number, block_hash),
            "2026-06-11T00:00:00Z",
            "test-commit".to_owned(),
        )
        .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "witness-generator-spec-cli-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_compressed_json(path: &Path, value: &serde_json::Value) {
        let compressed = zstd::bulk::compress(&serde_json::to_vec(value).unwrap(), 3).unwrap();
        fs::write(path, compressed).unwrap();
    }
}
