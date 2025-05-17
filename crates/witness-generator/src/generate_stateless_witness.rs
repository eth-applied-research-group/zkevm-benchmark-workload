use ef_tests::{
    Case,
    cases::blockchain_test::{BlockchainTestCase, run_case},
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use reth_chainspec::ChainSpec;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

use crate::BlocksAndWitnesses;
use reth_stateless::ClientInput;

/// Root directory for the relevant blockchain tests within the `zkevm-fixtures` submodule.
const BLOCKCHAIN_TEST_DIR: &str = "blockchain_tests";

/// Generates `BlocksAndWitnesses` for all valid blockchain test cases found
/// within the specified `BLOCKCHAIN_TEST_DIR` directory in `zkevm-fixtures`.
///
/// It walks the target directory, parses each JSON test file, executes the test
/// using `ef_tests`, collects the resulting block/witness pairs, and packages them.
///
/// Uses `rayon` for parallel processing of test cases within a single file.
///
/// # Panics
///
/// - If the `zkevm-fixtures` directory cannot be located relative to the crate root.
/// - If the target `BLOCKCHAIN_TEST_DIR` directory does not exist.
/// - If a JSON test case file cannot be parsed.
/// - If `ef_tests::cases::blockchain_test::run_case` fails for a test.
pub fn generate() -> Vec<BlocksAndWitnesses> {
    // First get the path to "BLOCKCHAIN_TEST_DIR"
    // TODO: Maybe we should have this be passed as a parameter in the future
    let suite_path = path_to_zkevm_fixtures(BLOCKCHAIN_TEST_DIR);
    // Verify that the path exists
    assert!(
        suite_path.exists(),
        "Test suite path does not exist: {suite_path:?}"
    );

    // Find all files with the ".json" extension in the test suite directory
    // Each Json file corresponds to a BlockchainTestCase
    let test_cases: Vec<_> = find_all_files_with_extension(&suite_path, ".json")
        .into_iter()
        .map(|test_case_path| {
            let case = BlockchainTestCase::load(&test_case_path).expect("test case should load");
            (test_case_path, case)
        })
        .collect();

    let mut blocks_and_witnesses = Vec::new();
    for (_, test_case) in test_cases {
        let blockchain_case: Vec<BlocksAndWitnesses> = test_case
            // Inside of a JSON file, we can have multiple tests, for example testopcode_Cancun,
            // testopcode_Prague
            // This is why we have `tests`.
            .tests
            .par_iter()
            // TODO: We shouldn't need this since we are generating specific tests and have control
            // TODO: over the network.
            .filter(|(_, case)| !BlockchainTestCase::excluded_fork(case.network))
            .map(|(name, case)| BlocksAndWitnesses {
                name: name.to_string(),
                blocks_and_witnesses: run_case(case)
                    .unwrap()
                    .into_iter()
                    .map(|(block, witness)| ClientInput { block, witness })
                    .collect(),
                network: ChainSpec::from(case.network).genesis,
            })
            .collect();
        blocks_and_witnesses.extend(blockchain_case);
    }

    blocks_and_witnesses
}

/// Recursively finds all files within `path` that end with `extension`.
// This function was copied from `ef-tests`
fn find_all_files_with_extension(path: &Path, extension: &str) -> Vec<PathBuf> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(extension))
        .map(DirEntry::into_path)
        .collect()
}

/// Constructs the absolute path to a subdirectory within the `zkevm-fixtures` submodule.
///
/// Assumes this crate (`witness-generator`) is located at `<workspace-root>/crates/witness-generator`.
///
fn path_to_zkevm_fixtures(suite: &str) -> PathBuf {
    let workspace_root = Path::new(env!("CARGO_WORKSPACE_DIR"));
    workspace_root
        .join("zkevm-fixtures")
        .join("fixtures")
        .join(suite)
}
