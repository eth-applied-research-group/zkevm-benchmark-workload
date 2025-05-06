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

    let generated = generate();
    let num_corpuses = generated.len();

    for (corpus_id, blockchain_corpus) in generated.into_iter().enumerate() {
        // let mut reports = Vec::new();
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

            let env = ExecutorEnv::builder()
                .write(&client_input)
                .unwrap()
                .write(&blockchain_corpus.network)
                .unwrap()
                .build()
                .unwrap();

            // Proof information by proving the specified ELF binary.
            // This struct contains the receipt along with statistics about execution of the guest
            let _prove_info = prover.prove(env, RISC0_GUEST_ELF).unwrap();
        }
    }
}
