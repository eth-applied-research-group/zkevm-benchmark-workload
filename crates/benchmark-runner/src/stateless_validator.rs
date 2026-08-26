//! Stateless validator guest program.

mod eest;
mod fixtures;
mod inputs;

use crate::guest_programs::GuestFixture;
use anyhow::{bail, Result};
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

impl ExecutionClient {
    /// Returns the active upstream guest kind for this execution client.
    pub fn registered_kind(self) -> Result<StatelessValidatorKind> {
        match self {
            Self::Reth => Ok(StatelessValidatorKind::Reth),
            // TODO(ethrex-release): Restore this mapping when ere-guests publishes a compatible
            // tests-zkevm v0.8.2 Ethrex artifact.
            Self::Ethrex => bail!(
                "Ethrex is temporarily unsupported with tests-zkevm v0.8.2; use \
                 --execution-client reth until ere-guests restores the Ethrex registry entry"
            ),
            // TODO(zesu-devnet-8): Restore this mapping when ere-guests publishes a compatible
            // Glamsterdam devnet-8 Zesu artifact.
            Self::Zesu => bail!(
                "Zesu is temporarily unsupported on Glamsterdam devnet-8; use \
                 --execution-client reth until ere-guests restores the Zesu registry entry"
            ),
        }
    }

    /// Returns the version string associated with the selected guest artifact.
    pub fn version(self) -> Result<&'static str> {
        Ok(self.registered_kind()?.version())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reth_has_an_active_v08_guest() {
        assert_eq!(
            ExecutionClient::Reth.registered_kind().unwrap(),
            StatelessValidatorKind::Reth
        );
        assert_eq!(
            ExecutionClient::Ethrex
                .registered_kind()
                .unwrap_err()
                .to_string(),
            "Ethrex is temporarily unsupported with tests-zkevm v0.8.2; use \
             --execution-client reth until ere-guests restores the Ethrex registry entry"
        );
        assert_eq!(
            ExecutionClient::Zesu
                .registered_kind()
                .unwrap_err()
                .to_string(),
            "Zesu is temporarily unsupported on Glamsterdam devnet-8; use \
             --execution-client reth until ere-guests restores the Zesu registry entry"
        );
    }
}
