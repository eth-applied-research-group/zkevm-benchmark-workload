<p align="center">
  <img src="assets/logo-white-transparent-bg.png" alt="ZK-EVM Bench" width="300"/>
</p>

# ZK-EVM Benchmark Workload

This workspace contains code for executing Ethereum block validation logic within different Zero-Knowledge Virtual Machines (zkVMs).

## Goal

The primary goal is to measure and compare the performance (currently in cycle counts) of running standardized Ethereum stateless validation logic across various zkVM platforms.

## Workspace Structure

The workspace is organized into several key components:

- **`crates/metrics`**: Defines common data structures (`WorkloadMetrics`) for storing and serializing benchmark results.
- **`crates/witness-generator`**: Generates the necessary inputs (`ClientInput`: block + witness pairs) required for stateless block validation by processing standard Ethereum test fixtures.
- **zkVM Implementations (`crates/zkevm-*`)**: Directories prefixed with `zkevm-` (e.g., `crates/zkevm-succinct`, `crates/zkevm-zkm`) contain the benchmark implementations for specific zkVM platforms. Each typically includes distinct 'guest' and 'host' sub-crates.
- **`zkevm-fixtures`**: (Git submodule) Contains the Ethereum execution layer test fixtures used by `witness-generator`.
- **`zkevm-metrics`**: Directory where benchmark results (cycle counts) are stored by the host programs, organized by zkVM type.
- **`scripts`**: Contains helper scripts (e.g., fetching fixtures).
- **`xtask`**: Cargo xtask runner for automating tasks.

## Core Concepts

Each zkVM benchmark implementation follows a common pattern:

1. **Guest Program:**
    - Located within the specific zkVM crate (e.g., `crates/zkevm-succinct/succinct-guest`).
    - Contains the Rust code that performs the core Ethereum block validation (`reth_stateless::validation::stateless_validation`).
    - This code is compiled specifically for the target zkVM's architecture (e.g., RISC-V for SP1, MIPS for zkMIPS).
    - It reads block/witness data from its zkVM environment's standard input.
    - Uses platform-specific mechanisms (often `println!` markers) to delineate code regions for cycle counting.

2. **Host Program:**
    - Located within the specific zkVM crate (e.g., `crates/zkevm-succinct/succinct-host`).
    - A standard Rust binary that orchestrates the benchmarking.
    - Uses `witness-generator` to get test data and generate input data.
    - Invokes the corresponding zkVM SDK to execute the compiled Guest program ELF with the necessary inputs.
    - Collects cycle count metrics reported by the zkVM SDK.
    - Saves the results using the `metrics` crate into the appropriate subdirectory within `zkevm-metrics/`.

## Prerequisites

1. **Rust Toolchain:** A standard Rust installation managed by `rustup`.
2. **zkVM-Specific Toolchains:** Each zkVM requires its own SDK and potentially a custom Rust toolchain/target. Please refer to the `README.md` within the specific `crates/zkevm-*` directory (e.g., `crates/zkevm-succinct/README.md`) for detailed setup instructions for that platform.
3. **Git:** Required for cloning the repository :)
4. **Common Shell Utilities:** The scripts in the `./scripts` directory require a `bash`-compatible shell and standard utilities like `curl`, `jq`, and `tar`.

## Setup

1. **Clone the Repository:**

    ```bash
    git clone <repository-url>
    cd zkevm-benchmark-workload
    ```

2. **Fetch/Update Benchmark Fixtures:**

    ```bash
    ./scripts/download-and-extract-fixtures.sh
    ```

## Supported zkVM Benchmarks

| zkVM Platform        | Crate Path                | Guest Crate    | Host Crate    | Metrics Output         |
| -------------------- | ------------------------- | -------------- | ------------- | ---------------------- |
| **Succinct SP1**     | `crates/zkevm-succinct` | `succinct-guest` | `succinct-host` | `zkevm-metrics/succinct/` |
| **zkMIPS**           | `crates/zkevm-zkm`      | `zkm-guest`    | `zkm-host`    | `zkevm-metrics/zkm/`
