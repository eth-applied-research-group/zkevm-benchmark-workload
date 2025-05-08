# zkEVM-Pico: Ethereum Block Validation with Pico zkVM

This directory contains the implementation for benchmarking Ethereum block validation logic using the Pico zkVM.

## Pico zkVM

Pico is an open-source zero-knowledge virtual machine (zkVM) designed for building secure, scalable, and high-performance decentralized applications. It features a modular architecture that allows for interchangeable proving backends and the integration of app-specific circuits (coprocessors).

Key strengths of Pico include:

* **Modularity:** Composed of independent, interchangeable components.
* **Flexibility:** Supports various proving backends and custom proving pipelines.
* **Extensibility:** Allows integration of app-specific circuits and custom acceleration modules.
* **Performance:** Engineered for efficiency, aiming for industry-leading proof generation speeds.

For more details, refer to the [official Pico documentation](https://docs.brevis.network).

## Core Concepts

The benchmarking setup for `zkevm-pico` follows the standard guest/host program model:

1. **Guest Program (`pico-guest`):**
    * Contains the Rust code that performs the core Ethereum block validation (`reth_stateless::validation::stateless_validation`).
    * This code is compiled for the Pico zkVM's target architecture.
    * It reads block and witness data provided by the host program.

2. **Host Program (`pico-host`):**
    * A standard Rust binary that orchestrates the benchmarking process.
    * Uses `witness-generator` (from the workspace root) to obtain Ethereum test fixtures and generate the necessary input data for the guest.
    * Invokes the Pico zkVM SDK to execute the compiled guest program with the prepared inputs.
    * Collects performance metrics (e.g., cycle counts, proof generation time) reported by the Pico SDK.
    * Saves the benchmark results using the `metrics` crate into the `zkevm-metrics/pico/` directory.

## Prerequisites

1. **Rust Toolchain:** A standard Rust installation managed by `rustup`.
2. **Pico SDK:** The Pico SDK and any associated toolchains must be installed. Please refer to the [Pico Installation Guide](https://docs.brevis.network/getting-started/installation) for detailed setup instructions.
3. **Cloned Workspace:** Ensure you are within the `zkevm-benchmark-workload` repository.

## Setup & Running Benchmarks

1. **Install Pico SDK:** Follow the instructions at [Pico Installation Guide](https://docs.brevis.network/getting-started/installation).
2. **Compile the Guest Program:** Navigate to the `pico-guest` directory and compile the guest code.

    ```bash
    cd crates/zkevm-pico/pico-guest
    cargo pico build
    # The ELF file will be generated at ./program/elf/riscv32im-pico-zkvm-elf
    ```

3. **Navigate to the `pico-host` directory:**

    ```bash
    cd crates/zkevm-pico/pico-host
    ```

4. **Compile and Run the Benchmark:**
    The host program will need to be configured to find the compiled guest ELF (e.g., at `crates/zkevm-pico/pico-guest/program/elf/riscv32im-pico-zkvm-elf`) and the required input data (e.g., block/witness pairs from `witness-generator`).

    Execute the host program to run the benchmarks:

    ```bash
    cargo run --release
    ```

5. **Results:** Benchmark results will be saved in the `zkevm-metrics/pico/` directory in the workspace root.
