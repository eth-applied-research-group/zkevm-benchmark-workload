# zkevm-openvm

This crate benchmarks the execution of Ethereum block validation within the OpenVM zkVM.

## Overview

This setup consists of two main components:

1. **`openvm-guest` (guest program):** A Rust program compiled to the OpenVM target for execution within the OpenVM zkVM. It reads an Ethereum block and its execution witness (`ClientInput`) along with network rules (`ForkSpec`), and performs stateless validation using `reth_stateless::validation::stateless_validation`.
2. **`openvm-host` (host program):** A standard Rust binary that orchestrates the benchmarking process. It:
    * Generates test cases (block/witness pairs) using the `witness-generator` crate.
    * For each test case block, invokes the OpenVM SDK to execute the compiled `openvm-guest` program with the corresponding `ClientInput` and `ForkSpec`.
    * Collects cycle count metrics (total and per-region, using OpenVM's cycle tracking mechanisms) for the zkVM execution.
    * Saves these metrics using the `metrics` crate to JSON files located in the `zkevm-metrics/openvm/` directory.

## Prerequisites

* **Install OpenVM Toolchain:** Follow the [official OpenVM installation guide](https://book.openvm.dev/getting-started/installing).
* **zkevm-fixtures:** Ensure the `zkevm-fixtures` submodule is initialized and contains the necessary Ethereum test cases. The `witness-generator` crate relies on this. This workspace contains a script that downloads the `zkevm-fixtures`.

## Usage

2. **Run the Host**

    ```bash
    # Navigate to the host program directory
    cd crates/zkevm-openvm/host
    cargo run --release
    ```

    The host will:
    * Compile the guest program
    * Generate test data.
    * Execute each test block within the OpenVM zkVM.
    * Generate JSON metric files in `zkevm-metrics/openvm/`.

## Input Data

The `openvm-host` uses the `witness-generator` crate, which reads Ethereum blockchain test cases from the `zkevm-fixtures` directory to generate the necessary `ClientInput` (block + witness) data required by the `openvm-guest`.

## License

This crate inherits its license from the workspace. See the root `Cargo.toml` or `LICENSE` file.
