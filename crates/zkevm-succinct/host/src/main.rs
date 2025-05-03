//! TODO

use generate_stateless_witness::generate;
use metrics::WorkloadMetrics;
use sp1_sdk::{ProverClient, SP1Stdin};
use std::collections::HashMap;
use witness_generator::generate_stateless_witness;

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const STATELESS_ELF: &[u8] = include_bytes!(concat!(
    env!("CARGO_WORKSPACE_DIR"),
    "/target/elf-compilation/riscv32im-succinct-zkvm-elf/release/stateless-program"
));

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
