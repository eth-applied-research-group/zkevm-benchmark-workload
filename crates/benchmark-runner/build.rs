//! Build script that extracts resolved dependency metadata from `Cargo.lock`
//! and exposes it as compile-time environment variables.
//!
//! It resolves the ere-guests download source from the workspace lockfile.

use std::env;
use std::fs;

/// Repository URL used by Cargo for ere-guests git dependencies.
const ERE_GUESTS_REPO: &str = "https://github.com/eth-act/ere-guests";
/// Package used to resolve the ere-guests source for guest artifact downloads.
const ERE_GUESTS_DOWNLOAD_PACKAGE_NAME: &str = "stateless-validator-downloader";

enum GuestDownloadSource {
    Tag(String),
    Commit(String),
}

fn main() {
    let workspace_dir = env::var("CARGO_WORKSPACE_DIR").expect("CARGO_WORKSPACE_DIR not set");

    let lockfile_path = format!("{workspace_dir}/Cargo.lock");
    println!("cargo:rerun-if-changed={lockfile_path}");

    let lockfile_content = fs::read_to_string(&lockfile_path).expect("Failed to read Cargo.lock");

    let lockfile: toml::Value =
        toml::from_str(&lockfile_content).expect("Failed to parse Cargo.lock");

    let packages = lockfile
        .get("package")
        .and_then(|v| v.as_array())
        .expect("No package array in Cargo.lock");

    let guest_download_source = find_ere_guests_download_source(packages);

    match guest_download_source {
        Some(GuestDownloadSource::Tag(tag)) => {
            println!("cargo:rustc-env=ERE_GUESTS_DOWNLOAD_KIND=tag");
            println!("cargo:rustc-env=ERE_GUESTS_DOWNLOAD_VALUE={tag}");
        }
        Some(GuestDownloadSource::Commit(commit)) => {
            println!("cargo:rustc-env=ERE_GUESTS_DOWNLOAD_KIND=commit");
            println!("cargo:rustc-env=ERE_GUESTS_DOWNLOAD_VALUE={commit}");
        }
        None => {
            println!("cargo:warning=Could not determine ere-guests source from Cargo.lock");
            println!("cargo:rustc-env=ERE_GUESTS_DOWNLOAD_KIND=unknown");
            println!("cargo:rustc-env=ERE_GUESTS_DOWNLOAD_VALUE=unknown");
        }
    }
}

/// Extracts the git tag from a Cargo.lock source string.
///
/// Source format: `git+https://github.com/.../repo?tag=v1.10.2#8e3b5e6a...`
fn extract_tag_from_source(source: &str) -> Option<String> {
    extract_query_param_from_source(source, "tag")
}

fn find_ere_guests_download_source(packages: &[toml::Value]) -> Option<GuestDownloadSource> {
    let source = packages.iter().find_map(|package| {
        let name = package.get("name").and_then(|n| n.as_str())?;
        if name != ERE_GUESTS_DOWNLOAD_PACKAGE_NAME {
            return None;
        }

        let source = package.get("source").and_then(|s| s.as_str())?;
        source.contains(ERE_GUESTS_REPO).then_some(source)
    })?;

    extract_tag_from_source(source)
        .map(GuestDownloadSource::Tag)
        .or_else(|| {
            extract_hash_from_source(source)
                .map(|hash| GuestDownloadSource::Commit(hash.to_string()))
        })
}

fn extract_query_param_from_source(source: &str, param: &str) -> Option<String> {
    let query = source.split('?').nth(1)?;
    let query = query.split('#').next().unwrap_or(query);
    let prefix = format!("{param}=");

    query.split('&').find_map(|part| {
        part.strip_prefix(&prefix)
            .map(std::string::ToString::to_string)
    })
}

fn extract_hash_from_source(source: &str) -> Option<&str> {
    let hash = source.split('#').nth(1)?;
    (!hash.is_empty()).then_some(hash)
}
