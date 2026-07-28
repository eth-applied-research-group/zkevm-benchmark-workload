use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};
use tracing::info;

const EEST_SAFE_FILE_STEM_MAX_LEN: usize = 220;

#[derive(Debug, Clone)]
pub(crate) struct EestStatelessFixture {
    pub(crate) name: String,
    pub(crate) original_test_name: String,
    pub(crate) source_path: String,
    pub(crate) block_index: usize,
    pub(crate) network: String,
    pub(crate) chain_id: u64,
    pub(crate) block_number: Option<u64>,
    pub(crate) block_used_gas: Option<u64>,
    pub(crate) opcode_count: Option<BTreeMap<String, u64>>,
    pub(crate) target_opcode: Option<String>,
    pub(crate) stateless_input_bytes: Vec<u8>,
    pub(crate) stateless_output_bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EestBlockchainTest {
    network: String,
    config: EestConfig,
    blocks: Vec<EestBlock>,
    #[serde(default, rename = "_info")]
    info: EestInfo,
}

#[derive(Debug, Deserialize)]
struct EestConfig {
    chainid: String,
}

#[derive(Debug, Default, Deserialize)]
struct EestInfo {
    #[serde(default)]
    metadata: EestMetadata,
}

/// EEST writes `_info.metadata` keys in snake case.
#[derive(Debug, Default, Deserialize)]
struct EestMetadata {
    #[serde(default)]
    opcode_count_per_block: Option<Vec<BTreeMap<String, u64>>>,
    #[serde(default)]
    target_opcode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EestBlock {
    #[serde(default)]
    stateless_input_bytes: Option<String>,
    #[serde(default)]
    stateless_output_bytes: Option<String>,
    #[serde(default)]
    block_header: Option<EestBlockHeader>,
    #[serde(default, rename = "blocknumber")]
    block_number: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EestBlockHeader {
    #[serde(default)]
    number: Option<String>,
    #[serde(default)]
    gas_used: Option<String>,
}

pub(crate) fn load_eest_benchmark_fixtures(
    value: serde_json::Value,
    path: &Path,
    input_root: &Path,
) -> Result<Vec<EestStatelessFixture>> {
    let cases: BTreeMap<String, EestBlockchainTest> =
        serde_json::from_value(value).with_context(|| {
            format!(
                "Failed to parse fixture {} as an EEST blockchain test",
                path.display()
            )
        })?;

    let source_path = relative_source_path(path, input_root);
    let mut fixtures = Vec::new();
    let mut fixture_names = HashSet::new();

    for (test_name, mut case) in cases {
        let chain_id = parse_json_u64(&case.config.chainid)
            .with_context(|| format!("Failed to parse chainid for EEST test {test_name}"))?;

        let opcode_count_per_block = case.info.metadata.opcode_count_per_block.as_ref();
        if opcode_count_per_block
            .is_some_and(|opcode_count_per_block| opcode_count_per_block.len() != case.blocks.len())
        {
            bail!(
                "EEST test {test_name} has {} opcode_count_per_block entries but {} blocks",
                opcode_count_per_block.unwrap().len(),
                case.blocks.len()
            );
        }

        // For EEST benchmark fixtures, the worst case block is the last block and the others are setup blocks,
        // so here we only load the last block as benchmark fixture.
        let block_index = case.blocks.len().saturating_sub(1);
        if let Some(block) = case.blocks.pop() {
            let Some(input_hex) = block.stateless_input_bytes else {
                info!(
                    "Skipping EEST test {test_name} block {block_index} from {source_path}: missing statelessInputBytes"
                );
                continue;
            };
            let stateless_input_bytes = decode_hex_bytes("statelessInputBytes", &input_hex)
                .with_context(|| {
                    format!(
                        "Failed to decode statelessInputBytes for EEST test {test_name} block {block_index}"
                    )
                })?;
            if stateless_input_bytes.is_empty() {
                info!(
                    "Skipping EEST test {test_name} block {block_index} from {source_path}: empty statelessInputBytes"
                );
                continue;
            }

            let output_hex = block.stateless_output_bytes.with_context(|| {
                format!(
                    "EEST test {test_name} block {block_index} has statelessInputBytes but no statelessOutputBytes"
                )
            })?;
            let stateless_output_bytes = decode_hex_bytes("statelessOutputBytes", &output_hex)
                .with_context(|| {
                    format!(
                        "Failed to decode statelessOutputBytes for EEST test {test_name} block {block_index}"
                    )
                })?;
            let block_number = parse_optional_json_u64(
                block
                    .block_header
                    .as_ref()
                    .and_then(|header| header.number.as_deref())
                    .or(block.block_number.as_deref()),
            )
            .with_context(|| {
                format!(
                    "Failed to parse block number for EEST test {test_name} block {block_index}"
                )
            })?;
            let block_used_gas = parse_optional_json_u64(
                block
                    .block_header
                    .as_ref()
                    .and_then(|header| header.gas_used.as_deref()),
            )
            .with_context(|| {
                format!("Failed to parse gas used for EEST test {test_name} block {block_index}")
            })?;

            fixtures.push(EestStatelessFixture {
                name: unique_eest_fixture_name(&test_name, block_index, &mut fixture_names),
                original_test_name: test_name.clone(),
                source_path: source_path.clone(),
                block_index,
                network: case.network.clone(),
                chain_id,
                block_number,
                block_used_gas,
                opcode_count: opcode_count_per_block
                    .map(|opcode_count_per_block| opcode_count_per_block[block_index].clone()),
                target_opcode: case.info.metadata.target_opcode,
                stateless_input_bytes,
                stateless_output_bytes,
            });
        }
    }

    Ok(fixtures)
}

fn decode_hex_bytes(field_name: &str, value: &str) -> Result<Vec<u8>> {
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);

    if !hex.len().is_multiple_of(2) {
        bail!("{field_name} must contain an even number of hex digits");
    }

    (0..hex.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&hex[index..index + 2], 16)
                .with_context(|| format!("{field_name} contains invalid hex at byte {index}"))
        })
        .collect()
}

fn parse_optional_json_u64(value: Option<&str>) -> Result<Option<u64>> {
    value.map(parse_json_u64).transpose()
}

fn parse_json_u64(value: &str) -> Result<u64> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16)
            .with_context(|| format!("failed to parse hex u64 value {value}"));
    }

    value
        .parse()
        .with_context(|| format!("failed to parse decimal u64 value {value}"))
}

fn relative_source_path(path: &Path, input_root: &Path) -> String {
    let relative = path
        .strip_prefix(input_root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(path);

    normalize_path_string(relative)
}

fn normalize_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn unique_eest_fixture_name(
    test_name: &str,
    block_index: usize,
    fixture_names: &mut HashSet<String>,
) -> String {
    let base = eest_fixture_name(test_name, block_index);
    let mut index = 1;

    loop {
        let suffix = if index == 1 {
            String::new()
        } else {
            format!("__{index}")
        };
        let candidate = truncate_fixture_name(&base, &suffix);

        if fixture_names.insert(candidate.clone()) {
            return candidate;
        }

        index += 1;
    }
}

fn eest_fixture_name(test_name: &str, block_index: usize) -> String {
    let sanitized = sanitize_fixture_name(test_name);

    format!("eest__{sanitized}__block{block_index}")
}

fn sanitize_fixture_name(value: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_separator = false;

    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            last_was_separator = false;
            ch
        } else if last_was_separator {
            continue;
        } else {
            last_was_separator = true;
            '_'
        };

        sanitized.push(next);
    }

    let sanitized = sanitized.trim_matches('_');
    if sanitized.is_empty() {
        return "fixture".to_string();
    }

    sanitized.to_string()
}

fn truncate_fixture_name(base: &str, suffix: &str) -> String {
    let base_max_len = EEST_SAFE_FILE_STEM_MAX_LEN.saturating_sub(suffix.len());
    if base.len() <= base_max_len {
        return format!("{base}{suffix}");
    }

    let truncated = base[..base_max_len].trim_end_matches('_');
    format!("{truncated}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_eest_fixture_flattens_blocks_and_preserves_raw_guest_io() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let fixture_path = dir
            .path()
            .join("blockchain_tests/for_amsterdam/compute/mcopy.json");
        fs::create_dir_all(fixture_path.parent().unwrap())?;
        fs::write(&fixture_path, sample_eest_fixture())?;

        let fixtures = load_eest_benchmark_fixtures(
            serde_json::from_str(sample_eest_fixture())?,
            &fixture_path,
            dir.path(),
        )?;
        assert_eq!(fixtures.len(), 2);

        let names: Vec<_> = fixtures
            .iter()
            .map(|fixture| fixture.name.clone())
            .collect();
        assert_eq!(
            names,
            vec![
                "eest__tests_foo_py_test_same_name_a__block2".to_string(),
                "eest__tests_foo_py_test_same_name_a__block2__2".to_string(),
            ]
        );
        assert!(names.iter().all(|name| name.starts_with("eest__")));
        assert!(names.iter().all(|name| !name.contains('/')));
        assert!(names.iter().all(|name| !name.contains(':')));
        assert!(names.iter().all(|name| !name.contains('[')));

        let fixture = fixtures
            .iter()
            .find(|fixture| fixture.original_test_name == "tests/foo.py::test_same[name/a]")
            .unwrap();
        assert_eq!(fixture.stateless_input_bytes, [0x15, 0x01, 0x02]);
        assert_eq!(fixture.stateless_output_bytes, [0xaa, 0xbb]);
        assert_eq!(
            fixture.source_path,
            "blockchain_tests/for_amsterdam/compute/mcopy.json"
        );
        assert_eq!(fixture.block_index, 2);
        assert_eq!(fixture.chain_id, 1);
        assert_eq!(fixture.block_number, Some(1));
        assert_eq!(fixture.block_used_gas, Some(16));
        assert_eq!(
            fixture.opcode_count,
            Some(BTreeMap::from([
                ("PUSH1".to_string(), 5),
                ("SSTORE".to_string(), 2)
            ]))
        );
        assert_eq!(fixture.target_opcode, Some("MCOPY".to_string()));

        let info_without_per_block = fixtures
            .iter()
            .find(|fixture| fixture.original_test_name == "tests/foo.py::test_same[name?a]")
            .unwrap();
        assert!(info_without_per_block.opcode_count.is_none());
        assert_eq!(
            info_without_per_block.target_opcode,
            Some("ADD".to_string())
        );

        Ok(())
    }

    #[test]
    fn eest_fixture_name_preserves_full_sanitized_name_when_it_fits() {
        let name = eest_fixture_name(
            "tests/amsterdam/eip8025_optional_proofs/test_witness_state_writes.py::test_witness_state_sstore_into_empty_storage_omits_post_state_nodes[fork_Amsterdam-blockchain_test]",
            0,
        );

        assert_eq!(
            name,
            "eest__tests_amsterdam_eip8025_optional_proofs_test_witness_state_writes_py_test_witness_state_sstore_into_empty_storage_omits_post_state_nodes_fork_Amsterdam-blockchain_test__block0"
        );
    }

    #[test]
    fn eest_fixture_name_truncates_only_when_it_exceeds_safe_file_stem_limit() {
        let test_name = format!("tests/foo.py::test_{}", "a".repeat(300));
        let name = eest_fixture_name(&test_name, 0);
        let truncated = truncate_fixture_name(&name, "");

        assert!(name.len() > EEST_SAFE_FILE_STEM_MAX_LEN);
        assert_eq!(truncated.len(), EEST_SAFE_FILE_STEM_MAX_LEN);
        assert!(truncated.starts_with("eest__tests_foo_py_test_"));
        assert!(!truncated.ends_with('_'));
    }

    #[test]
    fn eest_block_with_input_requires_output() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let fixture_path = dir.path().join("missing-output.json");
        fs::write(
            &fixture_path,
            r#"{
                "tests/foo.py::test_missing_output": {
                    "network": "Amsterdam",
                    "config": {"chainid": "0x01"},
                    "blocks": [{"statelessInputBytes": "0x0102"}]
                }
            }"#,
        )?;

        let err = load_eest_benchmark_fixtures(
            serde_json::from_str(&fs::read_to_string(&fixture_path)?)?,
            &fixture_path,
            dir.path(),
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("has statelessInputBytes but no statelessOutputBytes"));

        Ok(())
    }

    #[test]
    fn eest_opcode_count_per_block_length_must_match_blocks() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let fixture_path = dir.path().join("mismatched-opcode-count.json");
        fs::write(
            &fixture_path,
            r#"{
                "tests/foo.py::test_mismatch": {
                    "network": "Amsterdam",
                    "config": {"chainid": "0x01"},
                    "blocks": [
                        {"statelessInputBytes": "0x0102", "statelessOutputBytes": "0xaa"}
                    ],
                    "_info": {
                        "metadata": {
                            "opcode_count_per_block": [{"PUSH1": 1}, {"PUSH1": 2}]
                        }
                    }
                }
            }"#,
        )?;

        let err = load_eest_benchmark_fixtures(
            serde_json::from_str(&fs::read_to_string(&fixture_path)?)?,
            &fixture_path,
            dir.path(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("opcode_count_per_block"));

        Ok(())
    }

    pub(crate) fn sample_eest_fixture() -> &'static str {
        r#"{
            "tests/foo.py::test_same[empty_input]": {
                "network": "Amsterdam",
                "config": {"chainid": "0x01"},
                "blocks": [
                    {
                        "statelessInputBytes": "0x",
                        "statelessOutputBytes": "0xcc"
                    }
                ]
            },
            "tests/foo.py::test_same[name/a]": {
                "network": "Amsterdam",
                "config": {"chainid": "0x01"},
                "blocks": [
                    {
                        "statelessInputBytes": "0x150102",
                        "statelessOutputBytes": "0xcc"
                    },
                    {
                        "blockHeader": {"number": "0x02", "gasUsed": "0x20"}
                    },
                    {
                        "statelessInputBytes": "0x150102",
                        "statelessOutputBytes": "0xaabb",
                        "blockHeader": {"number": "0x01", "gasUsed": "0x10"}
                    }
                ],
                "_info": {
                    "metadata": {
                        "opcode_count": {"PUSH1": 8, "SSTORE": 2, "MCOPY": 7},
                        "target_opcode": "MCOPY",
                        "opcode_count_per_block": [
                            {"MCOPY": 7},
                            {"PUSH1": 3},
                            {"PUSH1": 5, "SSTORE": 2}
                        ]
                    }
                }
            },
            "tests/foo.py::test_same[name?a]": {
                "network": "Amsterdam",
                "config": {"chainid": "0x01"},
                "blocks": [
                    {},
                    {},
                    {
                        "statelessInputBytes": "0x0f",
                        "statelessOutputBytes": "0xdead",
                        "blocknumber": "0x03",
                        "blockHeader": {"gasUsed": "0x30"}
                    }
                ],
                "_info": {
                    "metadata": {
                        "opcode_count": {"ADD": 3},
                        "target_opcode": "ADD"
                    }
                }
            }
        }"#
    }
}
