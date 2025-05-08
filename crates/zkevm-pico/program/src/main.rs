#![no_main]

pico_sdk::entrypoint!(main);
use pico_sdk::io::read_as;

extern crate alloc;

use alloc::sync::Arc;
use reth_stateless::{ClientInput, fork_spec::ForkSpec, validation::stateless_validation};

/// Entry point.
pub fn main() {
    println!("start read_input");
    let input: ClientInput = read_as();
    let network: ForkSpec = read_as();
    let chain_spec = Arc::new(network.into());
    println!("end read_input");

    println!("start validation");
    stateless_validation(input.block, input.witness, chain_spec).unwrap();
    println!("end validation");
}
