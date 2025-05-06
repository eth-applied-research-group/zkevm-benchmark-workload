use openvm::io::{read, reveal_u32};

extern crate alloc;

use alloc::sync::Arc;
use reth_stateless::{ClientInput, fork_spec::ForkSpec, validation::stateless_validation};
use tracing_subscriber::fmt;

/// Entry point.
pub fn main() {
    println!("start read_input");
    let input: ClientInput = read();
    let network: ForkSpec = read();
    let chain_spec = Arc::new(network.into());
    println!("end read_input");

    println!("start validation");
    stateless_validation(input.block, input.witness, chain_spec).unwrap();
    println!("end validation");
}
