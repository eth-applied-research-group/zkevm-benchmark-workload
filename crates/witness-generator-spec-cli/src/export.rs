use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Write},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use alloy_primitives::hex;
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::{Builder, Header};
use tracing::{info, warn};

use crate::{
    artifact::{
        self, ARTIFACT_SCHEMA_VERSION, ArtifactIndexEntry, BATCH_MANIFEST_PATH,
        BATCH_MANIFEST_SCHEMA_VERSION, StatelessInputArtifact, fixture_archive_path,
        path_to_slash_string, read_artifact_with_json, relative_artifact_path_from_parts,
        sha256_hex,
    },
    config::CollectorConfig,
};

const ZSTD_LEVEL: i32 = 3;

#[derive(Debug)]
struct ArtifactDescriptor {
    path: PathBuf,
    metadata: ArtifactIndexEntry,
}

#[derive(Debug, Default)]
struct DiscoveryStats {
    artifacts: usize,
    indexed: usize,
    recovered: usize,
    stale: usize,
    malformed: usize,
    invalid: usize,
    duplicates: usize,
    conflicts: usize,
}

#[derive(Debug, Default)]
struct IndexCache {
    entries: BTreeMap<PathBuf, ArtifactIndexEntry>,
    conflicted_paths: BTreeSet<PathBuf>,
    malformed: usize,
    invalid: usize,
    duplicates: usize,
    conflicts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportedBatchMetadata {
    pub(crate) path: PathBuf,
    pub(crate) manifest_schema_version: u64,
    pub(crate) network: String,
    pub(crate) batch_start_block: u64,
    pub(crate) batch_end_block: u64,
    pub(crate) batch_size: u64,
    pub(crate) artifact_count: usize,
    pub(crate) created_at: String,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
    pub(crate) modified_at_unix_nanos: u64,
}

impl AsRef<Path> for ExportedBatchMetadata {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl std::ops::Deref for ExportedBatchMetadata {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

#[derive(Debug)]
struct WrittenBatchMetadata {
    manifest_schema_version: u64,
    network: String,
    batch_start_block: u64,
    batch_end_block: u64,
    batch_size: u64,
    artifact_count: usize,
    created_at: String,
    byte_length: u64,
    sha256: String,
}

struct ChecksumWriter<W> {
    inner: W,
    hasher: Sha256,
    byte_length: u64,
}

impl<W> ChecksumWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            byte_length: 0,
        }
    }

    fn finish(self) -> (W, u64, String) {
        (
            self.inner,
            self.byte_length,
            format!("0x{}", hex::encode(self.hasher.finalize())),
        )
    }
}

impl<W> Write for ChecksumWriter<W>
where
    W: Write,
{
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        self.byte_length += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchManifest {
    schema_version: u64,
    network: String,
    batch_start_block: u64,
    batch_end_block: u64,
    batch_size: u64,
    artifact_count: usize,
    created_at: String,
    artifacts: Vec<BatchManifestArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchManifestArtifact {
    archive_path: String,
    block_number: u64,
    block_hash: String,
    gas_used: u64,
    slot_number: u64,
    chain_id: u64,
    stateless_input_byte_length: usize,
    fixture_sha256: String,
    fixture_byte_length: usize,
}

pub(crate) fn export_batches(
    config: &CollectorConfig,
    force: bool,
) -> anyhow::Result<Vec<ExportedBatchMetadata>> {
    let (by_block, discovery) = artifacts_by_block(config)?;
    let complete_batches = complete_batch_starts(&by_block, config.batch_size);
    let mut exported = Vec::new();
    let mut skipped = 0_usize;

    info!(
        artifacts = discovery.artifacts,
        indexed = discovery.indexed,
        recovered = discovery.recovered,
        stale = discovery.stale,
        malformed = discovery.malformed,
        invalid = discovery.invalid,
        duplicates = discovery.duplicates,
        conflicts = discovery.conflicts,
        "discovered artifact metadata"
    );

    fs::create_dir_all(config.batches_root()).with_context(|| {
        format!(
            "failed to create batch export directory {}",
            config.batches_root().display()
        )
    })?;

    for &start in &complete_batches {
        let end = start + config.batch_size - 1;
        let out_path = config.batches_root().join(format!("{start}-{end}.tar.zst"));
        if out_path.exists() && !force {
            skipped += 1;
            continue;
        }

        let artifacts = batch_artifacts(&by_block, start, end);
        exported.push(write_batch_archive(
            config, start, end, &artifacts, &out_path, force,
        )?);
    }

    info!(
        complete = complete_batches.len(),
        skipped,
        exported = exported.len(),
        force,
        "processed complete artifact batches"
    );

    Ok(exported)
}

fn artifacts_by_block(
    config: &CollectorConfig,
) -> anyhow::Result<(BTreeMap<u64, Vec<ArtifactDescriptor>>, DiscoveryStats)> {
    let mut index = load_index(config)?;
    let mut files = Vec::new();
    collect_artifact_files(&config.blocks_root(), &mut files)?;
    files.sort();

    let mut stats = DiscoveryStats {
        malformed: index.malformed,
        invalid: index.invalid,
        duplicates: index.duplicates,
        conflicts: index.conflicts,
        ..DiscoveryStats::default()
    };
    let mut by_block: BTreeMap<u64, Vec<ArtifactDescriptor>> = BTreeMap::new();
    for path in files {
        let relative_path = path
            .strip_prefix(config.network_root())
            .with_context(|| {
                format!(
                    "artifact {} is not under network root {}",
                    path.display(),
                    config.network_root().display()
                )
            })?
            .to_path_buf();
        let metadata = if index.conflicted_paths.contains(&relative_path) {
            None
        } else {
            index.entries.remove(&relative_path)
        };
        let metadata = match metadata {
            Some(metadata) => {
                stats.indexed += 1;
                metadata
            }
            None => {
                stats.recovered += 1;
                recover_artifact_metadata(config, &path, &relative_path)?
            }
        };
        by_block
            .entry(metadata.block_number)
            .or_default()
            .push(ArtifactDescriptor { path, metadata });
    }

    for path in index.entries.keys() {
        warn!(path = %path.display(), "ignoring stale artifact index entry");
        stats.stale += 1;
    }

    for files in by_block.values_mut() {
        files.sort_by(|left, right| left.path.cmp(&right.path));
    }

    stats.artifacts = by_block.values().map(Vec::len).sum();
    Ok((by_block, stats))
}

fn load_index(config: &CollectorConfig) -> anyhow::Result<IndexCache> {
    let index_path = config.index_path();
    if !index_path.exists() {
        return Ok(IndexCache::default());
    }

    let file = fs::File::open(&index_path)
        .with_context(|| format!("failed to open artifact index {}", index_path.display()))?;
    let mut cache = IndexCache::default();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.with_context(|| {
            format!(
                "failed to read line {line_number} from artifact index {}",
                index_path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: ArtifactIndexEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(error) => {
                warn!(
                    path = %index_path.display(),
                    line = line_number,
                    %error,
                    "ignoring malformed artifact index entry"
                );
                cache.malformed += 1;
                continue;
            }
        };
        let Some(relative_path) = validated_index_path(config, &entry, line_number) else {
            cache.invalid += 1;
            continue;
        };
        if cache.conflicted_paths.contains(&relative_path) {
            cache.conflicts += 1;
            continue;
        }

        match cache.entries.entry(relative_path.clone()) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(entry);
            }
            std::collections::btree_map::Entry::Occupied(slot) if slot.get() == &entry => {
                warn!(
                    path = %relative_path.display(),
                    line = line_number,
                    "ignoring duplicate artifact index entry"
                );
                cache.duplicates += 1;
            }
            std::collections::btree_map::Entry::Occupied(slot) => {
                warn!(
                    path = %relative_path.display(),
                    line = line_number,
                    "ignoring conflicting artifact index entries"
                );
                slot.remove();
                cache.conflicted_paths.insert(relative_path);
                cache.conflicts += 1;
            }
        }
    }

    Ok(cache)
}

fn validated_index_path(
    config: &CollectorConfig,
    entry: &ArtifactIndexEntry,
    line_number: usize,
) -> Option<PathBuf> {
    let path = PathBuf::from(&entry.path);
    let expected_path = PathBuf::from("blocks").join(relative_artifact_path_from_parts(
        entry.block_number,
        &entry.block_hash,
    ));
    let valid_path = !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path.starts_with("blocks")
        && path == expected_path;
    let valid_metadata =
        entry.schema_version == ARTIFACT_SCHEMA_VERSION && entry.network == config.network;
    if valid_path && valid_metadata {
        return Some(path);
    }

    warn!(
        path = %entry.path,
        line = line_number,
        schema_version = entry.schema_version,
        network = %entry.network,
        "ignoring invalid artifact index entry"
    );
    None
}

fn recover_artifact_metadata(
    config: &CollectorConfig,
    path: &Path,
    relative_path: &Path,
) -> anyhow::Result<ArtifactIndexEntry> {
    let (artifact, fixture_json) = read_artifact_with_json(path)?;
    validate_artifact_identity(config, &artifact, None)?;
    let metadata = artifact.index_entry(relative_path);
    drop(fixture_json);
    drop(artifact);
    Ok(metadata)
}

fn complete_batch_starts(
    by_block: &BTreeMap<u64, Vec<ArtifactDescriptor>>,
    batch_size: u64,
) -> Vec<u64> {
    let starts = by_block
        .keys()
        .map(|block_number| (block_number / batch_size) * batch_size)
        .collect::<BTreeSet<_>>();

    starts
        .into_iter()
        .filter(|start| {
            let end = start + batch_size - 1;
            (*start..=end).all(|block_number| by_block.contains_key(&block_number))
        })
        .collect()
}

fn batch_artifacts(
    by_block: &BTreeMap<u64, Vec<ArtifactDescriptor>>,
    start: u64,
    end: u64,
) -> Vec<&ArtifactDescriptor> {
    (start..=end)
        .flat_map(|block_number| by_block.get(&block_number).into_iter().flatten())
        .collect()
}

fn write_batch_archive(
    config: &CollectorConfig,
    start: u64,
    end: u64,
    artifacts: &[&ArtifactDescriptor],
    out_path: &Path,
    force: bool,
) -> anyhow::Result<ExportedBatchMetadata> {
    let part_path = archive_part_path(out_path);
    if let Some(parent) = part_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let written = match write_batch_archive_part(config, start, end, artifacts, &part_path) {
        Ok(written) => written,
        Err(error) => {
            if part_path.exists() {
                fs::remove_file(&part_path).with_context(|| {
                    format!(
                        "{error:#}; additionally failed to remove partial archive {}",
                        part_path.display()
                    )
                })?;
            }
            return Err(error);
        }
    };

    if out_path.exists() && force {
        fs::remove_file(out_path)
            .with_context(|| format!("failed to replace archive {}", out_path.display()))?;
    }
    fs::rename(&part_path, out_path).with_context(|| {
        format!(
            "failed to atomically rename {} to {}",
            part_path.display(),
            out_path.display()
        )
    })?;

    let modified_at_unix_nanos = modified_at_unix_nanos(out_path)?;
    Ok(ExportedBatchMetadata {
        path: out_path.to_path_buf(),
        manifest_schema_version: written.manifest_schema_version,
        network: written.network,
        batch_start_block: written.batch_start_block,
        batch_end_block: written.batch_end_block,
        batch_size: written.batch_size,
        artifact_count: written.artifact_count,
        created_at: written.created_at,
        byte_length: written.byte_length,
        sha256: written.sha256,
        modified_at_unix_nanos,
    })
}

fn write_batch_archive_part(
    config: &CollectorConfig,
    start: u64,
    end: u64,
    artifacts: &[&ArtifactDescriptor],
    part_path: &Path,
) -> anyhow::Result<WrittenBatchMetadata> {
    let file = fs::File::create(part_path)
        .with_context(|| format!("failed to create partial archive {}", part_path.display()))?;
    let checksum_writer = ChecksumWriter::new(file);
    let encoder = zstd::stream::write::Encoder::new(checksum_writer, ZSTD_LEVEL)
        .context("failed to create zstd encoder")?;
    let mut tar = Builder::new(encoder);
    let mut manifest_artifacts = Vec::with_capacity(artifacts.len());

    for descriptor in artifacts {
        let (artifact, fixture_json) = read_artifact_with_json(&descriptor.path)?;
        validate_artifact_identity(config, &artifact, Some(&descriptor.metadata))?;
        let archive_path = fixture_archive_path(&artifact);
        manifest_artifacts.push(BatchManifestArtifact {
            archive_path: path_to_slash_string(&archive_path),
            block_number: artifact.block_number,
            block_hash: artifact.block_hash.clone(),
            gas_used: artifact.gas_used,
            slot_number: artifact.slot_number,
            chain_id: artifact.chain_id,
            stateless_input_byte_length: artifact.stateless_input_byte_length,
            fixture_sha256: sha256_hex(&fixture_json),
            fixture_byte_length: fixture_json.len(),
        });
        append_bytes(&mut tar, &archive_path, &fixture_json).with_context(|| {
            format!(
                "failed to append {} as {}",
                descriptor.path.display(),
                archive_path.display()
            )
        })?;
    }

    let manifest = BatchManifest {
        schema_version: BATCH_MANIFEST_SCHEMA_VERSION,
        network: config.network.clone(),
        batch_start_block: start,
        batch_end_block: end,
        batch_size: config.batch_size,
        artifact_count: manifest_artifacts.len(),
        created_at: artifact::utc_now_rfc3339()?,
        artifacts: manifest_artifacts,
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).context("failed to serialize batch manifest")?;
    append_bytes(&mut tar, Path::new(BATCH_MANIFEST_PATH), &manifest_bytes)
        .context("failed to append batch manifest")?;

    let encoder = tar.into_inner().context("failed to finish tar archive")?;
    let checksum_writer = encoder.finish().context("failed to finish zstd archive")?;
    let (_file, byte_length, sha256) = checksum_writer.finish();
    Ok(WrittenBatchMetadata {
        manifest_schema_version: manifest.schema_version,
        network: manifest.network,
        batch_start_block: manifest.batch_start_block,
        batch_end_block: manifest.batch_end_block,
        batch_size: manifest.batch_size,
        artifact_count: manifest.artifact_count,
        created_at: manifest.created_at,
        byte_length,
        sha256,
    })
}

fn modified_at_unix_nanos(path: &Path) -> anyhow::Result<u64> {
    let modified = fs::metadata(path)
        .with_context(|| format!("failed to stat batch archive {}", path.display()))?
        .modified()
        .with_context(|| {
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

fn validate_artifact_identity(
    config: &CollectorConfig,
    artifact: &StatelessInputArtifact,
    expected: Option<&ArtifactIndexEntry>,
) -> anyhow::Result<()> {
    ensure!(
        artifact.schema_version == ARTIFACT_SCHEMA_VERSION,
        "artifact uses unsupported schema version {}; expected {}",
        artifact.schema_version,
        ARTIFACT_SCHEMA_VERSION
    );
    ensure!(
        artifact.network == config.network,
        "artifact is for network {}, expected {}",
        artifact.network,
        config.network
    );
    if let Some(expected) = expected {
        ensure!(
            artifact.schema_version == expected.schema_version
                && artifact.network == expected.network
                && artifact.chain_id == expected.chain_id
                && artifact.block_number == expected.block_number
                && artifact.block_hash == expected.block_hash
                && artifact.gas_used == expected.gas_used
                && artifact.slot_number == expected.slot_number
                && artifact.collection_mode == expected.collection_mode
                && artifact.collected_at == expected.collected_at
                && artifact.stateless_input_byte_length == expected.stateless_input_byte_length,
            "artifact metadata does not match its index entry for {}",
            expected.path
        );
    }
    Ok(())
}

fn append_bytes<W: Write>(tar: &mut Builder<W>, path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut header = Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, path, bytes)
        .context("failed to append tar entry")
}

fn collect_artifact_files(dir: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read file type for {}", path.display()))?;
        if file_type.is_dir() {
            collect_artifact_files(&path, files)?;
        } else if file_type.is_file() && is_artifact_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_artifact_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".json.zst"))
}

fn archive_part_path(path: &Path) -> PathBuf {
    path.with_extension(
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension) => format!("{extension}.part"),
            None => "part".to_owned(),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use alloy_primitives::B256;
    use benchmark_runner::stateless_validator::{
        ExecutionClient, benchmark_fixture_paths, stateless_validator_input_iter,
    };
    use tar::Archive;

    use crate::artifact::{
        ArtifactIndexEntry, StatelessInputArtifact, append_index_entry, test_generated_input,
        write_artifact_atomic,
    };

    use super::*;

    #[test]
    fn exports_complete_batches_with_manifest() {
        let config = test_config("complete_batch", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        write_generated_artifact(&config, 2, B256::repeat_byte(0xcc));

        let exported = export_batches(&config, false).unwrap();

        assert_eq!(exported.len(), 1);
        assert_eq!(
            exported[0].file_name().and_then(|name| name.to_str()),
            Some("0-1.tar.zst")
        );
        let manifest = read_manifest_from_archive(&exported[0]);
        assert_eq!(manifest.batch_start_block, 0);
        assert_eq!(manifest.batch_end_block, 1);
        assert_eq!(manifest.artifact_count, 2);
        assert_eq!(manifest.schema_version, 2);
        assert!(manifest.artifacts.iter().all(|artifact| {
            artifact.archive_path.starts_with("blockchain_tests/")
                && artifact.archive_path.ends_with(".json")
        }));
        let entries = archive_entries(&exported[0]);
        assert!(entries.contains(&BATCH_MANIFEST_PATH.to_owned()));
        assert_eq!(
            entries
                .iter()
                .filter(|path| path.starts_with("blockchain_tests/"))
                .count(),
            2
        );
    }

    #[test]
    fn skips_existing_batch_without_force() {
        let config = test_config("skip_existing", 2);
        let first = write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        let second = write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        append_index_entries(&config, [&first, &second]);

        assert_eq!(export_batches(&config, false).unwrap().len(), 1);
        assert!(export_batches(&config, false).unwrap().is_empty());
        assert_eq!(export_batches(&config, true).unwrap().len(), 1);

        fs::write(
            config.network_root().join(&first.path),
            b"corrupted artifact",
        )
        .unwrap();
        assert!(export_batches(&config, false).unwrap().is_empty());

        let out_path = config.batches_root().join("0-1.tar.zst");
        let error = export_batches(&config, true).unwrap_err();
        assert!(format!("{error:#}").contains("failed to decompress artifact"));
        assert!(out_path.exists());
        assert!(!archive_part_path(&out_path).exists());
    }

    #[test]
    fn corrupted_existing_batch_does_not_block_new_batch() {
        let config = test_config("corrupted_existing", 2);
        let first = write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        let second = write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        append_index_entries(&config, [&first, &second]);
        assert_eq!(export_batches(&config, false).unwrap().len(), 1);

        fs::write(
            config.network_root().join(&first.path),
            b"corrupted artifact",
        )
        .unwrap();
        let third = write_generated_artifact(&config, 2, B256::repeat_byte(0xcc));
        let fourth = write_generated_artifact(&config, 3, B256::repeat_byte(0xdd));
        append_index_entries(&config, [&third, &fourth]);

        let exported = export_batches(&config, false).unwrap();

        assert_eq!(exported.len(), 1);
        assert_eq!(
            exported[0].file_name().and_then(|name| name.to_str()),
            Some("2-3.tar.zst")
        );
    }

    #[test]
    fn reconciles_unusable_index_entries_from_filesystem() {
        let config = test_config("reconcile_index", 2);
        let first = write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        let second = write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        let stale = write_generated_artifact(&config, 99, B256::repeat_byte(0xee));
        fs::remove_file(config.network_root().join(&stale.path)).unwrap();

        let mut conflict = first.clone();
        conflict.gas_used += 1;
        let mut unsafe_entry = second;
        unsafe_entry.path = "../outside.json.zst".to_owned();
        let index = [
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&conflict).unwrap(),
            serde_json::to_string(&unsafe_entry).unwrap(),
            "{malformed".to_owned(),
            serde_json::to_string(&stale).unwrap(),
            "{\"schemaVersion\":".to_owned(),
        ]
        .join("\n");
        fs::write(config.index_path(), index).unwrap();

        let (by_block, stats) = artifacts_by_block(&config).unwrap();

        assert_eq!(by_block.values().map(Vec::len).sum::<usize>(), 2);
        assert_eq!(stats.indexed, 0);
        assert_eq!(stats.recovered, 2);
        assert_eq!(stats.stale, 1);
        assert_eq!(stats.malformed, 2);
        assert_eq!(stats.invalid, 1);
        assert_eq!(stats.duplicates, 1);
        assert_eq!(stats.conflicts, 1);
        assert_eq!(export_batches(&config, false).unwrap().len(), 1);
    }

    #[test]
    fn exports_all_reorg_variants_without_cloning_payloads() {
        let config = test_config("reorg_variants", 2);
        let first = write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        let variant = write_generated_artifact(&config, 0, B256::repeat_byte(0xbb));
        let second = write_generated_artifact(&config, 1, B256::repeat_byte(0xcc));
        append_index_entries(&config, [&first, &variant, &second]);

        let archive = export_batches(&config, false).unwrap().remove(0);
        let manifest = read_manifest_from_archive(&archive);

        assert_eq!(manifest.artifact_count, 3);
        assert_eq!(
            archive_entries(&archive)
                .iter()
                .filter(|path| path.starts_with("blockchain_tests/"))
                .count(),
            3
        );
    }

    #[test]
    fn extracted_batch_is_directly_loadable_by_benchmark_runner() {
        let config = test_config("benchmark_ready", 2);
        write_generated_artifact(&config, 0, B256::repeat_byte(0xaa));
        write_generated_artifact(&config, 1, B256::repeat_byte(0xbb));
        let archive = export_batches(&config, false).unwrap().remove(0);
        let extracted = config.out_root.join("extracted");

        let file = fs::File::open(archive).unwrap();
        let decoder = zstd::stream::read::Decoder::new(file).unwrap();
        Archive::new(decoder).unpack(&extracted).unwrap();

        let paths = benchmark_fixture_paths(&extracted).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(
            paths
                .iter()
                .all(|path| path.starts_with(extracted.join("blockchain_tests")))
        );
        let fixtures =
            stateless_validator_input_iter(&extracted, None, ExecutionClient::Reth, None)
                .unwrap()
                .collect::<anyhow::Result<Vec<_>>>()
                .unwrap();
        assert_eq!(fixtures.len(), 2);
        assert_eq!(
            fixtures[0].expected_public_values().unwrap(),
            test_generated_input(0, B256::repeat_byte(0xaa)).stateless_output_bytes
        );
        assert_eq!(fixtures[0].metadata()["block_used_gas"], 21_000);
    }

    fn write_generated_artifact(
        config: &CollectorConfig,
        block_number: u64,
        block_hash: B256,
    ) -> ArtifactIndexEntry {
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
        artifact.index_entry(&PathBuf::from("blocks").join(write.relative_path))
    }

    fn append_index_entries<'a>(
        config: &CollectorConfig,
        entries: impl IntoIterator<Item = &'a ArtifactIndexEntry>,
    ) {
        for entry in entries {
            append_index_entry(&config.index_path(), entry).unwrap();
        }
    }

    fn read_manifest_from_archive(path: &Path) -> BatchManifest {
        let file = fs::File::open(path).unwrap();
        let decoder = zstd::stream::read::Decoder::new(file).unwrap();
        let mut archive = Archive::new(decoder);
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            if entry.path().unwrap().as_ref() == Path::new(BATCH_MANIFEST_PATH) {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                return serde_json::from_slice(&bytes).unwrap();
            }
        }
        panic!("manifest not found");
    }

    fn archive_entries(path: &Path) -> Vec<String> {
        let file = fs::File::open(path).unwrap();
        let decoder = zstd::stream::read::Decoder::new(file).unwrap();
        Archive::new(decoder)
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn test_config(name: &str, batch_size: u64) -> CollectorConfig {
        let out_root = std::env::temp_dir().join(format!(
            "witness-generator-spec-cli-export-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&out_root);
        CollectorConfig {
            network: "glamsterdam-devnet-5".to_owned(),
            cl_url: "http://cl".to_owned(),
            el_url: "http://el".to_owned(),
            out_root,
            poll_interval: std::time::Duration::from_secs(4),
            request_timeout: std::time::Duration::from_secs(30),
            batch_size,
            r2: None,
        }
    }
}
