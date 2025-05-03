# zkevm-zkm

This crate benchmarks the execution of Ethereum block validation within the zkMIPS zkVM.

## Overview

This setup consists of two main components, similar to the `zkevm-succinct` crate but targeting the zkMIPS platform:

1. **`zkm-guest` (`zkm-guest`):** A Rust program compiled to MIPS ELF for execution within the zkMIPS zkVM. It reads an Ethereum block and its execution witness (`ClientInput`) along with network rules (`ForkSpec`), and performs stateless validation using `reth_stateless::validation::stateless_validation`.
2. **`zkm-host` (`zkm-host`):** A standard Rust binary that orchestrates the benchmarking process. It:
    * Generates test cases (block/witness pairs) using the `witness-generator` crate.
    * For each test case block, invokes the zkMIPS executor (`zkm-sdk`) to run the compiled `zkm-guest` ELF with the corresponding `ClientInput` and `ForkSpec`.
    * Collects cycle count metrics (total and per-region, using the zkMIPS SDK's cycle tracking) for the execution.
    * Saves these metrics using the `metrics` crate to JSON files located in the `zkevm-metrics/zkm/` directory.

## Prerequisites

* **Install zkMIPS Toolchain:** Follow the instructions in the [zkMIPS repository](https://github.com/zkMIPS/zkMIPS) to set up the necessary build environment and toolchain.
* **zkevm-fixtures:** Ensure the `zkevm-fixtures` submodule is initialized and contains the necessary Ethereum test cases. The `witness-generator` crate relies on this. This workspace contains a script to download `zkevm-fixtures`.

## Usage

1. **Compile the zkVM Program:**

    ```bash
    # TODO: insert the command to do this. Right now we compile the guest program whenever the host is built via build.rs
    ```

    The guest program is compiled to MIPS ELF and placed in the `target/mipsel-zkm-zkvm-elf/release/` directory within the workspace root.

2. **Run the Host Benchmarker:**

    ```bash
    cd crates/zkevm-zkm/zkm-host
    # use RUST_LOG=info if you want detailed logs
    cargo run --release
    ```

    The host will:
    * Generate test data.
    * Execute each test block using the zkMIPS executor.
    * Generate JSON metric files in `zkevm-metrics/zkm/`.

## Input Data

The `zkm-host` uses the `witness-generator` crate, which reads Ethereum blockchain test cases from the `zkevm-fixtures` directory to generate the necessary `ClientInput` (block + witness) data required by the `zkm-guest`.

## Metrics Output

Benchmark results are stored as JSON files in `zkevm-metrics/zkm/`, with each file corresponding to a test corpus (e.g., `ModExpAttackContract.json`). Each file contains a list of `WorkloadMetrics` objects (one per block in the corpus), detailing total cycles and cycles spent in specific code regions defined in the `zkm-guest` (like `read_input` and `validation`).

## License

This crate inherits its license from the workspace. See the root `Cargo.toml` or `LICENSE` file.
