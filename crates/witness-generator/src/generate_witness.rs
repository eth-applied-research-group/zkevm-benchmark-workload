use ef_tests::{
    Case,
    cases::blockchain_test::{BlockchainTestCase, run_case},
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

use crate::BlocksAndWitnesses;
use reth_stateless::ClientInput;

/// Name of a directory in the ethereum spec tests
const VALID_BLOCKS: &str = "blockchain_tests/prague/eip2537_bls_12_381_precompiles";

/// This method will fetch all tests in the ethereum-tests/Blockchaintests/ValidBlocks folder
/// and generate a stateless witness for them.
pub fn generate() -> Vec<BlocksAndWitnesses> {
    // First get the path to "ValidBlocks"
    let suite_path = path_to_zkevm_fixtures(VALID_BLOCKS);
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
    for (_, test_case) in test_cases.into_iter() {
        // if test_case.skip {
        //     continue;
        // }
        let blockchain_case: Vec<BlocksAndWitnesses> = test_case
            // Inside of a JSON file, we can have multiple tests, for example testopcode_Cancun,
            // testopcode_Prague
            // This is why we have `tests`.
            .tests
            .par_iter()
            // .filter(|(_, case)| !BlockchainTestCase::excluded_fork(case.network))
            .map(|(name, case)| BlocksAndWitnesses {
                name: name.to_string(),
                blocks_and_witnesses: run_case(case)
                    .unwrap()
                    .into_iter()
                    .map(|(block, witness)| ClientInput { block, witness })
                    .collect(),
                network: case.network.into(),
            })
            .collect();
        blocks_and_witnesses.extend(blockchain_case);
    }

    blocks_and_witnesses
}

/// Recursively find all files with a given extension.
// This function was copied from `ef-tests`
fn find_all_files_with_extension(path: &Path, extension: &str) -> Vec<PathBuf> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(extension))
        .map(DirEntry::into_path)
        .collect()
}

/// Path to the zkevm-fixtures in the ef-tests crate
fn path_to_zkevm_fixtures(suite: &str) -> PathBuf {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("should be at the crates directory")
        .parent()
        .expect("should be at the workspace directory");

    workspace_root
        .join("zkevm-fixtures")
        .join("fixtures")
        .join(suite)
}
