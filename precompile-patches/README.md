# Precompile Patches

This directory contains patches for dependencies used within the zkVM guest programs (e.g., `succinct-guest`, `zkm-guest`).

## Purpose

Certain dependencies, particularly cryptographic libraries or those interacting heavily with low-level primitives, may require modifications to be:

1. **Compatible:** Ensure they work correctly within the specific constraints and environment of a zkVM. (Usually this means `no_std`)
2. **Efficient:** Optimize their performance when executed inside the zkVM, as standard implementations might be unnecessarily costly in a ZK context. (Usually this is done by having a circuit implementation of the algorithm and exposing that as a precompile that the patches will call)

These patches apply the necessary modifications directly to the source code of the dependencies before the guest programs are compiled.

## Structure

Patch configurations are defined in TOML files, named after the corresponding zkVM platform:

- `succinct.toml`: Defines patches applied when building for the Succinct SP1 platform.
- `zkm.toml`: Defines patches applied when building for the zkMIPS platform.

These TOML files typically specify which crates need patching and point to the directories containing the modified source located elsewhere.

## Application

The application of these patches is automated via the workspace's `xtask` runner.

As mentioned in the main `README.md`, running `cargo <zkvm-name>` (e.g., `cargo succinct`, `cargo zkm`) will trigger the `xtask` for that specific zkVM. This task reads the corresponding `.toml` file (`succinct.toml` or `zkm.toml`) and applies the specified patches to the relevant dependencies within the `[patch.crates-io]` section of the workspace `Cargo.toml`.

Since the `xtask` integrates with cargo, you can chain standard cargo commands after the zkVM name. For instance, to ensure patches are applied (if needed by the xtask) and then build the corresponding host program, you could run:

```bash
# Example for Succinct SP1 host
cargo succinct build --release -p succinct-host 

# Example for zkMIPS host
cargo zkm build --release -p zkm-host
```

**Note:** Manually applying these patches is generally not required, as the `xtask` handles the process. But one could manually modify the workspace Cargo.toml and it would have the same effect.
