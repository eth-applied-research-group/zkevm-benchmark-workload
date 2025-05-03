#![doc = include_str!("../../README.md")]

use generate_stateless_witness::generate;
use metrics::WorkloadMetrics;
use sp1_sdk::{ProverClient, SP1Stdin};
use std::collections::HashMap;
use witness_generator::generate_stateless_witness;

/// Path to the compiled RISC-V ELF file for the `succinct-guest` crate.
///
/// This constant assumes the ELF has been built using `cargo prove build --release`
/// within the `crates/zkevm-succinct/succinct-guest` directory.
pub const STATELESS_ELF: &[u8] = include_bytes!(concat!(
    env!("CARGO_WORKSPACE_DIR"),
    "/target/elf-compilation/riscv32im-succinct-zkvm-elf/release/succinct-guest"
));

/// Main entry point for the host benchmarker.
///
/// This program orchestrates the execution of Ethereum block validation
/// within the SP1 zkVM for various test cases and records performance metrics.
///
/// # Steps:
/// 1. Initializes logging and loads environment variables (e.g., for ProverClient).
/// 2. Sets up the SP1 `ProverClient`.
/// 3. Generates test data (`Vec<BlocksAndWitnesses>`) using `witness-generator`.
/// 4. Iterates through each test corpus and each block within the corpus.
/// 5. For each block:
///    a. Creates SP1 standard input (`SP1Stdin`) containing the `ClientInput` and `ForkSpec`.
///    b. Executes the `succinct-guest` ELF (`STATELESS_ELF`) in the SP1 zkVM using `client.execute()`.
///    c. Extracts total cycle count and region-specific cycle counts from the execution report.
///    d. Formats the results into a `WorkloadMetrics` struct.
/// 6. Saves the collected `WorkloadMetrics` for each corpus to a JSON file in `zkevm-metrics/succinct/`.
/// 7. Prints the collected metrics to standard output for immediate feedback.
///
/// # Panics
/// - If `ProverClient` setup fails (missing environment variables).
/// - If `witness-generator::generate()` panics.
/// - If SP1 execution (`client.execute().run()`) fails.
/// - If writing the metrics JSON file fails.
fn main() {
    // Setup the logger.
    sp1_sdk::utils::setup_logger();
    dotenv::dotenv().ok();

    // Setup the prover client.
    let client = ProverClient::from_env();

    let generated = generate();
    let num_corpuses = generated.len();

    for (corpus_id, blockchain_corpus) in generated.into_iter().enumerate() {
        let mut reports = Vec::new();
        // Iterate each block in the mini blockchain
        let name = blockchain_corpus.name.clone();
        let num_blocks_in_corpus = blockchain_corpus.blocks_and_witnesses.len();

        println!("Processing corpus {} / {num_corpuses}", corpus_id + 1);
        println!("Corpus name: {name}");
        println!("Num blocks in corpus: {num_blocks_in_corpus}\n");

        for (block_id, client_input) in blockchain_corpus.blocks_and_witnesses.iter().enumerate() {
            println!(
                "   Processing block {} / {num_blocks_in_corpus}",
                block_id + 1
            );

            let block_number = client_input.block.number;
            let mut stdin = SP1Stdin::new();
            stdin.write(client_input);
            stdin.write(&blockchain_corpus.network);

            let (_, report) = client.execute(STATELESS_ELF, &stdin).run().unwrap();

            let total_num_cycles = report.total_instruction_count();
            let region_cycles: HashMap<_, _> = report.cycle_tracker.into_iter().collect();

            let metrics = WorkloadMetrics {
                name: format!("{}-{}", name, block_number),
                total_num_cycles,
                region_cycles,
            };
            reports.push(metrics);
        }
        WorkloadMetrics::to_path(
            &format!(
                "{}/{}/{}/{}.json",
                env!("CARGO_WORKSPACE_DIR"),
                "zkevm-metrics",
                "succinct",
                name
            ),
            &reports,
        )
        .unwrap();
        // Print out the reports to std for now
        // We can prettify it later.
        dbg!(&reports);
    }
}
