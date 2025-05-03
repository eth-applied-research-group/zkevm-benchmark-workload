#![no_main]
#![doc = include_str!("../../README.md")]

extern crate alloc;

use alloc::sync::Arc;
use reth_stateless::{ClientInput, fork_spec::ForkSpec, validation::stateless_validation};
use tracing_subscriber::fmt;

sp1_zkvm::entrypoint!(main);
/// Entry point.
pub fn main() {
    init_tracing_just_like_println();

    println!("cycle-tracker-report-start: read_input");
    let input = sp1_zkvm::io::read::<ClientInput>();
    let network = sp1_zkvm::io::read::<ForkSpec>();
    let chain_spec = Arc::new(network.into());
    println!("cycle-tracker-report-end: read_input");

    println!("cycle-tracker-report-start: validation");
    stateless_validation(input.block, input.witness, chain_spec).unwrap();
    println!("cycle-tracker-report-end: validation");
}

/// TODO: can we put this in the host? (Note that if we want sp1 logs, it will look very plain in that case)
/// Initializes a basic `tracing` subscriber that mimics `println!` behavior.
///
/// This is because we want to use tracing in the no_std program to capture cycle counts.
fn init_tracing_just_like_println() {
    // Build a formatter that prints *only* the message text + '\n'
    let plain = fmt::format()
        .without_time() // no timestamp
        .with_level(false) // no INFO/TRACE prefix
        .with_target(false); // no module path

    fmt::Subscriber::builder()
        .event_format(plain) // use the stripped-down format
        .with_writer(std::io::stdout) // stdout == println!
        .with_max_level(tracing::Level::INFO) // capture info! and up
        .init(); // set as global default
}
