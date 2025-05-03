# metrics

This crate provides data structures and utilities for handling workload performance metrics, specifically cycle counts.

## Overview

The core data structure is `WorkloadMetrics`, which stores:

- `name`: The name of the workload (e.g., "fft", "aes"). -- This is usually linked to the inputs that you supply to the guest program. For example,
   if you supply odd numbers to a guest program that adds numbers together, you might name the workload "odd_numbers_add"
- `total_num_cycles`: The total cycle count for the whole execution.
- `region_cycles`: A map associating names (e.g., "setup", "compute") with the cycle counts for specific regions within the workload.

The crate offers functionality to:

- Serialize a list of `WorkloadMetrics` to a JSON string.
- Deserialize a list of `WorkloadMetrics` from a JSON string.
- Serialize and write a list of `WorkloadMetrics` to a file (creating parent directories if needed).
- Read and deserialize a list of `WorkloadMetrics` from a file.

## Usage

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
metrics = { path = "../metrics" } # Adjust path as needed
```

Example:

```rust
use metrics::WorkloadMetrics;
use std::collections::HashMap;
use std::iter::FromIterator;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metrics_data = vec![
        WorkloadMetrics {
            name: "workload name".into(),
            total_num_cycles: 1_000,
            region_cycles: HashMap::from_iter([
                ("setup".to_string(), 100),
                ("compute".to_string(), 800),
                ("teardown".to_string(), 100),
            ]),
        },
        // ... other workloads
    ];

    // Serialize to JSON string
    let json_string = WorkloadMetrics::to_json(&metrics_data)?;
    println!("Serialized JSON: {}", json_string);

    // Write to file
    let output_path = "metrics_output.json";
    WorkloadMetrics::to_path(output_path, &metrics_data)?;
    println!("Metrics written to {}", output_path);

    // Read from file
    let read_metrics = WorkloadMetrics::from_path(output_path)?;
    assert_eq!(metrics_data, read_metrics);
    println!("Successfully read metrics back from file.");

    Ok(())
}

```

## Error Handling

Functions return `Result<_, MetricsError>`.

## License

This crate inherits its license from the workspace. See the root `Cargo.toml` or `LICENSE` file.
