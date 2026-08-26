# Benchmark Execution

This is the source-of-truth workflow guide for `ere-hosts`.

## Overview

`ere-hosts` consumes canonical EEST fixtures, resolves guest binaries, runs benchmarks across selected zkVMs, and optionally persists or verifies proofs.

Inspect the current CLI surface from the repository root:

```bash
cargo run -p ere-hosts -- --help
```

Prerequisites:

- Docker is required because zkVM hosts are managed through `ere-dockerized`.
- Execute and prove actions require an explicit `--input-folder` pointing to a canonical EEST JSON file, a directory of EEST JSON files, or an EEST checkout containing `blockchain_tests/`.
- Verification reads proofs and does not require `--input-folder`. A supplied verification input path is accepted and ignored for backward compatibility.

## Common Benchmark Commands

Run the stateless validator with Reth:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures
```

Ethrex and Zesu remain accepted CLI values but are temporarily unavailable:

```bash
cargo run -p ere-hosts --release -- --zkvms openvm \
    stateless-validator --execution-client ethrex \
    --input-folder /path/to/eest-fixtures
cargo run -p ere-hosts --release -- --zkvms zisk \
    stateless-validator --execution-client zesu \
    --input-folder /path/to/amsterdam-fixtures
```

Both commands fail at the centralized compatibility check until `ere-guests`
registers compatible tests-zkevm v0.8.2 artifacts.

Run directly from an EEST fixture checkout:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    stateless-validator --execution-client reth \
    --input-folder /path/to/execution-specs/fixtures
```

When the path contains a `blockchain_tests/` subdirectory, only that subtree is used. A direct EEST JSON file is also accepted.

Filter selected fixtures by prefix:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures \
    --fixture test_sha256.py::test_sha256 \
    --fixture test_memory.py::test_mcopy
```

## Action Model

`ere-hosts` supports three actions:

- `--action execute`: execute the guest only. This is the default and requires `--input-folder`.
- `--action prove`: execute, generate a proof, and require `--input-folder`.
- `--action verify`: verify proofs loaded from disk or a downloaded `.tar.gz` archive. Input fixtures are not read.

Input presence and path existence for execute/prove are checked before guest artifact resolution. Verification may omit the option; when `--input-folder` is supplied with verify, its value is ignored even if that path no longer exists.

Timeouts are action-scoped:

- Default execute timeout: `5m`
- Default prove timeout: `15m`
- Default verify timeout: `2s`

Override the selected action's timeout:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    --timeout 90s \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures
```

## Inputs And Outputs

- Metrics output folder default: `zkevm-metrics/`
- Verification proof folder default: `zkevm-fixtures-proofs/`
- ZisK profile output folder default: `zisk-profiles/`

Use the focused references for exact schemas and file layouts:

- [Benchmark Execution Inputs](benchmark-execution-inputs.md) describes canonical input discovery, filtering, client compatibility, and explicit legacy-format rejection.
- [Benchmark Execution Output](benchmark-execution-output.md) describes metrics JSON, `hardware.json`, proof files, input dumps, and workload metadata.

Dump the raw serialized guest inputs used for a run:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    --dump-inputs debug-inputs \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures
```

These input dumps are zkVM-independent, so each fixture input is written once even if multiple zkVMs are selected.

## Proof Persistence And Verification

Generate and save proofs:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    --action prove \
    --save-proofs my-proofs \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures
```

Verify proofs from a local folder without fixture input:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    --action verify \
    --proofs-folder my-proofs \
    stateless-validator --execution-client reth
```

Verify proofs from a remote archive:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    --action verify \
    --proofs-url https://example.com/proofs.tar.gz \
    stateless-validator --execution-client reth
```

When `--proofs-url` is used, the archive is downloaded, extracted to a temporary directory, and cleaned up after verification.

## Guest Artifact Resolution

Default guests use the `ere-guests` artifact resolver. Tagged dependencies use release assets for that tag; commit or branch dependencies use GitHub Actions artifacts for the resolved commit and require `GITHUB_TOKEN` or `GH_TOKEN`. This repository currently pins an exact commit, so one of those token variables is required for default Reth artifact downloads.

Artifacts are named `stateless-validator-<execution-client>-<zkvm>-<zkvm-sdk-version>`, so a zkVM SDK bump changes the resolved file names.

Use local Reth artifacts with `--bin-path <DIRECTORY>`, or provide a compatible remote directory with `--guest-artifact-base-url <URL>`. Those options remain mutually exclusive. The temporary Ethrex/Zesu compatibility check also applies to custom artifact sources.

## Operational Notes

- Use `--force-rerun` to ignore existing metrics and rerun a workload. Without it, fixtures with existing output files are skipped.
- `--resource gpu` selects GPU proving resources where supported.
- `--zisk-profile` only works with `--zkvms zisk` and `--action execute`.
- `--save-proofs` is only valid with `--action prove`.
- `--proofs-url` is only valid with `--action verify`.
