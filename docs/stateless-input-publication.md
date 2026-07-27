# Stateless Fixture Publication

This guide describes how to publish benchmark-ready stateless fixture batches as a public R2 dataset.

## Public Dataset Shape

R2 public buckets do not provide directory listing, so share the generated HTML page rather than the bucket or prefix root:

```text
https://<public-host>/devnets/<network>/index.html
```

The public dataset is batch-first. Users should download complete `.tar.zst` batch archives from:

```text
exports/batches/<start>-<end>.tar.zst
```

Each archive contains directly executable EEST fixtures under `blockchain_tests/`
and describes them in `.meta/manifest.json`. Individual fixtures are not
published as standalone public objects.

## Generated Catalog Files

Running `witness-generator-spec-cli export` rebuilds these files at the network root:

- `index.html`: human-readable landing page with download examples and a batch table.
- `manifest.json`: dataset summary and paths to all public metadata files.
- `batches.jsonl`: one completed batch archive per line.
- `SHA256SUMS`: checksums for completed batch archives.

The generated links are relative, so the same catalog works with an `r2.dev` development URL or a custom domain.

The exporter keeps private derived metadata in `exports/catalog-cache.json`.
Unchanged archives are reused when their path, byte length, and modification
time match, so routine catalog rebuilds do not decompress or checksum every
archive. The cache is not published to R2 and can be deleted safely; the next
export seeds it from a valid `batches.jsonl` or inspects any unresolved
archives.

## Operator Flow

Schema v2 is a clean cut and does not read input-only v1 artifacts. Configure a
fresh local output root and publish to a fresh R2 prefix when deploying it.

Generate one benchmark-ready fixture without starting the collector:

```bash
cargo run -p witness-generator-spec-cli --release -- generate \
    --cl-url http://127.0.0.1:3500 \
    --el-url http://127.0.0.1:8545 \
    --out block.json
```

Collect live stateless fixtures:

```bash
cargo run -p witness-generator-spec-cli --release -- collect \
    --config /etc/witness-generator-spec-cli/glamsterdam-devnet-5.toml
```

Export complete local block ranges and rebuild the public catalog:

```bash
cargo run -p witness-generator-spec-cli --release -- export \
    --config /etc/witness-generator-spec-cli/glamsterdam-devnet-5.toml
```

Publish batch archives and catalog files to R2:

```bash
cargo run -p witness-generator-spec-cli --release -- publish-r2 \
    --config /etc/witness-generator-spec-cli/glamsterdam-devnet-5.toml
```

If `publish-r2` reports a missing public catalog file, run `export` first.

Example systemd services and timers for this flow live in
[`crates/witness-generator-spec-cli/systemd`](../crates/witness-generator-spec-cli/systemd/README.md).

## User Download Examples

Download a batch archive from the generated HTML page:

```bash
curl -LO https://<public-host>/devnets/<network>/exports/batches/32500-32999.tar.zst
tar --zstd -xf 32500-32999.tar.zst
```

Verify checksums:

```bash
curl -LO https://<public-host>/devnets/<network>/SHA256SUMS
sha256sum -c SHA256SUMS
```

Inspect machine-readable metadata:

```bash
curl -fsSL https://<public-host>/devnets/<network>/manifest.json | jq
curl -fsSL https://<public-host>/devnets/<network>/batches.jsonl | head
```

Run an extracted batch directly with the benchmark runner:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
  stateless-validator --execution-client reth \
  --input-folder /path/to/extracted-batch
```

The runner detects the extracted `blockchain_tests/` directory automatically.

## Local EEST Validation

Use [`scripts/validate-r2-stateless-inputs-with-eest.py`](../scripts/validate-r2-stateless-inputs-with-eest.py)
to validate published R2 batch archives against the EEST Amsterdam stateless
guest. The script downloads the selected batch archives, verifies the catalog
metadata, reads each `blockchain_tests/**/*.json` fixture, checks its recorded
stateless input byte length, runs each input through EEST, and requires EEST's
complete output bytes to match the stored `statelessOutputBytes` exactly.

Prerequisites:

- Install [`uv`](https://docs.astral.sh/uv/).
- Check out `ethereum/execution-specs` next to this repository.
- Check out the EEST ref you want to validate against.

```bash
cd /path/to/parent
git clone https://github.com/ethereum/execution-specs.git
cd execution-specs
git fetch --tags
git checkout tests-zkevm@v0.6.2
```

From this repository root, run:

```bash
CATALOG_URL="https://<public-host>/<new-v0.6.2-prefix>/<network>"
EEST_REF="tests-zkevm@v0.6.2"
EEST_DIR="../execution-specs"
SUMMARY_DIR="target/eest-r2-stateless-inputs"

mkdir -p "$SUMMARY_DIR"

uv run --project "$EEST_DIR" --with zstandard \
  python scripts/validate-r2-stateless-inputs-with-eest.py \
    --catalog-url "$CATALOG_URL" \
    --batch-count 70 \
    --summary-json "$SUMMARY_DIR/summary.json" \
    --summary-md "$SUMMARY_DIR/summary.md" \
    --eest-ref "$EEST_REF" \
    --eest-commit "$(git -C "$EEST_DIR" rev-parse HEAD)"

cat "$SUMMARY_DIR/summary.md"
```

The `uv run --project "$EEST_DIR"` part matters: the validator imports EEST's
Python modules from the `execution-specs` checkout while adding `zstandard` for
the compressed R2 artifacts. `--eest-ref` and `--eest-commit` are recorded in
the summaries for provenance; the script does not use them to check out EEST.

Useful selection options:

- `--batch-count N`: validate the latest `N` complete batches from
  `batches.jsonl`.
- `--block-number N`: validate the batch containing one block and only run the
  matching block artifact.
- `--max-artifacts N`: stop after `N` matching artifacts, useful for a fast
  smoke test.

For a quick local smoke test:

```bash
uv run --project "$EEST_DIR" --with zstandard \
  python scripts/validate-r2-stateless-inputs-with-eest.py \
    --catalog-url "$CATALOG_URL" \
    --batch-count 1 \
    --max-artifacts 1 \
    --summary-json "$SUMMARY_DIR/smoke.json" \
    --summary-md "$SUMMARY_DIR/smoke.md" \
    --eest-ref "$EEST_REF" \
    --eest-commit "$(git -C "$EEST_DIR" rev-parse HEAD)"
```

The JSON summary is intended for automation and contains full failure details.
The Markdown summary is intended for local inspection or GitHub Actions job
summaries.
