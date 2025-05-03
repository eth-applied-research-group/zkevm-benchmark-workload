#![doc = include_str!("../README.md")]

use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::Path};
use thiserror::Error;

/// Cycle-count metrics for a particular workload.
///
/// Stores the total cycle count and a breakdown of cycle count per named region.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkloadMetrics {
    /// Name of the workload (e.g., "fft", "aes").
    pub name: String,
    /// Total number of cycles for the entire workload execution.
    pub total_num_cycles: u64,
    /// Region-specific cycles, mapping region names (e.g., "setup", "compute") to their cycle counts.
    pub region_cycles: HashMap<String, u64>,
}

/// Errors that can occur during metrics processing.
#[derive(Error, Debug)]
pub enum MetricsError {
    /// Error during JSON serialization or deserialization.
    #[error("serde (de)serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// Error during file system I/O operations.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

impl MetricsError {
    #[cfg(test)]
    fn into_serde_err(self) -> serde_json::Error {
        match self {
            MetricsError::Serde(e) => e,
            MetricsError::Io(e) => panic!("unexpected IO error in test: {e}"),
        }
    }
}

impl WorkloadMetrics {
    /// Serializes a list of `WorkloadMetrics` into a JSON string.
    ///
    /// # Errors
    ///
    /// Returns `MetricsError::Serde` if serialization fails.
    pub fn to_json(items: &[WorkloadMetrics]) -> Result<String, MetricsError> {
        serde_json::to_string(items).map_err(MetricsError::from)
    }

    /// Deserializes a list of `WorkloadMetrics` from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns `MetricsError::Serde` if deserialization fails.
    pub fn from_json(json: &str) -> Result<Vec<WorkloadMetrics>, MetricsError> {
        serde_json::from_str(json).map_err(MetricsError::from)
    }

    /// Serializes `items` using JSON pretty-print and writes them to `path` atomically.
    ///
    /// The file is created if it does not exist and truncated if it does.
    /// Parent directories are created if they are missing.
    ///
    /// # Errors
    ///
    /// Returns `MetricsError::Io` if any filesystem operation fails.
    /// Returns `MetricsError::Serde` if JSON serialization fails.
    pub fn to_path<P: AsRef<Path>>(path: P, items: &[WorkloadMetrics]) -> Result<(), MetricsError> {
        let path = path.as_ref();

        if let Some(parent) = path.parent() {
            // `create_dir_all` is a no-op when the dirs are already there.
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(items)?;
        fs::write(path, json)?;

        Ok(())
    }

    /// Reads the file at `path` and deserializes a `Vec<WorkloadMetrics>` from its JSON content.
    ///
    /// # Errors
    ///
    /// Returns `MetricsError::Io` if reading the file fails.
    /// Returns `MetricsError::Serde` if JSON deserialization fails.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Vec<WorkloadMetrics>, MetricsError> {
        let contents = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&contents)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::FromIterator;
    use tempfile::NamedTempFile;

    // This is just a fixed sample we are using to test serde_roundtrip
    fn sample() -> Vec<WorkloadMetrics> {
        vec![
            WorkloadMetrics {
                name: "fft".into(),
                total_num_cycles: 1_000,
                region_cycles: HashMap::from_iter([
                    ("setup".to_string(), 100),
                    ("compute".to_string(), 800),
                    ("teardown".to_string(), 100),
                ]),
            },
            WorkloadMetrics {
                name: "aes".into(),
                total_num_cycles: 2_000,
                region_cycles: HashMap::from_iter([
                    ("init".to_string(), 200),
                    ("encrypt".to_string(), 1_600),
                    ("final".to_string(), 200),
                ]),
            },
        ]
    }

    #[test]
    fn round_trip_json() {
        let workloads = sample();
        let json = WorkloadMetrics::to_json(&workloads).expect("serialize");
        let parsed = WorkloadMetrics::from_json(&json).expect("deserialize");
        assert_eq!(workloads, parsed);
    }

    #[test]
    fn bad_json_is_error() {
        let bad = "{this is not valid json}";
        let err = WorkloadMetrics::from_json(bad).unwrap_err();
        assert!(err.into_serde_err().is_data());
    }

    #[test]
    fn file_round_trip() -> Result<(), MetricsError> {
        // Create a named temporary file.
        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path();

        let workloads = sample();

        // Write → read → compare using the temp file's path.
        WorkloadMetrics::to_path(path, &workloads)?;
        let read_back = WorkloadMetrics::from_path(path)?;
        assert_eq!(workloads, read_back);

        Ok(())
    }
}
