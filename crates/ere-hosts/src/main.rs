use benchmark_runner::run_benchmark;
use std::collections::HashMap;
use witness_generator::{generate_stateless_witness, BlocksAndWitnesses};
use zkevm_metrics::WorkloadMetrics;

/// Main entry point for the host benchmarker
fn main() {
    dotenv::dotenv().ok();

    // List all of the supported ere hosts that we want to benchmark
    let hosts = vec![("sp1", "ere-guests/sp1")];

    // Generate corpus
    let generated_corpuses = generate_stateless_witness::generate();

    for (zkvm_name, path_to_guest) in hosts {
        // Compile the guest program using zkevm interface
        // let program = ere_sp1::

        generated_corpuses.into_par_iter().for_each(|bw| {
            println!("{} (num_blocks={})", bw.name, bw.blocks_and_witnesses.len());

            // Add input using zkVM input struct

            // Execute the guest program using zkvm interface
            // TODO: Add an enum for whether we should execute or prove

            WorkloadMetrics::to_path(
                format!(
                    "{}/{}/{}/{}.json",
                    env!("CARGO_WORKSPACE_DIR"),
                    "zkevm-metrics",
                    zkvm_name,
                    bw.name
                ),
                &reports,
            )
            .unwrap();

            println!(
                "Finished processing and saved metrics for corpus: {}. Number of reports: {}",
                bw.name,
                reports.len()
            );
        });
    }
}
