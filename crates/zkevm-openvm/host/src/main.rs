//! Host program for Ethereum validation

use eyre::Result;
use openvm_build::GuestOptions;
use openvm_sdk::{Sdk, StdIn, config::SdkVmConfig};
use openvm_transpiler::elf::Elf;
use std::path::Path;

use generate_stateless_witness::generate;
use std::collections::HashMap;
use witness_generator::generate_stateless_witness;

fn main() -> Result<()> {
    let sdk = Sdk::new();
    let vm_cfg = SdkVmConfig::builder()
        .system(Default::default())
        .rv32i(Default::default())
        .rv32m(Default::default())
        .io(Default::default())
        .build();

    // Build the guest crate
    let guest_path = Path::new("../program");
    let elf: Elf = sdk.build(GuestOptions::default(), guest_path, &Default::default())?;

    // ---------- 3 · transpile and execute ---------------------------------
    let exe = sdk.transpile(elf, vm_cfg.transpiler())?;

    let generated = generate();
    let num_corpuses = generated.len();

    for (corpus_id, blockchain_corpus) in generated.into_iter().enumerate() {
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
            let mut stdin = StdIn::default();
            stdin.write(client_input);
            stdin.write(&blockchain_corpus.network);

            let outputs = sdk.execute(exe.clone(), vm_cfg.clone(), stdin)?;
        }
    }

    Ok(())
}
