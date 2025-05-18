use openvm::io::read;

extern crate alloc;

use alloc::sync::Arc;
use alloy_genesis::Genesis;
use reth_stateless::{ClientInput, validation::stateless_validation};
use tracing_subscriber::fmt;

/// Entry point.
pub fn main() {
    println!("start read_input");
    let input: ClientInput = read();
    let genesis: Genesis = read();
    let chain_spec = Arc::new(genesis.into());
    println!("end read_input");

    println!("start validation");
    stateless_validation(input.block, input.witness, chain_spec).unwrap();
    println!("end validation");
}
