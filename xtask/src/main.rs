//! xtask – swap pre-compile patch-sets, then delegate to Cargo
//!
//! Build once:   cargo build -p xtask --release
//! Run directly: ./target/release/xtask succinct -- build -p succinct
//!
//! (or add aliases in .cargo/config.toml, see README)

use anyhow::{Context, Result, bail};
use clap::Parser;
use std::{collections::HashSet, fs, path::PathBuf, process::Command};
use toml_edit::{DocumentMut, Item, Table};

/// Inject one of the `precompile-patches/*.toml` files into the workspace
/// `Cargo.toml`, replacing previous precompile patches, then forward the rest
/// of the command-line to `cargo`.
#[derive(Parser)]
#[command(
    author,
    version,
    about = "Patch-set injector for the workspace",
    trailing_var_arg = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Patch-set name (file stem of precompile-patches/<name>.toml)
    patch: String,

    /// Everything after <patch> is passed straight to Cargo
    cargo_args: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // workspace-root/Cargo.toml  (xtask is at <root>/xtask)
    let ws_root = workspace_root()?;
    let manifest_path = ws_root.join("Cargo.toml");

    // 1 ── read root manifest
    let manifest_src = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let mut root_doc: DocumentMut = manifest_src.parse()?;

    // 2 ── remove only the keys we “own”, ie the keys in the precompile-patches
    let precompile_dir = ws_root.join("precompile-patches");
    let owned_keys = gather_owned_keys(&precompile_dir)?;
    {
        let ci = root_doc["patch"]["crates-io"]
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .unwrap();
        for key in &owned_keys {
            ci.remove(key);
        }
    }

    // 3 ── insert the selected patch-set
    let chosen_file = precompile_dir.join(format!("{}.toml", cli.patch));
    if !chosen_file.exists() {
        bail!(
            "unknown patch-set `{}` (available: {})",
            cli.patch,
            display_patch_sets(&precompile_dir)?
        );
    }
    let chosen_src = fs::read_to_string(&chosen_file)?;
    let chosen_doc: DocumentMut = chosen_src.parse()?;
    if let Some(tbl) = chosen_doc
        .get("patch")
        .and_then(|p| p.get("crates-io"))
        .and_then(Item::as_table)
    {
        let ci = root_doc["patch"]["crates-io"].as_table_mut().unwrap();
        for (k, v) in tbl {
            ci[k] = v.clone(); // overwrite / insert
        }
    } else {
        bail!("{} has no [patch.crates-io] table", chosen_file.display());
    }

    // 4 ── write the updated manifest back
    fs::write(&manifest_path, root_doc.to_string())
        .with_context(|| format!("writing {}", manifest_path.display()))?;

    // 5 ── forward to Cargo
    let status = Command::new("cargo")
        .args(&cli.cargo_args)
        .status()
        .context("failed to invoke cargo")?;
    std::process::exit(status.code().unwrap_or(1));
}

/// repo root (assumes xtask lives in <root>/xtask)
fn workspace_root() -> Result<PathBuf> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    Ok(p)
}

/// All crate-keys that occur in any precompile-patches/*.toml
fn gather_owned_keys(dir: &PathBuf) -> Result<HashSet<String>> {
    let mut keys = HashSet::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let src = fs::read_to_string(&path)?;
        let doc: DocumentMut = src.parse()?;
        if let Some(tbl) = doc
            .get("patch")
            .and_then(|p| p.get("crates-io"))
            .and_then(Item::as_table)
        {
            for (k, _) in tbl {
                keys.insert(k.to_string());
            }
        }
    }
    Ok(keys)
}

/// Pretty-print available patch-set names
fn display_patch_sets(dir: &PathBuf) -> Result<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names.join(", "))
}
