use methods::RISC0_GUEST_ELF;
use risc0_zkvm::{default_prover, ExecutorEnv};
use witness_generator::generate_stateless_witness::generate;

fn main() {
    // Initialize tracing. In order to view logs, run `RUST_LOG=info cargo run`
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    // Obtain the default prover.
    let prover = default_prover();

    run_benchmark(
        RISC0GUEST_ELF,
        "risc0",
        |blockchain_corpus: &BlocksAndWitnesses, _elf_data: &'static [u8]| {
            let mut reports = Vec::new();
            let corpus_name = &blockchain_corpus.name;
            let num_blocks_in_corpus = blockchain_corpus.blocks_and_witnesses.len();

            for (block_id, client_input) in
                blockchain_corpus.blocks_and_witnesses.iter().enumerate()
            {
                let block_number = client_input.block.number;

                let env = ExecutorEnv::builder()
                    .write(&client_input)
                    .unwrap()
                    .write(&blockchain_corpus.network)
                    .unwrap()
                    .build()
                    .unwrap();

                // Proof information by proving the specified ELF binary.
                let prove_info = prover.prove(env, RISC0GUEST_ELF).unwrap();
                let receipt = prove_info.receipt;
                let total_num_cycles = receipt.cycles;

                // RISC0 receipt does not provide detailed region cycle counts by default.
                // We'll use an empty HashMap for region_cycles.
                let region_cycles: HashMap<String, u64> = HashMap::new();

                let metrics = WorkloadMetrics {
                    name: format!("{}-{}", corpus_name, block_number),
                    total_num_cycles,
                    region_cycles,
                };
                reports.push(metrics);
            }
            reports
        },
    );
}
