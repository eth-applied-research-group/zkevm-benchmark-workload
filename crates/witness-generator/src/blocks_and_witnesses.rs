use std::{fs, io, path::Path};

pub use reth_stateless::{ClientInput, fork_spec::ForkSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize)]
/// You can think of a test case as a mini-blockchain
pub struct BlocksAndWitnesses {
    /// Name of the blockchain test
    pub name: String,
    /// Block coupled with its corresponding execution witness
    pub blocks_and_witnesses: Vec<ClientInput>,
    /// Chain specification
    // TODO: Don't think we want to pass this through maybe ForkSpec
    // TODO: Also Genesis file is wrong in chainspec
    // TODO: We can keep this initially and don't measure the time it takes to deserialize
    pub network: ForkSpec,
}

#[derive(Error, Debug)]
pub enum BwError {
    /// Serde (de)serialisation error.
    #[error("serde de/serialization error")]
    Serde(#[from] serde_json::Error),

    /// Any failure while touching the filesystem.
    #[error("I/O error")]
    Io(#[from] io::Error),
}

impl BlocksAndWitnesses {
    /// Serialise *multiple* test-cases to a JSON string (compact).
    pub fn to_json(items: &[BlocksAndWitnesses]) -> Result<String, BwError> {
        serde_json::to_string(items).map_err(BwError::from)
    }

    /// Parse the JSON representation produced by [`to_json`].
    pub fn from_json(json: &str) -> Result<Vec<BlocksAndWitnesses>, BwError> {
        serde_json::from_str(json).map_err(BwError::from)
    }

    /// Pretty-print to `path` (truncates or creates the file).
    pub fn to_path<P: AsRef<Path>>(path: P, items: &[BlocksAndWitnesses]) -> Result<(), BwError> {
        let json = serde_json::to_string_pretty(items)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Read `path` and deserialise.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Vec<BlocksAndWitnesses>, BwError> {
        let contents = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&contents)?)
    }
}
