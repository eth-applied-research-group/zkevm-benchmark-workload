use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use tar::Archive;
use tracing::{info, warn};

use crate::{
    artifact::{
        self, BATCH_MANIFEST_PATH, BATCH_MANIFEST_SCHEMA_VERSION, path_to_slash_string,
        write_bytes_atomic,
    },
    config::CollectorConfig,
    export::ExportedBatchMetadata,
};

const CATALOG_SCHEMA_VERSION: u64 = 2;
const CATALOG_KIND: &str = "stateless-inputs-public-catalog";
const CATALOG_CACHE_SCHEMA_VERSION: u64 = 1;
const CATALOG_CACHE_KIND: &str = "stateless-inputs-catalog-cache";
const HTML_INDEX: &str = "index.html";
const PUBLIC_MANIFEST: &str = "manifest.json";
const PUBLIC_BATCHES_INDEX: &str = "batches.jsonl";
const CHECKSUMS: &str = "SHA256SUMS";
const BATCH_PREFIX: &str = "exports/batches";
const STALE_PUBLIC_BLOCKS_INDEX: &str = "blocks.jsonl";

pub(crate) const REQUIRED_CATALOG_FILES: &[&str] =
    &[HTML_INDEX, PUBLIC_MANIFEST, PUBLIC_BATCHES_INDEX, CHECKSUMS];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogGeneration {
    pub(crate) artifact_count: usize,
    pub(crate) batch_count: usize,
    pub(crate) fresh_batch_count: usize,
    pub(crate) cached_batch_count: usize,
    pub(crate) seeded_batch_count: usize,
    pub(crate) inspected_batch_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicManifest {
    schema_version: u64,
    kind: String,
    network: String,
    generated_at: String,
    batch_size: u64,
    paths: PublicManifestPaths,
    batches: PublicBatchesSummary,
    notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicManifestPaths {
    html: String,
    manifest: String,
    batches: String,
    checksums: String,
    batch_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicBatchesSummary {
    count: usize,
    artifact_count: usize,
    first_start_block: Option<u64>,
    last_end_block: Option<u64>,
    total_byte_length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicBatchEntry {
    schema_version: u64,
    network: String,
    batch_start_block: u64,
    batch_end_block: u64,
    batch_size: u64,
    artifact_count: usize,
    created_at: String,
    byte_length: u64,
    sha256: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogCache {
    schema_version: u64,
    kind: String,
    network: String,
    batches: Vec<CachedBatchEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedBatchEntry {
    modified_at_unix_nanos: u64,
    batch: PublicBatchEntry,
}

#[derive(Debug)]
struct ArchiveFile {
    path: PathBuf,
    relative_path: String,
    byte_length: u64,
    modified_at_unix_nanos: u64,
}

#[derive(Debug, Default)]
struct ResolutionStats {
    fresh: usize,
    cached: usize,
    seeded: usize,
    inspected: usize,
}

enum LoadedCache {
    Missing,
    Valid(BTreeMap<String, CachedBatchEntry>),
    Invalid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchArchiveManifest {
    schema_version: u64,
    network: String,
    batch_start_block: u64,
    batch_end_block: u64,
    batch_size: u64,
    artifact_count: usize,
    created_at: String,
}

pub(crate) fn required_catalog_files(config: &CollectorConfig) -> Vec<(&'static str, PathBuf)> {
    REQUIRED_CATALOG_FILES
        .iter()
        .map(|name| (*name, config.network_root().join(name)))
        .collect()
}

pub(crate) fn generate_catalog(
    config: &CollectorConfig,
    exported: &[ExportedBatchMetadata],
) -> anyhow::Result<CatalogGeneration> {
    let (batches, cache_batches, stats) = read_batch_entries(config, exported)?;
    let artifact_count = batches.iter().map(|batch| batch.artifact_count).sum();
    let manifest = public_manifest(config, &batches)?;
    let cache = CatalogCache {
        schema_version: CATALOG_CACHE_SCHEMA_VERSION,
        kind: CATALOG_CACHE_KIND.to_owned(),
        network: config.network.clone(),
        batches: cache_batches,
    };

    write_json(config.catalog_cache_path(), &cache)?;
    write_json(config.network_root().join(PUBLIC_MANIFEST), &manifest)?;
    write_jsonl(config.network_root().join(PUBLIC_BATCHES_INDEX), &batches)?;
    write_bytes_atomic(
        &config.network_root().join(CHECKSUMS),
        checksums_file(&batches).as_bytes(),
    )?;
    write_bytes_atomic(
        &config.network_root().join(HTML_INDEX),
        render_html(&manifest, &batches).as_bytes(),
    )?;
    remove_stale_public_file(config, STALE_PUBLIC_BLOCKS_INDEX)?;

    info!(
        fresh = stats.fresh,
        cached = stats.cached,
        seeded = stats.seeded,
        inspected = stats.inspected,
        "resolved batch catalog metadata"
    );

    Ok(CatalogGeneration {
        artifact_count,
        batch_count: batches.len(),
        fresh_batch_count: stats.fresh,
        cached_batch_count: stats.cached,
        seeded_batch_count: stats.seeded,
        inspected_batch_count: stats.inspected,
    })
}

fn read_batch_entries(
    config: &CollectorConfig,
    exported: &[ExportedBatchMetadata],
) -> anyhow::Result<(
    Vec<PublicBatchEntry>,
    Vec<CachedBatchEntry>,
    ResolutionStats,
)> {
    let archives = archive_files(config)?;
    let mut fresh = fresh_batch_entries(config, exported)?;
    let loaded_cache = load_catalog_cache(config)?;
    let seeds = if matches!(loaded_cache, LoadedCache::Missing) {
        read_public_seed(config)?
    } else {
        BTreeMap::new()
    };
    let cache = match loaded_cache {
        LoadedCache::Valid(cache) => cache,
        LoadedCache::Missing | LoadedCache::Invalid => BTreeMap::new(),
    };

    let mut batches = Vec::with_capacity(archives.len());
    let mut cache_batches = Vec::with_capacity(archives.len());
    let mut stats = ResolutionStats::default();
    for archive in archives {
        let batch = if let Some(batch) = fresh.remove(&archive.relative_path) {
            ensure!(
                batch.batch.byte_length == archive.byte_length
                    && batch.modified_at_unix_nanos == archive.modified_at_unix_nanos,
                "fresh metadata for {} does not match the completed archive",
                archive.path.display()
            );
            stats.fresh += 1;
            batch.batch
        } else if let Some(cached) = cache.get(&archive.relative_path).filter(|cached| {
            cached.batch.byte_length == archive.byte_length
                && cached.modified_at_unix_nanos == archive.modified_at_unix_nanos
        }) {
            stats.cached += 1;
            cached.batch.clone()
        } else if let Some(seed) = seeds
            .get(&archive.relative_path)
            .filter(|seed| seed.byte_length == archive.byte_length)
        {
            stats.seeded += 1;
            seed.clone()
        } else {
            stats.inspected += 1;
            inspect_archive(config, &archive)?
        };

        cache_batches.push(CachedBatchEntry {
            modified_at_unix_nanos: archive.modified_at_unix_nanos,
            batch: batch.clone(),
        });
        batches.push(batch);
    }
    ensure!(
        fresh.is_empty(),
        "metadata was returned for exported archives that are not present in {}",
        config.batches_root().display()
    );

    batches.sort_by_key(|batch| (batch.batch_start_block, batch.batch_end_block));
    cache_batches
        .sort_by_key(|cached| (cached.batch.batch_start_block, cached.batch.batch_end_block));
    Ok((batches, cache_batches, stats))
}

fn archive_files(config: &CollectorConfig) -> anyhow::Result<Vec<ArchiveFile>> {
    if !config.batches_root().exists() {
        return Ok(Vec::new());
    }

    let mut archives = Vec::new();
    for entry in fs::read_dir(config.batches_root()).with_context(|| {
        format!(
            "failed to read batch export directory {}",
            config.batches_root().display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "failed to read entry in batch export directory {}",
                config.batches_root().display()
            )
        })?;
        let path = entry.path();
        if entry
            .file_type()
            .with_context(|| format!("failed to read file type for {}", path.display()))?
            .is_file()
            && is_batch_archive(&path)
        {
            let metadata = fs::metadata(&path)
                .with_context(|| format!("failed to stat batch archive {}", path.display()))?;
            let relative_path = path.strip_prefix(config.network_root()).with_context(|| {
                format!(
                    "batch archive {} is not under {}",
                    path.display(),
                    config.network_root().display()
                )
            })?;
            archives.push(ArchiveFile {
                relative_path: path_to_slash_string(relative_path),
                byte_length: metadata.len(),
                modified_at_unix_nanos: modified_at_unix_nanos(&path, &metadata)?,
                path,
            });
        }
    }
    archives.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(archives)
}

fn fresh_batch_entries(
    config: &CollectorConfig,
    exported: &[ExportedBatchMetadata],
) -> anyhow::Result<BTreeMap<String, CachedBatchEntry>> {
    let mut entries = BTreeMap::new();
    for exported in exported {
        let relative_path = exported
            .path
            .strip_prefix(config.network_root())
            .with_context(|| {
                format!(
                    "exported batch archive {} is not under {}",
                    exported.path.display(),
                    config.network_root().display()
                )
            })?;
        let batch = PublicBatchEntry {
            schema_version: CATALOG_SCHEMA_VERSION,
            network: exported.network.clone(),
            batch_start_block: exported.batch_start_block,
            batch_end_block: exported.batch_end_block,
            batch_size: exported.batch_size,
            artifact_count: exported.artifact_count,
            created_at: exported.created_at.clone(),
            byte_length: exported.byte_length,
            sha256: exported.sha256.clone(),
            path: path_to_slash_string(relative_path),
        };
        ensure!(
            exported.manifest_schema_version == BATCH_MANIFEST_SCHEMA_VERSION,
            "exported batch archive {} uses unsupported manifest schema version {}; expected {}",
            exported.path.display(),
            exported.manifest_schema_version,
            BATCH_MANIFEST_SCHEMA_VERSION
        );
        ensure!(
            valid_public_batch_entry(config, &batch),
            "exported metadata for {} is invalid",
            exported.path.display()
        );
        let key = batch.path.clone();
        ensure!(
            entries
                .insert(
                    key.clone(),
                    CachedBatchEntry {
                        modified_at_unix_nanos: exported.modified_at_unix_nanos,
                        batch,
                    },
                )
                .is_none(),
            "duplicate exported metadata for {key}"
        );
    }
    Ok(entries)
}

fn load_catalog_cache(config: &CollectorConfig) -> anyhow::Result<LoadedCache> {
    let path = config.catalog_cache_path();
    if !path.exists() {
        return Ok(LoadedCache::Missing);
    }
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read catalog cache {}", path.display()))?;
    let cache: CatalogCache = match serde_json::from_slice(&bytes) {
        Ok(cache) => cache,
        Err(error) => {
            warn!(path = %path.display(), %error, "ignoring malformed catalog cache");
            return Ok(LoadedCache::Invalid);
        }
    };
    if cache.schema_version != CATALOG_CACHE_SCHEMA_VERSION
        || cache.kind != CATALOG_CACHE_KIND
        || cache.network != config.network
    {
        warn!(
            path = %path.display(),
            schema_version = cache.schema_version,
            kind = %cache.kind,
            network = %cache.network,
            "ignoring incompatible catalog cache"
        );
        return Ok(LoadedCache::Invalid);
    }

    let mut entries = BTreeMap::new();
    for cached in cache.batches {
        if !valid_public_batch_entry(config, &cached.batch)
            || entries.insert(cached.batch.path.clone(), cached).is_some()
        {
            warn!(path = %path.display(), "ignoring invalid catalog cache");
            return Ok(LoadedCache::Invalid);
        }
    }
    Ok(LoadedCache::Valid(entries))
}

fn read_public_seed(
    config: &CollectorConfig,
) -> anyhow::Result<BTreeMap<String, PublicBatchEntry>> {
    let path = config.network_root().join(PUBLIC_BATCHES_INDEX);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let file = fs::File::open(&path)
        .with_context(|| format!("failed to open public batch index {}", path.display()))?;
    let mut entries = BTreeMap::new();
    let mut conflicted = BTreeSet::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.with_context(|| {
            format!(
                "failed to read line {line_number} from public batch index {}",
                path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let batch: PublicBatchEntry = match serde_json::from_str(&line) {
            Ok(batch) if valid_public_batch_entry(config, &batch) => batch,
            Ok(_) => {
                warn!(
                    path = %path.display(),
                    line = line_number,
                    "ignoring invalid public batch seed entry"
                );
                continue;
            }
            Err(error) => {
                warn!(
                    path = %path.display(),
                    line = line_number,
                    %error,
                    "ignoring malformed public batch seed entry"
                );
                continue;
            }
        };
        if conflicted.contains(&batch.path) {
            continue;
        }
        match entries.entry(batch.path.clone()) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(batch);
            }
            std::collections::btree_map::Entry::Occupied(slot) => {
                let batch_path = batch.path.clone();
                slot.remove();
                conflicted.insert(batch_path.clone());
                warn!(
                    path = %path.display(),
                    line = line_number,
                    batch_path,
                    "ignoring duplicate public batch seed entries"
                );
            }
        }
    }
    Ok(entries)
}

fn inspect_archive(
    config: &CollectorConfig,
    archive: &ArchiveFile,
) -> anyhow::Result<PublicBatchEntry> {
    let manifest = read_batch_manifest(&archive.path)?;
    ensure!(
        manifest.schema_version == BATCH_MANIFEST_SCHEMA_VERSION,
        "batch archive {} uses unsupported manifest schema version {}; expected {}",
        archive.path.display(),
        manifest.schema_version,
        BATCH_MANIFEST_SCHEMA_VERSION
    );
    ensure!(
        manifest.network == config.network,
        "batch archive {} is for network {}, expected {}",
        archive.path.display(),
        manifest.network,
        config.network
    );
    let batch = PublicBatchEntry {
        schema_version: CATALOG_SCHEMA_VERSION,
        network: manifest.network,
        batch_start_block: manifest.batch_start_block,
        batch_end_block: manifest.batch_end_block,
        batch_size: manifest.batch_size,
        artifact_count: manifest.artifact_count,
        created_at: manifest.created_at,
        byte_length: archive.byte_length,
        sha256: artifact::file_sha256_hex(&archive.path)?,
        path: archive.relative_path.clone(),
    };
    ensure!(
        valid_public_batch_entry(config, &batch),
        "batch archive {} has invalid catalog metadata",
        archive.path.display()
    );
    Ok(batch)
}

fn valid_public_batch_entry(config: &CollectorConfig, batch: &PublicBatchEntry) -> bool {
    let path = Path::new(&batch.path);
    let expected_path = Path::new(BATCH_PREFIX).join(format!(
        "{}-{}.tar.zst",
        batch.batch_start_block, batch.batch_end_block
    ));
    let valid_path = !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path == expected_path;
    let valid_range = batch
        .batch_end_block
        .checked_sub(batch.batch_start_block)
        .and_then(|span| span.checked_add(1))
        == Some(batch.batch_size)
        && batch.batch_size > 0;
    batch.schema_version == CATALOG_SCHEMA_VERSION
        && batch.network == config.network
        && valid_path
        && valid_range
        && valid_sha256(&batch.sha256)
}

fn valid_sha256(sha256: &str) -> bool {
    sha256.len() == 66
        && sha256.starts_with("0x")
        && sha256[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn modified_at_unix_nanos(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<u64> {
    let modified = metadata.modified().with_context(|| {
        format!(
            "failed to read modification time for batch archive {}",
            path.display()
        )
    })?;
    let nanos = modified
        .duration_since(UNIX_EPOCH)
        .with_context(|| {
            format!(
                "batch archive {} has a modification time before the Unix epoch",
                path.display()
            )
        })?
        .as_nanos();
    u64::try_from(nanos).with_context(|| {
        format!(
            "batch archive {} has an out-of-range modification time",
            path.display()
        )
    })
}

fn public_manifest(
    config: &CollectorConfig,
    batches: &[PublicBatchEntry],
) -> anyhow::Result<PublicManifest> {
    let first_start_block = batches.iter().map(|entry| entry.batch_start_block).min();
    let last_end_block = batches.iter().map(|entry| entry.batch_end_block).max();
    let artifact_count = batches.iter().map(|entry| entry.artifact_count).sum();
    let total_byte_length = batches.iter().map(|entry| entry.byte_length).sum();

    Ok(PublicManifest {
        schema_version: CATALOG_SCHEMA_VERSION,
        kind: CATALOG_KIND.to_owned(),
        network: config.network.clone(),
        generated_at: artifact::utc_now_rfc3339()?,
        batch_size: config.batch_size,
        paths: PublicManifestPaths {
            html: HTML_INDEX.to_owned(),
            manifest: PUBLIC_MANIFEST.to_owned(),
            batches: PUBLIC_BATCHES_INDEX.to_owned(),
            checksums: CHECKSUMS.to_owned(),
            batch_prefix: BATCH_PREFIX.to_owned(),
        },
        batches: PublicBatchesSummary {
            count: batches.len(),
            artifact_count,
            first_start_block,
            last_end_block,
            total_byte_length,
        },
        notes: vec![
            "Public downloads are batch archives containing benchmark-ready EEST fixtures under blockchain_tests/; individual fixtures are not published as standalone R2 objects.".to_owned(),
            "After extraction, pass the archive root directly to ere-hosts --input-folder.".to_owned(),
            "Cloudflare R2 public buckets do not provide directory listing; use this page or the JSON indexes instead.".to_owned(),
        ],
    })
}

fn remove_stale_public_file(config: &CollectorConfig, name: &str) -> anyhow::Result<()> {
    let path = config.network_root().join(name);
    if path.exists() {
        fs::remove_file(&path).with_context(|| {
            format!(
                "failed to remove stale public catalog file {}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn read_batch_manifest(path: &Path) -> anyhow::Result<BatchArchiveManifest> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open batch archive {}", path.display()))?;
    let decoder = zstd::stream::read::Decoder::new(file)
        .with_context(|| format!("failed to create zstd decoder for {}", path.display()))?;
    let mut archive = Archive::new(decoder);
    for entry in archive
        .entries()
        .with_context(|| format!("failed to read tar entries from {}", path.display()))?
    {
        let mut entry =
            entry.with_context(|| format!("failed to read tar entry from {}", path.display()))?;
        if entry
            .path()
            .with_context(|| format!("failed to read tar entry path from {}", path.display()))?
            .as_ref()
            == Path::new(BATCH_MANIFEST_PATH)
        {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).with_context(|| {
                format!(
                    "failed to read {BATCH_MANIFEST_PATH} from {}",
                    path.display()
                )
            })?;
            let manifest = serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "failed to decode {BATCH_MANIFEST_PATH} from {}",
                    path.display()
                )
            })?;
            return Ok(manifest);
        }
    }
    anyhow::bail!(
        "batch archive {} does not contain {BATCH_MANIFEST_PATH}",
        path.display()
    )
}

fn write_json<T>(path: PathBuf, value: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value).context("failed to serialize JSON")?;
    write_bytes_atomic(&path, &bytes)
}

fn write_jsonl<T>(path: PathBuf, entries: &[T]) -> anyhow::Result<()>
where
    T: Serialize,
{
    let mut bytes = Vec::new();
    for entry in entries {
        serde_json::to_writer(&mut bytes, entry).context("failed to serialize JSONL entry")?;
        bytes.push(b'\n');
    }
    write_bytes_atomic(&path, &bytes)
}

fn checksums_file(batches: &[PublicBatchEntry]) -> String {
    let mut checksums = String::new();
    for batch in batches {
        checksums.push_str(batch.sha256.strip_prefix("0x").unwrap_or(&batch.sha256));
        checksums.push_str("  ");
        checksums.push_str(batch.path.rsplit('/').next().unwrap_or(&batch.path));
        checksums.push('\n');
    }
    checksums
}

fn render_html(manifest: &PublicManifest, batches: &[PublicBatchEntry]) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>");
    push_escaped(&mut html, &manifest.network);
    html.push_str(" stateless inputs</title>\n<style>\n");
    html.push_str("body{font-family:system-ui,-apple-system,Segoe UI,sans-serif;line-height:1.5;margin:0;color:#1f2933;background:#f7f9fb}main{max-width:1080px;margin:0 auto;padding:32px 20px 48px}h1{font-size:32px;margin:0 0 8px}h2{font-size:20px;margin-top:32px}.summary{display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:12px;margin:24px 0}.metric{background:#fff;border:1px solid #d9e2ec;border-radius:8px;padding:14px}.metric strong{display:block;font-size:24px}.panel{background:#fff;border:1px solid #d9e2ec;border-radius:8px;padding:18px;margin:18px 0}code,pre{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}pre{overflow:auto;background:#102a43;color:#f0f4f8;border-radius:8px;padding:14px}table{width:100%;border-collapse:collapse;background:#fff;border:1px solid #d9e2ec}th,td{text-align:left;border-bottom:1px solid #d9e2ec;padding:10px}th{background:#eef2f7}a{color:#0967d2}.muted{color:#627d98}.nowrap{white-space:nowrap}\n");
    html.push_str("</style>\n</head>\n<body>\n<main>\n");
    html.push_str("<h1>");
    push_escaped(&mut html, &manifest.network);
    html.push_str(" stateless inputs</h1>\n");
    html.push_str("<p class=\"muted\">Batch-first public dataset catalog generated at ");
    push_escaped(&mut html, &manifest.generated_at);
    html.push_str(".</p>\n");

    html.push_str("<section class=\"summary\">\n");
    push_metric(
        &mut html,
        "Block artifacts",
        manifest.batches.artifact_count,
    );
    push_metric(&mut html, "Batches", manifest.batches.count);
    push_metric(&mut html, "Batch size", manifest.batch_size);
    push_metric(
        &mut html,
        "Total batch size",
        format_byte_size(manifest.batches.total_byte_length),
    );
    html.push_str("</section>\n");

    html.push_str("<section class=\"panel\">\n<h2>How to download</h2>\n");
    html.push_str("<p>Each archive contains benchmark-ready EEST fixtures under <code>blockchain_tests/</code> and metadata at <code>.meta/manifest.json</code>.</p>\n");
    if let Some(first_batch) = batches.first() {
        html.push_str("<pre>curl -LO ");
        push_escaped(&mut html, &first_batch.path);
        html.push_str("\ntar --zstd -xf ");
        push_escaped(
            &mut html,
            first_batch
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&first_batch.path),
        );
        html.push_str("</pre>\n");
        html.push_str("<p>Pass the extracted directory directly to <code>ere-hosts --input-folder</code>.</p>\n");
    } else {
        html.push_str("<p>No complete batch archives are available yet.</p>\n");
    }
    html.push_str("<p>Verify downloads with <a href=\"");
    push_escaped_attr(&mut html, &manifest.paths.checksums);
    html.push_str("\"><code>SHA256SUMS</code></a>.</p>\n</section>\n");

    html.push_str("<section class=\"panel\">\n<h2>Machine-readable indexes</h2>\n<ul>\n");
    push_link_item(&mut html, &manifest.paths.manifest, "Dataset manifest");
    push_link_item(&mut html, &manifest.paths.batches, "Batch index");
    html.push_str("</ul>\n<p class=\"muted\">R2 public buckets do not provide directory listing; use these files instead of folder URLs.</p>\n</section>\n");

    html.push_str("<h2>Batch archives</h2>\n");
    if batches.is_empty() {
        html.push_str("<p>No completed batch archives have been exported yet.</p>\n");
    } else {
        html.push_str("<table>\n<thead><tr><th>Blocks</th><th>Artifacts</th><th>Size</th><th>SHA-256</th><th>Download</th></tr></thead>\n<tbody>\n");
        for batch in batches.iter().rev() {
            html.push_str("<tr><td class=\"nowrap\">");
            push_escaped(
                &mut html,
                &format!("{}-{}", batch.batch_start_block, batch.batch_end_block),
            );
            html.push_str("</td><td>");
            push_escaped(&mut html, &batch.artifact_count.to_string());
            html.push_str("</td><td>");
            push_escaped(&mut html, &format_byte_size(batch.byte_length));
            html.push_str("</td><td><code>");
            push_escaped(&mut html, &short_sha256(&batch.sha256));
            html.push_str("</code></td><td><a href=\"");
            push_escaped_attr(&mut html, &batch.path);
            html.push_str("\">");
            push_escaped(
                &mut html,
                batch.path.rsplit('/').next().unwrap_or(&batch.path),
            );
            html.push_str("</a></td></tr>\n");
        }
        html.push_str("</tbody>\n</table>\n");
    }

    html.push_str("</main>\n</body>\n</html>\n");
    html
}

fn push_metric<T>(html: &mut String, label: &str, value: T)
where
    T: std::fmt::Display,
{
    html.push_str("<div class=\"metric\"><span>");
    push_escaped(html, label);
    html.push_str("</span><strong>");
    push_escaped(html, &value.to_string());
    html.push_str("</strong></div>\n");
}

fn push_link_item(html: &mut String, href: &str, label: &str) {
    html.push_str("<li><a href=\"");
    push_escaped_attr(html, href);
    html.push_str("\"><code>");
    push_escaped(html, href);
    html.push_str("</code></a> ");
    push_escaped(html, label);
    html.push_str("</li>\n");
}

fn push_escaped_attr(out: &mut String, input: &str) {
    push_escaped(out, input);
}

fn push_escaped(out: &mut String, input: &str) {
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

fn short_sha256(sha256: &str) -> String {
    let hash = sha256.strip_prefix("0x").unwrap_or(sha256);
    match hash.get(..16) {
        Some(prefix) => format!("0x{prefix}..."),
        None => sha256.to_owned(),
    }
}

fn format_byte_size(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        return format!("{bytes} B");
    }

    let formatted = format!("{value:.2}");
    let formatted = formatted.trim_end_matches('0').trim_end_matches('.');
    format!("{formatted} {}", UNITS[unit])
}

fn is_batch_archive(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".tar.zst"))
}

#[cfg(test)]
mod tests {
    use std::{
        fs::FileTimes,
        time::{Duration, SystemTime},
    };

    use alloy_primitives::B256;
    use serde_json::Value;

    use crate::{
        artifact::{
            StatelessInputArtifact, append_index_entry, test_generated_input, write_artifact_atomic,
        },
        export,
    };

    use super::*;

    #[test]
    fn formats_byte_sizes_for_display() {
        assert_eq!(format_byte_size(0), "0 B");
        assert_eq!(format_byte_size(1_023), "1023 B");
        assert_eq!(format_byte_size(1_024), "1 KiB");
        assert_eq!(format_byte_size(1_536), "1.5 KiB");
        assert_eq!(format_byte_size(1_891_330_682), "1.76 GiB");
    }

    #[test]
    fn generates_public_catalog_for_completed_batches() {
        let config = test_config("completed_batches", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        write_generated_artifact(&config, 2, B256::repeat_byte(0xcc));
        let exported = export::export_batches(&config, false).unwrap();

        assert_eq!(
            exported[0].byte_length,
            fs::metadata(&exported[0].path).unwrap().len()
        );
        assert_eq!(
            exported[0].sha256,
            artifact::file_sha256_hex(&exported[0].path).unwrap()
        );
        let generation = generate_catalog(&config, &exported).unwrap();

        assert_eq!(generation.artifact_count, 2);
        assert_eq!(generation.batch_count, 1);
        assert_eq!(generation.fresh_batch_count, 1);
        assert_eq!(generation.inspected_batch_count, 0);
        assert!(config.catalog_cache_path().is_file());
        assert!(config.network_root().join("index.html").is_file());
        assert!(config.network_root().join("manifest.json").is_file());
        assert!(config.network_root().join("batches.jsonl").is_file());
        assert!(!config.network_root().join("blocks.jsonl").exists());
        assert!(config.network_root().join("SHA256SUMS").is_file());

        let manifest: Value =
            serde_json::from_slice(&fs::read(config.network_root().join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["network"], "glamsterdam-devnet-8");
        assert!(manifest.get("blocks").is_none());
        assert!(manifest["paths"].get("blocks").is_none());
        assert!(manifest["paths"].get("legacyBlockIndex").is_none());
        assert_eq!(manifest["batches"]["count"], 1);
        assert_eq!(manifest["batches"]["artifactCount"], 2);

        let batches = read_jsonl_values(&config.network_root().join("batches.jsonl"));
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0]["path"], "exports/batches/0-1.tar.zst");
        assert_eq!(
            batches[0]["byteLength"],
            fs::metadata(config.batches_root().join("0-1.tar.zst"))
                .unwrap()
                .len()
        );
        assert_eq!(
            batches[0]["sha256"],
            artifact::file_sha256_hex(&config.batches_root().join("0-1.tar.zst")).unwrap()
        );

        let checksums = fs::read_to_string(config.network_root().join("SHA256SUMS")).unwrap();
        assert!(checksums.contains("  0-1.tar.zst\n"));

        let html = fs::read_to_string(config.network_root().join("index.html")).unwrap();
        assert!(html.contains("glamsterdam-devnet-8 stateless inputs"));
        assert!(html.contains("Total batch size"));
        assert!(!html.contains("Total batch bytes"));
        assert!(html.contains("exports/batches/0-1.tar.zst"));
        assert!(html.contains("blockchain_tests/"));
        assert!(html.contains(".meta/manifest.json"));
        assert!(!html.contains("blocks.jsonl"));
        assert!(!html.contains("index.jsonl"));
    }

    #[test]
    fn incomplete_ranges_are_omitted_from_public_catalog() {
        let config = test_config("incomplete_ranges", 2);
        write_generated_artifact(&config, 3, B256::repeat_byte(0xdd));
        fs::write(config.network_root().join("blocks.jsonl"), b"stale\n").unwrap();
        export::export_batches(&config, false).unwrap();

        let generation = generate_catalog(&config, &[]).unwrap();

        assert_eq!(generation.artifact_count, 0);
        assert_eq!(generation.batch_count, 0);
        assert!(read_jsonl_values(&config.network_root().join("batches.jsonl")).is_empty());
        assert!(!config.network_root().join("blocks.jsonl").exists());
    }

    #[test]
    fn catalog_includes_existing_archives_when_export_skips_them() {
        let config = test_config("skipped_archives", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));

        let exported = export::export_batches(&config, false).unwrap();
        assert_eq!(exported.len(), 1);
        generate_catalog(&config, &exported).unwrap();
        assert!(export::export_batches(&config, false).unwrap().is_empty());
        let generation = generate_catalog(&config, &[]).unwrap();

        assert_eq!(generation.artifact_count, 2);
        assert_eq!(generation.batch_count, 1);
        assert_eq!(generation.cached_batch_count, 1);
        assert_eq!(generation.inspected_batch_count, 0);
        assert_eq!(
            read_jsonl_values(&config.network_root().join("batches.jsonl")).len(),
            1
        );
    }

    #[test]
    fn html_lists_batch_archives_by_block_range_descending() {
        let config = test_config("descending_archives", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        write_generated_artifact(&config, 2, B256::repeat_byte(0xcc));
        write_generated_artifact(&config, 3, B256::repeat_byte(0xdd));
        export::export_batches(&config, false).unwrap();

        generate_catalog(&config, &[]).unwrap();

        let html = fs::read_to_string(config.network_root().join("index.html")).unwrap();
        let newest = html.find(">2-3</td>").unwrap();
        let oldest = html.find(">0-1</td>").unwrap();
        assert!(newest < oldest);

        let batches = read_jsonl_values(&config.network_root().join("batches.jsonl"));
        assert_eq!(batches[0]["batchStartBlock"], 0);
        assert_eq!(batches[1]["batchStartBlock"], 2);
    }

    #[test]
    fn seeds_missing_cache_from_public_batch_index() {
        let config = test_config("seed_cache", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        let exported = export::export_batches(&config, false).unwrap();
        generate_catalog(&config, &exported).unwrap();
        fs::remove_file(config.catalog_cache_path()).unwrap();

        let seeded = generate_catalog(&config, &[]).unwrap();
        assert_eq!(seeded.seeded_batch_count, 1);
        assert_eq!(seeded.inspected_batch_count, 0);

        let cached = generate_catalog(&config, &[]).unwrap();
        assert_eq!(cached.cached_batch_count, 1);
        assert_eq!(cached.inspected_batch_count, 0);
    }

    #[test]
    fn modification_time_change_invalidates_cached_entry() {
        let config = test_config("mtime_change", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        let exported = export::export_batches(&config, false).unwrap();
        generate_catalog(&config, &exported).unwrap();
        let archive_path = config.batches_root().join("0-1.tar.zst");
        let original_sha256 = exported[0].sha256.clone();
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&archive_path)
            .unwrap();
        file.set_times(FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(2)))
            .unwrap();

        let generation = generate_catalog(&config, &[]).unwrap();

        assert_eq!(generation.cached_batch_count, 0);
        assert_eq!(generation.inspected_batch_count, 1);
        let batches = read_jsonl_values(&config.network_root().join(PUBLIC_BATCHES_INDEX));
        assert_eq!(batches[0]["sha256"], original_sha256);
    }

    #[test]
    fn force_replacement_overrides_cached_metadata() {
        let config = test_config("force_replacement", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        let initial = export::export_batches(&config, false).unwrap();
        generate_catalog(&config, &initial).unwrap();
        let initial_sha256 = initial[0].sha256.clone();

        write_generated_artifact(&config, 0, B256::repeat_byte(0xcc));
        let replaced = export::export_batches(&config, true).unwrap();
        let generation = generate_catalog(&config, &replaced).unwrap();

        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].artifact_count, 3);
        assert_ne!(replaced[0].sha256, initial_sha256);
        assert_eq!(generation.fresh_batch_count, 1);
        assert_eq!(generation.cached_batch_count, 0);
        assert_eq!(generation.inspected_batch_count, 0);
    }

    #[test]
    fn deleted_archive_is_removed_from_cache_and_public_catalog() {
        let config = test_config("deleted_archive", 2);
        for block_number in 0..4 {
            write_generated_artifact(&config, block_number, B256::repeat_byte(block_number as u8));
        }
        let exported = export::export_batches(&config, false).unwrap();
        generate_catalog(&config, &exported).unwrap();
        fs::remove_file(config.batches_root().join("0-1.tar.zst")).unwrap();

        let generation = generate_catalog(&config, &[]).unwrap();

        assert_eq!(generation.batch_count, 1);
        assert_eq!(generation.cached_batch_count, 1);
        let batches = read_jsonl_values(&config.network_root().join(PUBLIC_BATCHES_INDEX));
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0]["path"], "exports/batches/2-3.tar.zst");
        let cache: CatalogCache =
            serde_json::from_slice(&fs::read(config.catalog_cache_path()).unwrap()).unwrap();
        assert_eq!(cache.batches.len(), 1);
        assert_eq!(cache.batches[0].batch.path, "exports/batches/2-3.tar.zst");
    }

    #[test]
    fn invalid_caches_trigger_full_rebuild_without_using_public_seed() {
        let config = test_config("invalid_cache", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        let exported = export::export_batches(&config, false).unwrap();
        generate_catalog(&config, &exported).unwrap();
        let valid_cache: Value =
            serde_json::from_slice(&fs::read(config.catalog_cache_path()).unwrap()).unwrap();

        fs::write(config.catalog_cache_path(), b"{malformed").unwrap();
        assert_full_cache_rebuild(&config);

        let mut wrong_network = valid_cache.clone();
        wrong_network["network"] = Value::String("other-network".to_owned());
        write_cache_value(&config, &wrong_network);
        assert_full_cache_rebuild(&config);

        let mut unsupported_version = valid_cache.clone();
        unsupported_version["schemaVersion"] = Value::from(999);
        write_cache_value(&config, &unsupported_version);
        assert_full_cache_rebuild(&config);

        let mut duplicate = valid_cache;
        let duplicate_batch = duplicate["batches"][0].clone();
        duplicate["batches"]
            .as_array_mut()
            .unwrap()
            .push(duplicate_batch);
        write_cache_value(&config, &duplicate);
        assert_full_cache_rebuild(&config);
    }

    #[test]
    fn invalid_changed_archive_preserves_public_catalog_files() {
        let config = test_config("invalid_changed_archive", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        let exported = export::export_batches(&config, false).unwrap();
        generate_catalog(&config, &exported).unwrap();
        let public_files = required_catalog_files(&config)
            .into_iter()
            .map(|(name, path)| (name, path.clone(), fs::read(path).unwrap()))
            .collect::<Vec<_>>();
        fs::write(
            config.batches_root().join("0-1.tar.zst"),
            b"invalid archive",
        )
        .unwrap();

        let error = generate_catalog(&config, &[]).unwrap_err();

        assert!(
            format!("{error:#}").contains("0-1.tar.zst"),
            "unexpected error: {error:#}"
        );
        for (name, path, contents) in public_files {
            assert_eq!(fs::read(path).unwrap(), contents, "{name} was replaced");
        }
    }

    fn assert_full_cache_rebuild(config: &CollectorConfig) {
        let generation = generate_catalog(config, &[]).unwrap();
        assert_eq!(generation.cached_batch_count, 0);
        assert_eq!(generation.seeded_batch_count, 0);
        assert_eq!(generation.inspected_batch_count, 1);
    }

    fn write_cache_value(config: &CollectorConfig, value: &Value) {
        fs::write(
            config.catalog_cache_path(),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }

    fn write_generated_artifact(config: &CollectorConfig, block_number: u64, block_hash: B256) {
        let generated = test_generated_input(block_number, block_hash);
        let artifact = StatelessInputArtifact::from_generated_at(
            &config.network,
            "head",
            &generated,
            "2026-06-11T00:00:00Z",
            "test-commit".to_owned(),
        )
        .unwrap();
        let write = write_artifact_atomic(&config.blocks_root(), &artifact).unwrap();
        let index_entry = artifact.index_entry(&PathBuf::from("blocks").join(write.relative_path));
        append_index_entry(&config.index_path(), &index_entry).unwrap();
    }

    fn read_jsonl_values(path: &Path) -> Vec<Value> {
        let contents = fs::read_to_string(path).unwrap();
        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn test_config(name: &str, batch_size: u64) -> CollectorConfig {
        let out_root = std::env::temp_dir().join(format!(
            "witness-generator-spec-cli-catalog-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&out_root);
        CollectorConfig {
            network: "glamsterdam-devnet-8".to_owned(),
            cl_url: "http://cl".to_owned(),
            el_url: "http://el".to_owned(),
            out_root,
            poll_interval: Duration::from_secs(4),
            request_timeout: Duration::from_secs(30),
            batch_size,
            r2: None,
        }
    }
}
