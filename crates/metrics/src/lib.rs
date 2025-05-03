use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::Path};
use thiserror::Error;

/// Cycle-count metrics for a particular workload.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkloadMetrics {
    /// Name of the workload.
    pub name: String,
    /// Total number of cycles.
    pub total_num_cycles: u64,
    /// Region-specific cycles.
    pub region_cycles: HashMap<String, u64>,
}

#[derive(Error, Debug)]
pub enum MetricsError {
    /// Serde de/serialization error.
    #[error("serde de/serialization error")]
    Serde(#[from] serde_json::Error),

    /// Any std-io failure while touching the filesystem.
    #[error("I/O error")]
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
    pub fn to_json(items: &[WorkloadMetrics]) -> Result<String, MetricsError> {
        serde_json::to_string(items).map_err(MetricsError::from)
    }

    pub fn from_json(json: &str) -> Result<Vec<WorkloadMetrics>, MetricsError> {
        serde_json::from_str(json).map_err(MetricsError::from)
    }

    /// Serialise `items` and write them to `path` atomically.
    ///
    /// The file is created if it does not exist and truncated if it does.
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

    /// Read the file at `path` and deserialise a `Vec<WorkloadMetrics>`.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Vec<WorkloadMetrics>, MetricsError> {
        let contents = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&contents)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::FromIterator;

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
        // Put the temp file in the system temp directory.
        let mut p: PathBuf = std::env::temp_dir();
        p.push("workload_metrics_test.json");

        // Clean up even if the test panics.
        struct Guard(PathBuf);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let _g = Guard(p.clone());

        let workloads = sample();

        // Write → read → compare.
        WorkloadMetrics::to_path(&p, &workloads)?;
        let read_back = WorkloadMetrics::from_path(&p)?;
        assert_eq!(workloads, read_back);
        Ok(())
    }
}
