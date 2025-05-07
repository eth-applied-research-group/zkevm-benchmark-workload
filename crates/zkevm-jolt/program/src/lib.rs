#![cfg_attr(feature = "guest", no_std)]

extern crate alloc;

use alloc::sync::Arc;
use reth_stateless::{fork_spec::ForkSpec, validation::stateless_validation, ClientInput};

// Example program
#[jolt::provable]
fn fib(n: u32) -> u128 {
    let mut a: u128 = 0;
    let mut b: u128 = 1;
    let mut sum: u128;
    for _ in 1..n {
        sum = a + b;
        a = b;
        b = sum;
    }

    b
}

#[jolt::provable]
fn validate_block(input: ClientInput, network: ForkSpec) {
    let chain_spec = Arc::new(network.into());
    stateless_validation(input.block, input.witness, chain_spec).unwrap();
}
