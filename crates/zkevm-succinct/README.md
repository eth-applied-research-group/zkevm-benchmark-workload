# zkevm-succinct

This crate benchmarks the execution of Ethereum block validation within the Succinct SP1 zkVM.

## Overview

This setup consists of two main components:

1. **`succinct-guest` (`succinct-guest`):** A Rust program compiled to RISC-V ELF for execution within the SP1 zkVM. It reads an Ethereum block and its execution witness (`ClientInput`) along with network rules (`ForkSpec`), and performs stateless validation using `reth_stateless::validation::stateless_validation`.
2. **`succinct-host` (`succinct-host`):** A standard Rust binary that orchestrates the benchmarking process/execution and potentially proving of the RISC-V ELD. It:
    * Generates test cases (block/witness pairs) using the `witness-generator` crate.
    * For each test case block, invokes the SP1 zkVM to execute the compiled `succinct-guest` ELF with the corresponding `ClientInput` and `ForkSpec`.
    * Collects cycle count metrics (total and per-region, using SP1's cycle tracking) for the zkVM execution.
    * Saves these metrics using the `metrics` crate to JSON files located in the `zkevm-metrics/succinct/` directory.

## Prerequisites

* **Install SP1 Toolchain:** Follow the [official SP1 installation guide](https://docs.succinct.xyz/docs/sp1/getting-started/install).
* **zkevm-fixtures:** Ensure the `zkevm-fixtures` submodule is initialized and contains the necessary Ethereum test cases. The `witness-generator` crate relies on this. This workspace contains a script that download the `zkevm-fixtures`.

## Usage

1. **Compile the zkVM Program:**

    ```bash
    cd crates/zkevm-succinct/succinct-guest
    cargo prove build
    ```

    This compiles `succinct-guest/src/main.rs` to RISC-V ELF and places it in the `target/elf-compilation/` directory within the workspace root.

2. **Run the Host Benchmarker:**

    ```bash
    cd crates/zkevm-succinct/succinct-host
    # use RUST_LOG=info if you want detailed logs
    cargo run --release
    ```

    The host will:
    * Generate test data.
    * Execute each test block within the SP1 zkVM.
    * Generate JSON metric files in `zkevm-metrics/succinct/`.

## Input Data

The `succinct-host` uses the `witness-generator` crate, which reads Ethereum blockchain test cases from the `zkevm-fixtures` directory to generate the necessary `ClientInput` (block + witness) data required by the `succinct-guest`.

## Metrics Output

Benchmark results are stored as JSON files in `zkevm-metrics/succinct/`, with each file corresponding to a test corpus (e.g., `ModExpAttackContract.json`). Each file contains a list of `WorkloadMetrics` objects (one per block in the corpus), detailing total cycles and cycles spent in specific code regions defined in the `succinct-guest` (like `read_input` and `validation`).

## License

This crate inherits its license from the workspace. See the root `Cargo.toml` or `LICENSE` file.
