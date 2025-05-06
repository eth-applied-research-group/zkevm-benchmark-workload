#![doc = include_str!("../../README.md")]

use benchmark_runner::run_benchmark;
use sp1_sdk::{ProverClient, SP1Stdin};
use std::collections::HashMap;
use witness_generator::BlocksAndWitnesses;
use zkevm_metrics::WorkloadMetrics;

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
fn main() {
    // Setup the logger.
    sp1_sdk::utils::setup_logger();
    dotenv::dotenv().ok();

    // Setup the prover client.
    let client = ProverClient::from_env();

    run_benchmark(
        STATELESS_ELF,
        "succinct",
        |blockchain_corpus: &BlocksAndWitnesses, elf_path: &'static [u8]| {
            let mut reports = Vec::new();
            let name = &blockchain_corpus.name;

            for client_input in &blockchain_corpus.blocks_and_witnesses {
                let block_number = client_input.block.number;
                let mut stdin = SP1Stdin::new();
                stdin.write(client_input);
                stdin.write(&blockchain_corpus.network);

                let (_, report) = client.execute(elf_path, &stdin).run().unwrap();

                let total_num_cycles = report.total_instruction_count();
                let region_cycles: HashMap<_, _> = report.cycle_tracker.into_iter().collect();

                let metrics = WorkloadMetrics {
                    name: format!("{}-{}", name, block_number),
                    total_num_cycles,
                    region_cycles,
                };
                reports.push(metrics);
            }
            reports
        },
    );
}
