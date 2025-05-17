#![doc = include_str!("../../README.md")]

use benchmark_runner::run_benchmark;
use zkvm_interface::{zkVM, Input};
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
    run_benchmark(
        STATELESS_ELF,
        "succinct",
        |blockchain_corpus: &BlocksAndWitnesses, elf_path: &'static [u8]| {
            let mut reports = Vec::new();
            let name = &blockchain_corpus.name;
            
            let zkvm = ere_sp1::EreSP1::new(elf_path.to_vec());
            for client_input in &blockchain_corpus.blocks_and_witnesses {
                let block_number = client_input.block.number;
                let mut stdin = Input::new();
                stdin.write(client_input).unwrap();
                stdin.write(&blockchain_corpus.network).unwrap();
                
                
                let report = zkvm.execute(&stdin).unwrap();

                let region_cycles : HashMap<_, _>= report.region_cycles.into_iter().collect();

                let metrics = WorkloadMetrics {
                    name: format!("{}-{}", name, block_number),
                    total_num_cycles : report.total_num_cycles,
                    region_cycles,
                };
                reports.push(metrics);
            }
            reports
        },
    );
}
