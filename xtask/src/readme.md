# xtask – Workspace Patch Manager

This is a helper CLI for managing `[patch.crates-io]` entries in the workspace `Cargo.toml`.

## Usage

```bash
cargo run -p xtask -- <patch-set> -- <cargo subcommand and args>

# Examples:
cargo run -p xtask -- succinct -- build -p succinct
cargo run -p xtask -- zkm      -- test  -p zkm_guest

## Alias

Note that we have also added in aliases in .cargo/config.toml, so `cargo run -p xtask -- succinct --` is compressed to `cargo succinct`
