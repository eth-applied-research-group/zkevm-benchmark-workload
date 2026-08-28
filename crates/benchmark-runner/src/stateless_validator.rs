//! Stateless validator guest program.

mod eest;
mod fixtures;
mod inputs;

use crate::guest_programs::GuestFixture;
use anyhow::{bail, Context, Result};
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
        let kind = match self {
            Self::Reth => StatelessValidatorKind::Reth,
            Self::Ethrex => StatelessValidatorKind::Ethrex,
            Self::Zesu => StatelessValidatorKind::Zesu,
        };
        if kind.version().is_none() {
            bail!(
                "{} is temporarily unsupported because ere-guests has no active tests-zkevm v0.8.2 artifacts",
                self.as_ref()
            );
        }
        Ok(kind)
    }

    /// Returns the version string associated with the selected guest artifact.
    pub fn version(self) -> Result<&'static str> {
        self.registered_kind()?
            .version()
            .context("active upstream guest is missing a version")
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
    fn active_v08_guests_follow_upstream_catalog() {
        assert_eq!(
            ExecutionClient::Reth.registered_kind().unwrap(),
            StatelessValidatorKind::Reth
        );
        assert_eq!(
            ExecutionClient::Ethrex.registered_kind().unwrap(),
            StatelessValidatorKind::Ethrex
        );
        assert_eq!(ExecutionClient::Reth.version().unwrap(), "0.1.0-rc.2");
        assert_eq!(ExecutionClient::Ethrex.version().unwrap(), "26.0.0-rc.2");

        let expected_zesu_error = "Zesu is temporarily unsupported because ere-guests has no active tests-zkevm v0.8.2 artifacts";
        assert_eq!(
            ExecutionClient::Zesu
                .registered_kind()
                .unwrap_err()
                .to_string(),
            expected_zesu_error
        );
        assert_eq!(
            ExecutionClient::Zesu.version().unwrap_err().to_string(),
            expected_zesu_error
        );
    }
}
