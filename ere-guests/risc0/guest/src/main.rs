use risc0_zkvm::guest::env;

extern crate alloc;

use alloc::sync::Arc;
use alloy_genesis::Genesis;
use reth_stateless::{validation::stateless_validation, ClientInput};

/// Entry point.
pub fn main() {
    println!("start reading input");
    let start = env::cycle_count();
    let input = env::read::<ClientInput>();
    let genesis = env::read::<Genesis>();
    let chain_spec = Arc::new(genesis.into());
    let end = env::cycle_count();
    eprintln!("reading input (cycle tracker): {}", end - start);

    println!("start stateless validation");
    let start = env::cycle_count();
    stateless_validation(input.block, input.witness, chain_spec).unwrap();
    let end = env::cycle_count();
    eprintln!("stateless validation (cycle tracker): {}", end - start);
}
