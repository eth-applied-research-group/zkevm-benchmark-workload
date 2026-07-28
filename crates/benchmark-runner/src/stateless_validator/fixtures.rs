use crate::{
    guest_programs::GuestFixture,
    stateless_validator::{
        eest::{load_eest_benchmark_fixtures, EestStatelessFixture},
        inputs::stateless_validator_input_from_fixture,
        ExecutionClient,
    },
};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tracing::info;
use walkdir::WalkDir;

const EEST_BLOCKCHAIN_TESTS_DIR: &str = "blockchain_tests";
const EEST_BLOCKCHAIN_TESTS_ENGINE_DIR: &str = "blockchain_tests_engine";
const EEST_BLOCKCHAIN_TESTS_ENGINE_X_DIR: &str = "blockchain_tests_engine_x";
const EEST_BLOCKCHAIN_TESTS_SYNC_DIR: &str = "blockchain_tests_sync";

/// Lazily walks a fixture folder and yields each fixture file path.
pub fn iter_benchmark_fixture_paths(path: &Path) -> impl Iterator<Item = PathBuf> {
    let min_depth = if path.is_file() { 0 } else { 1 };

    WalkDir::new(path)
        .min_depth(min_depth)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter(|entry| {
            !entry
                .path()
                .components()
                .any(|component| component.as_os_str() == ".meta")
        })
        .map(walkdir::DirEntry::into_path)
}

/// Resolves benchmark fixture JSON paths.
pub fn benchmark_fixture_paths(input_folder: &Path) -> Result<Vec<PathBuf>> {
    let fixture_root = benchmark_fixture_root(input_folder)?;
    Ok(iter_benchmark_fixture_paths(&fixture_root).collect())
}

fn benchmark_fixture_root(input_folder: &Path) -> Result<PathBuf> {
    if input_folder.is_file() {
        return Ok(input_folder.to_path_buf());
    }

    let blockchain_tests = input_folder.join(EEST_BLOCKCHAIN_TESTS_DIR);
    if blockchain_tests.is_dir() {
        return Ok(blockchain_tests);
    }

    if looks_like_eest_fixture_bundle(input_folder) {
        bail!(
            "EEST fixture bundle {} does not contain required {}/ directory; \
             stateless-validator supports EEST blockchain_test fixtures, \
             not blockchain_test_engine-only fixtures",
            input_folder.display(),
            EEST_BLOCKCHAIN_TESTS_DIR
        );
    }

    Ok(input_folder.to_path_buf())
}

fn looks_like_eest_fixture_bundle(input_folder: &Path) -> bool {
    input_folder.join(EEST_BLOCKCHAIN_TESTS_ENGINE_DIR).is_dir()
        || input_folder
            .join(EEST_BLOCKCHAIN_TESTS_ENGINE_X_DIR)
            .is_dir()
        || input_folder.join(EEST_BLOCKCHAIN_TESTS_SYNC_DIR).is_dir()
        || eest_fixture_index_has_formats(input_folder)
}

fn eest_fixture_index_has_formats(input_folder: &Path) -> bool {
    let index_path = input_folder.join(".meta").join("index.json");
    let Ok(content) = std::fs::read(index_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&content) else {
        return false;
    };

    value.get("fixture_formats").is_some()
}

pub(super) fn stateless_validator_input_iter(
    input_folder: &Path,
    selected_fixtures: Option<&[String]>,
    el: ExecutionClient,
    existing_output_dir: Option<&Path>,
) -> Result<impl Iterator<Item = Result<Box<dyn GuestFixture>>>> {
    let fixture_prefixes = selected_fixtures
        .filter(|fixtures| !fixtures.is_empty())
        .map(normalize_fixture_prefixes)
        .transpose()?;

    Ok(stateless_validator_input_iter_from_paths(
        benchmark_fixture_paths(input_folder)?.into_iter(),
        input_folder.to_path_buf(),
        fixture_prefixes,
        el,
        existing_output_dir.map(Path::to_path_buf),
    ))
}

fn stateless_validator_input_iter_from_paths<I>(
    paths: I,
    input_root: PathBuf,
    fixture_prefixes: Option<Vec<String>>,
    el: ExecutionClient,
    existing_output_dir: Option<PathBuf>,
) -> impl Iterator<Item = Result<Box<dyn GuestFixture>>>
where
    I: Iterator<Item = PathBuf>,
{
    paths.flat_map(move |path| {
        let results: Vec<_> = match load_benchmark_fixtures(&path, &input_root) {
            Ok(fixtures) => fixtures
                .into_iter()
                .filter(|fixture| fixture_matches_prefixes(fixture, fixture_prefixes.as_deref()))
                .filter_map(|fixture| {
                    match skip_existing_fixture_output(
                        &fixture.name,
                        existing_output_dir.as_deref(),
                    ) {
                        Ok(true) => None,
                        Ok(false) => Some(stateless_validator_input_from_fixture(fixture, el)),
                        Err(err) => Some(Err(err)),
                    }
                })
                .collect(),
            Err(err) => vec![Err(err)],
        };
        results
    })
}

fn fixture_matches_prefixes(fixture: &EestStatelessFixture, prefixes: Option<&[String]>) -> bool {
    let Some(prefixes) = prefixes else {
        return true;
    };

    prefixes.iter().any(|prefix| {
        fixture.name.starts_with(prefix) || fixture.original_test_name.starts_with(prefix)
    })
}

fn load_benchmark_fixtures(path: &Path, input_root: &Path) -> Result<Vec<EestStatelessFixture>> {
    let content = std::fs::read(path)?;
    let value: serde_json::Value = serde_json::from_slice(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    if value.get("stateless_input").is_some() {
        bail!(
            "legacy fixture format with top-level stateless_input is no longer supported; provide an EEST blockchain_tests fixture containing statelessInputBytes and statelessOutputBytes"
        );
    }

    load_eest_benchmark_fixtures(value, path, input_root)
}

fn skip_existing_fixture_output(
    fixture_name: &str,
    existing_output_dir: Option<&Path>,
) -> Result<bool> {
    let Some(existing_output_dir) = existing_output_dir else {
        return Ok(false);
    };

    let output_path = existing_output_dir.join(format!("{fixture_name}.json"));
    if output_path.exists() {
        info!("Skipping {fixture_name} (already exists)");
        return Ok(true);
    }

    Ok(false)
}

fn normalize_fixture_prefixes(prefixes: &[String]) -> Result<Vec<String>> {
    let mut normalized_prefixes = Vec::with_capacity(prefixes.len());
    let mut seen_prefixes = HashSet::new();

    for prefix in prefixes {
        let normalized_prefix = normalize_fixture_prefix(prefix)?;
        if seen_prefixes.insert(normalized_prefix.clone()) {
            normalized_prefixes.push(normalized_prefix);
        }
    }

    Ok(normalized_prefixes)
}

fn normalize_fixture_prefix(prefix: &str) -> Result<String> {
    let normalized = prefix.trim();
    if normalized.is_empty() {
        bail!("Fixture prefix cannot be empty");
    }

    Ok(normalized
        .strip_suffix(".json")
        .unwrap_or(normalized)
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn fixture_iter_yields_sorted_paths() -> Result<()> {
        let dir = tempfile::tempdir()?;

        let paths = (24950000..)
            .take(100)
            .map(|block_number| dir.path().join(format!("{block_number}.json")))
            .collect::<Vec<_>>();

        for path in &paths {
            fs::File::create(path)?;
        }

        assert_eq!(paths, benchmark_fixture_paths(dir.path())?);

        Ok(())
    }

    #[test]
    fn fixture_prefix_matching_accepts_safe_and_original_eest_names() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let fixture_path = dir.path().join("mcopy.json");
        fs::write(&fixture_path, sample_eest_fixture())?;
        let fixtures = load_benchmark_fixtures(&fixture_path, dir.path())?;
        let fixture = fixtures
            .iter()
            .find(|fixture| fixture.original_test_name == "tests/foo.py::test_same[name?a]")
            .unwrap();

        let safe_prefix = normalize_fixture_prefixes(&[format!("{}.json", fixture.name)])?;
        assert!(fixture_matches_prefixes(fixture, Some(&safe_prefix)));

        let original_prefix =
            normalize_fixture_prefixes(&["tests/foo.py::test_same[name?a]".to_string()])?;
        assert!(fixture_matches_prefixes(fixture, Some(&original_prefix)));

        let non_matching_prefix = normalize_fixture_prefixes(&["other_test".to_string()])?;
        assert!(!fixture_matches_prefixes(
            fixture,
            Some(&non_matching_prefix)
        ));

        Ok(())
    }

    #[test]
    fn eest_fixture_iter_yields_raw_input_and_output() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let fixture_path = dir.path().join("mcopy.json");
        fs::write(&fixture_path, sample_eest_fixture())?;

        let selected = vec!["tests/foo.py::test_same[name/a]".to_string()];
        let mut fixtures = stateless_validator_input_iter(
            dir.path(),
            Some(&selected),
            ExecutionClient::Reth,
            None,
        )?;
        let guest_fixture = fixtures.next().unwrap()?;
        assert!(fixtures.next().is_none());

        let input = guest_fixture.input()?;
        assert_eq!(input.stdin(), [0x15, 0x01, 0x02]);
        assert_eq!(guest_fixture.expected_public_values()?, [0xaa, 0xbb]);

        let metadata = guest_fixture.metadata();
        assert_eq!(metadata["fixture_format"], "eest");
        assert_eq!(
            metadata["original_test_name"],
            "tests/foo.py::test_same[name/a]"
        );
        assert_eq!(metadata["block_used_gas"].as_u64(), Some(16));
        assert_eq!(metadata["opcode_count"]["PUSH1"].as_u64(), Some(5));
        assert_eq!(metadata["opcode_count"]["SSTORE"].as_u64(), Some(2));
        assert_eq!(metadata["target_opcode"], "MCOPY");

        Ok(())
    }

    #[test]
    fn eest_fixture_iter_skips_existing_outputs() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let fixture_path = dir.path().join("mcopy.json");
        fs::write(&fixture_path, sample_eest_fixture())?;

        let loaded = load_benchmark_fixtures(&fixture_path, dir.path())?;
        let output_dir = dir.path().join("output");
        fs::create_dir(&output_dir)?;
        fs::write(output_dir.join(format!("{}.json", loaded[0].name)), "{}")?;

        let fixtures = stateless_validator_input_iter(
            &fixture_path,
            None,
            ExecutionClient::Reth,
            Some(&output_dir),
        )?
        .collect::<Result<Vec<_>>>()?;
        assert_eq!(fixtures.len(), 1);
        assert_eq!(fixtures[0].name(), loaded[1].name);

        Ok(())
    }

    #[test]
    fn legacy_fixture_is_rejected_with_migration_error() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let fixture_path = dir.path().join("legacy.json");
        fs::write(&fixture_path, r#"{"stateless_input": {}}"#)?;

        let err = load_benchmark_fixtures(&fixture_path, dir.path()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "legacy fixture format with top-level stateless_input is no longer supported; provide an EEST blockchain_tests fixture containing statelessInputBytes and statelessOutputBytes"
        );

        Ok(())
    }

    #[test]
    fn fixture_path_discovery_skips_meta_and_non_json_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join(".meta"))?;
        fs::write(dir.path().join(".meta/index.json"), "{}")?;
        fs::write(dir.path().join("ignored.txt"), "not json")?;
        fs::write(dir.path().join("fixture.json"), "{}")?;

        let paths: Vec<_> = iter_benchmark_fixture_paths(dir.path()).collect();
        assert_eq!(paths, vec![dir.path().join("fixture.json")]);

        Ok(())
    }

    #[test]
    fn benchmark_fixture_paths_prefers_eest_blockchain_tests_subdir() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let included_path = dir
            .path()
            .join("blockchain_tests/for_amsterdam/included.json");
        let engine_path = dir
            .path()
            .join("blockchain_tests_engine/for_amsterdam/ignored.json");
        fs::create_dir_all(included_path.parent().unwrap())?;
        fs::create_dir_all(engine_path.parent().unwrap())?;
        fs::write(&included_path, "{}")?;
        fs::write(&engine_path, "{}")?;

        let paths = benchmark_fixture_paths(dir.path())?;
        assert_eq!(paths, vec![included_path]);

        Ok(())
    }

    #[test]
    fn benchmark_fixture_paths_rejects_engine_only_eest_bundle() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let engine_path = dir
            .path()
            .join("blockchain_tests_engine/for_amsterdam/ignored.json");
        fs::create_dir_all(engine_path.parent().unwrap())?;
        fs::write(&engine_path, "{}")?;

        let err = benchmark_fixture_paths(dir.path()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("blockchain_tests"));
        assert!(message.contains("blockchain_test_engine-only"));

        Ok(())
    }

    fn sample_eest_fixture() -> &'static str {
        r#"{
            "tests/foo.py::test_same[name/a]": {
                "network": "Amsterdam",
                "config": {"chainid": "0x01"},
                "blocks": [
                    {
                        "statelessInputBytes": "0x150102",
                        "statelessOutputBytes": "0xaabb",
                        "blockHeader": {"number": "0x01", "gasUsed": "0x10"}
                    }
                ],
                "_info": {
                    "metadata": {
                        "opcode_count_per_block": [{"PUSH1": 5, "SSTORE": 2}],
                        "target_opcode": "MCOPY"
                    }
                }
            },
            "tests/foo.py::test_same[name?a]": {
                "network": "Amsterdam",
                "config": {"chainid": "0x01"},
                "blocks": [
                    {
                        "statelessInputBytes": "0x0f",
                        "statelessOutputBytes": "0xdead",
                        "blocknumber": "0x03",
                        "blockHeader": {"gasUsed": "0x30"}
                    }
                ]
            }
        }"#
    }
}
