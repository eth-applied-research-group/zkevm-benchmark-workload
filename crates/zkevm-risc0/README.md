# zkevm-risc0

This crate benchmarks the execution of Ethereum block validation (or a similar zkEVM workload) using the RISC Zero zkVM.

## Overview

This project consists of two main components:

1. **Guest Program (`methods/guest/src/main.rs`):** A Rust program compiled to the RISC-V instruction set for execution within the RISC Zero zkVM. It is responsible for the core zkEVM logic, such as reading Ethereum block data and performing validation tasks.
2. **Host Program (`host/src/main.rs`):** A standard Rust binary that orchestrates the benchmarking process. It:
    * Prepares input data for the guest program, potentially using the `witness-generator` crate.
    * Invokes the RISC Zero zkVM to execute the compiled guest program.
    * Collects and optionally saves performance metrics (e.g., cycle counts, proof data) using the `zkevm-metrics` crate.

## Prerequisites

* **Rustup:** Ensure [rustup](https://rustup.rs) is installed. The `rust-toolchain.toml` file in the Risc0 project will be used by `cargo` to automatically install the correct Rust version.
* **RISC Zero Toolchain:** The necessary tools are typically managed by `cargo-risczero` and the build scripts. No separate installation is usually required beyond setting up Rust.
* **zkevm-fixtures:** This project uses the `witness-generator` crate, which relies on Ethereum test cases from the `zkevm-fixtures` directory. Ensure this submodule is initialized and populated. The workspace may contain a script to download `zkevm-fixtures`.

## Building and Running

### Building the Guest Method

The guest program (zkVM method) is compiled to its RISC-V ELF representation automatically when you build or run the host program. This is typically handled by the `build.rs` script within the `methods` directory.

### Running the Host

Navigate to the host program's directory and use `cargo run`:

```bash
cd crates/zkevm-risc0/host
cargo run
```

By default, this will **prove** the guest program within the zkVM locally.

#### Execution Modes (Environment Variables)

RISC Zero provides different execution modes, controlled via environment variables. This is a key difference compared to some other zkEVMs that might use explicit API calls (e.g., an `.execute()` method) for different modes.

* **Development Mode (Faster Iteration, No Proofs):**
    For faster local development and debugging without generating full proofs, use `RISC0_DEV_MODE=1`. You can also enable detailed logging:

    ```bash
    cd crates/zkevm-risc0/host
    RUST_LOG="[executor]=info" RISC0_DEV_MODE=1 cargo run
    ```

* **Standard Local Proving:**
    To generate a local proof, simply run without `RISC0_DEV_MODE`:

    ```bash
    cd crates/zkevm-risc0/host
    cargo run
    ```

## Input Data

The `host` program utilizes the `witness-generator` crate to prepare input for the guest. This generator typically sources Ethereum blockchain test cases from the `zkevm-fixtures` directory, converting them into the `ClientInput` (or similar structure) required by the guest for validation.

## Metrics Output

Performance metrics, such as execution cycle counts and proof details, are collected by the `host` program. These metrics are typically saved as JSON files using the `zkevm-metrics` crate in the `zkevm-metrics/risc0/` directory (or a similar path, please verify). Each file may correspond to a test corpus, containing detailed workload metrics.

**TODO**: Right now, Risc0 prints this to stdout so its a bit hard to collect metrics

## License

This crate inherits its license from the workspace. See the root `Cargo.toml` or `LICENSE` file in the main project directory.
