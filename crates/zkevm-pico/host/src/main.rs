use benchmark_runner::run_benchmark;
use pico_sdk::client::DefaultProverClient;
use witness_generator::BlocksAndWitnesses;
use zkevm_metrics::WorkloadMetrics;

/// Path to the compiled RISC-V ELF file for the `succinct-guest` crate.
///
/// This constant assumes the ELF has been built using `cargo prove build --release`
/// within the `crates/zkevm-succinct/succinct-guest` directory.
pub const STATELESS_ELF: &[u8] = include_bytes!(concat!(
    env!("CARGO_WORKSPACE_DIR"),
    "crates/zkevm-pico/program/elf/riscv32im-pico-zkvm-elf"
));

fn main() {
    let client = DefaultProverClient::new(&STATELESS_ELF);

    run_benchmark(
        STATELESS_ELF,
        "pico",
        |blockchain_corpus: &BlocksAndWitnesses, _elf_path: &'static [u8]| {
            let mut reports = Vec::new();
            let name = &blockchain_corpus.name;

            for client_input in &blockchain_corpus.blocks_and_witnesses {
                let block_number = client_input.block.number;

                let mut stdin = client.new_stdin_builder();
                stdin.write(client_input);
                stdin.write(&blockchain_corpus.network);

                let _proof = client.prove_fast(stdin).expect("Failed to generate proof");

                let metrics = WorkloadMetrics {
                    name: format!("{}-{}", name, block_number),
                    total_num_cycles: 0,
                    region_cycles: Default::default(),
                };
                reports.push(metrics);
            }
            reports
        },
    );
}
