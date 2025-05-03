#![doc = include_str!("../README.md")]

mod blocks_and_witnesses;
/// generate the execution witnesses for `zkevm-fixtures`
pub mod generate_stateless_witness;

pub use blocks_and_witnesses::{BlocksAndWitnesses, BwError, ClientInput, ForkSpec};
