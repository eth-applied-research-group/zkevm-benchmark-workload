#![doc = include_str!("../../README.md")]

use generate_stateless_witness::generate;
use zkevm_metrics::WorkloadMetrics;
use zkm_sdk::{ProverClient, ZKMStdin};

use std::collections::HashMap;
use witness_generator::BlocksAndWitnesses;
use zkm_sdk::{ProverClient, ZKMStdin};

/// Path to the compiled MIPS ELF file for the `zkm-guest` crate.
pub const STATELESS_ELF: &[u8] = include_bytes!(concat!(
    env!("CARGO_WORKSPACE_DIR"),
    "/target/mipsel-zkm-zkvm-elf/release/zkm-guest"
));

/// Main entry point for the host benchmarker.
///
/// This program orchestrates the execution of Ethereum block validation
/// within the zkMIPS zkVM for various test cases and records performance metrics.
fn main() {
    // Setup the logger.
    zkm_sdk::utils::setup_logger();
    dotenv::dotenv().ok();

    // Setup the prover client.
    let client = ProverClient::cpu();

    run_benchmark(
        STATELESS_ELF,
        "zkm",
        |blockchain_corpus: &BlocksAndWitnesses, elf_path: &'static [u8]| {
            let mut reports = Vec::new();
            let name = &blockchain_corpus.name;
            let num_blocks_in_corpus = blockchain_corpus.blocks_and_witnesses.len();

            for client_input in &blockchain_corpus.blocks_and_witnesses {
                let block_number = client_input.block.number;
                let mut stdin = ZKMStdin::new();
                stdin.write(client_input);
                stdin.write(&blockchain_corpus.network);

                let (_, report) = client.execute(elf_path, stdin).run().unwrap();

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
