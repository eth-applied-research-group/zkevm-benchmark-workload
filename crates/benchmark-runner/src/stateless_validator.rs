//! Stateless validator guest program.

mod eest;
mod fixtures;
mod inputs;

use crate::guest_programs::GuestFixture;
use anyhow::Result;
use stateless_validator_catalog::StatelessValidatorKind;
use std::path::Path;
use strum::{AsRefStr, EnumString};

pub use fixtures::{benchmark_fixture_paths, iter_benchmark_fixture_paths};

/// Execution client variants.
#[derive(Debug, Copy, Clone, PartialEq, Eq, EnumString, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum ExecutionClient {
    /// Reth stateless block validation guest program.
    Reth,
    /// Ethrex stateless block validation guest program.
    Ethrex,
    /// Zesu stateless block validation guest program.
    Zesu,
}

impl From<ExecutionClient> for StatelessValidatorKind {
    fn from(value: ExecutionClient) -> Self {
        match value {
            ExecutionClient::Reth => Self::Reth,
            ExecutionClient::Ethrex => Self::Ethrex,
            ExecutionClient::Zesu => Self::Zesu,
        }
    }
}

impl ExecutionClient {
    /// Returns the version string associated with the selected guest artifact.
    pub const fn version(&self) -> &'static str {
        match self {
            Self::Reth => StatelessValidatorKind::Reth.version(),
            Self::Ethrex => StatelessValidatorKind::Ethrex.version(),
            Self::Zesu => StatelessValidatorKind::Zesu.version(),
        }
    }
}

/// Lazily prepares stateless validator inputs from a fixture folder.
pub fn stateless_validator_input_iter(
    input_folder: &Path,
    selected_fixtures: Option<&[String]>,
    el: ExecutionClient,
    existing_output_dir: Option<&Path>,
) -> Result<impl Iterator<Item = Result<Box<dyn GuestFixture>>>> {
    fixtures::stateless_validator_input_iter(
        input_folder,
        selected_fixtures,
        el,
        existing_output_dir,
    )
}
