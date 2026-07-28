# Benchmark Execution Output

This is the detailed output reference for `ere-hosts`: metrics files, hardware metadata, proof files, serialized input dumps, and workload metadata.

For accepted input schemas, see [Benchmark Execution Inputs](benchmark-execution-inputs.md). For common execution commands, see [Benchmark Execution](benchmark-execution.md).

## Output Destinations

Default output locations:

- Metrics output folder: `zkevm-metrics/`
- Verification proof folder: `zkevm-fixtures-proofs/`
- Zisk profile output folder: `zisk-profiles/`

Proofs are only saved when `--save-proofs <PATH>` is provided with `--action prove`.

Serialized guest inputs are only saved when `--dump-inputs <PATH>` is provided. These dumps are zkVM-independent, so each fixture input is written once even if multiple zkVMs are selected.

## Metrics Layout

Completed execution and proving runs are still written as successful metrics even when public values do not match the fixture expectation. In that case the runner logs a warning and writes `output_matched: false` inside `execution.success` or `proving.success`. zkVM errors, proof verification errors, expected-output computation errors, and panics are still recorded as crashed or returned as fatal infrastructure errors as before.

For `stateless-validator` runs, metrics are written under:

```text
zkevm-metrics/
  hardware.json
  <execution-client>-<execution-client-version>/
    <zkvm>-<sdk-version>/
      <fixture-name>.json
```

Each fixture metrics file is a single pretty-printed `BenchmarkRun` JSON object. The `zkevm-metrics` library helper `BenchmarkRun::to_json` serializes a list of runs, but the CLI output files under `zkevm-metrics/` contain one object per file.

## Hardware JSON

`hardware.json` contains detected host hardware:

```json
{
  "cpu_model": "AMD Ryzen 7 PRO 7840U w/ Radeon 780M Graphics",
  "total_ram_gib": 30,
  "gpus": [
    {
      "model": "NVIDIA ..."
    }
  ]
}
```

GPU information is detected through `nvidia-smi` when available.

## BenchmarkRun JSON

A successful execution metrics file has this shape:

```json
{
  "name": "eest__tests_foo_py_test_case_param__block0",
  "timestamp_completed": "2026-05-25T12:34:56.789Z",
  "metadata": {
    "fixture_format": "eest",
    "original_test_name": "tests/foo.py::test_case[param]",
    "source_path": "blockchain_tests/for_amsterdam/compute/mcopy.json",
    "block_index": 0,
    "network": "Amsterdam",
    "chain_id": 1,
    "block_number": 1,
    "block_used_gas": 16,
    "opcode_count": {
      "PUSH1": 5,
      "SSTORE": 2
    },
    "target_opcode": "MCOPY"
  },
  "execution": {
    "success": {
      "output_matched": true,
      "total_num_cycles": 1048737679,
      "region_cycles": {
        "setup": 1000
      },
      "execution_duration": {
        "secs": 12,
        "nanos": 327837000
      }
    }
  }
}
```

Optional top-level fields are omitted when they are not populated:

- `execution` is present for `--action execute`.
- `proving` is present for `--action prove`.
- `verification` is present for `--action verify`.

Success variants:

```json
{
  "execution": {
    "success": {
      "output_matched": true,
      "total_num_cycles": 123,
      "region_cycles": {},
      "execution_duration": {
        "secs": 0,
        "nanos": 1000000
      }
    }
  }
}
```

```json
{
  "proving": {
    "success": {
      "output_matched": true,
      "proof_size": 256,
      "proving_time_ms": 2000,
      "verification_time_ms": 200
    }
  }
}
```

```json
{
  "verification": {
    "success": {
      "proof_size": 256,
      "verification_time_ms": 200
    }
  }
}
```

Crash variants use the same enum wrapper with `crashed`:

```json
{
  "execution": {
    "crashed": {
      "reason": "error or panic message"
    }
  }
}
```

## Metadata By Workload

The `metadata` field is workload-specific:

- Canonical EEST stateless-validator fixtures write EEST provenance and block metadata.
- Standalone verification metrics write `null`.

Canonical EEST metadata has this shape:

```json
{
  "fixture_format": "eest",
  "original_test_name": "tests/foo.py::test_case[param]",
  "source_path": "blockchain_tests/for_amsterdam/compute/mcopy.json",
  "block_index": 0,
  "network": "Amsterdam",
  "chain_id": 1,
  "block_number": 1,
  "block_used_gas": 16,
  "opcode_count": {
    "PUSH1": 5,
    "SSTORE": 2
  },
  "target_opcode": "MCOPY"
}
```

`block_number` and `block_used_gas` are `null` when the source fixture does not provide those values. `opcode_count` is the opcode tally of the benchmarked block taken from `_info.metadata.opcode_count_per_block`, and `target_opcode` is the opcode the benchmark stresses taken from `_info.metadata.target_opcode`. Either key is omitted when the source fixture supplies no value.

## Proofs And Verification

Generate and save proofs:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    --action prove \
    --save-proofs my-proofs \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures
```

Verify proofs from a local folder:

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

## Input Dumps

Dump the raw serialized guest inputs used for a run:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    --dump-inputs debug-inputs \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures
```

Use these dumps to inspect the canonical `statelessInputBytes` passed to the guest after fixture loading.
