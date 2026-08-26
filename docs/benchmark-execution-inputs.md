# Benchmark Execution Inputs

This is the detailed input reference for the `ere-hosts stateless-validator` workload. For commands and proof operations, see [Benchmark Execution](benchmark-execution.md).

## Action-Aware Input Requirement

Execute and prove actions require:

```text
stateless-validator --input-folder <PATH>
```

There is no default input location. The path must exist and may identify:

- One canonical EEST `.json` fixture file.
- A directory containing canonical EEST `.json` fixture files.
- An EEST fixture checkout or archive root containing `blockchain_tests/`.

Verification does not need fixture input. If `--input-folder` is supplied with `--action verify`, the option is accepted and ignored for backward compatibility, including when its path no longer exists.

## Discovery

Directory input is walked recursively in sorted filename order. Only `.json` files are considered, and files below `.meta/` are excluded.

When the input path contains a `blockchain_tests/` subdirectory, only that subtree is used. This preference prevents engine, sync, or metadata JSON from entering stateless validation. EEST bundles that contain only `blockchain_tests_engine/`, `blockchain_tests_engine_x/`, or `blockchain_tests_sync/` are rejected because those formats do not provide the canonical stateless validator bytes.

An empty directory retains the existing discovery behavior: it produces no fixture paths.

Batch archives exported by `witness-generator-spec-cli` use this layout. After
extracting one, pass the extraction root as `--input-folder`; discovery selects
its `blockchain_tests/` subtree and ignores `.meta/manifest.json`.

## Canonical EEST Schema

The only accepted benchmark fixture format is an EEST `blockchain_tests` JSON object whose executable blocks contain `statelessInputBytes` and `statelessOutputBytes`:

```json
{
  "tests/foo.py::test_case[param]": {
    "network": "Amsterdam",
    "config": {
      "chainid": "0x01"
    },
    "blocks": [
      {
        "statelessInputBytes": "0x150102",
        "statelessOutputBytes": "0xaabb",
        "blockHeader": {
          "number": "0x01",
          "gasUsed": "0x10"
        }
      }
    ],
    "_info": {
      "metadata": {
        "opcode_count_per_block": [
          {
            "PUSH1": 5,
            "SSTORE": 2
          }
        ],
        "target_opcode": "MCOPY"
      }
    }
  }
}
```

Rules:

- The file is a JSON object keyed by the original EEST test name.
- Each test case includes `network`, `config.chainid`, and `blocks`; unrelated EEST fields are ignored.
- `config.chainid`, `blockHeader.number`, `blocknumber`, and `blockHeader.gasUsed` may be decimal strings or `0x`-prefixed hexadecimal strings.
- Only the last block of a test case is loaded, because EEST benchmark tests place the worst case block last and use any preceding blocks for setup.
- A block without `statelessInputBytes`, or with empty `statelessInputBytes`, is skipped.
- A block with non-empty `statelessInputBytes` must also contain `statelessOutputBytes`.
- Both byte fields are hexadecimal strings with an optional `0x` prefix and an even number of hexadecimal digits after that prefix.
- `_info.metadata.opcode_count_per_block` holds one opcode-count map per block in block order, so entry N describes `blocks[N]` and the last entry describes the loaded block. A present array whose length differs from the block count is rejected.
- `_info.metadata.target_opcode` names the opcode a benchmark stresses. Benchmarks that stress no single opcode omit it.

Each accepted block becomes one benchmark fixture. Its safe output name is derived from the original EEST test name, block index, and source context; collisions are disambiguated. The original test name remains available for fixture-prefix selection.

## Execution-Client Routing

Reth receives `statelessInputBytes` unchanged on stdin and uses
`statelessOutputBytes` as the expected public values. Ethrex and Zesu remain
valid CLI choices so their routing can be restored without reconstructing the
interface, but they currently fail before artifact resolution because
`ere-guests` has no compatible tests-zkevm v0.8.2 releases for them.

Fixture deserialization remains independent of the selected execution client.
Client-specific availability is checked before artifact resolution or guest
execution.

## Fixture Selection

Repeat `--fixture <PREFIX>` to select one or more fixture-name prefixes:

```bash
cargo run -p ere-hosts --release -- --zkvms sp1 \
    stateless-validator --execution-client reth \
    --input-folder /path/to/eest-fixtures \
    --fixture test_sha256.py::test_sha256 \
    --fixture test_memory.py::test_mcopy
```

A prefix may match either the sanitized fixture name or the original EEST test name. A `.json` suffix is ignored during prefix normalization, repeated prefixes are deduplicated, and empty prefixes are rejected.

## Metadata, Existing Outputs, And Public Values

Benchmark metadata preserves the fixture format, original test name, source path, block index, network, chain ID, block number, gas used, the block's opcode count, and the target opcode. See [Benchmark Execution Output](benchmark-execution-output.md#metadata-by-workload) for the serialized shape.

Unless `--force-rerun` is set, fixture preparation skips cases whose metrics output already exists. Execution and proving both compare the guest's public values with the fixture's raw `statelessOutputBytes`; proof verification retains the existing stored-proof verification behavior.

## Legacy Format Rejection

JSON with a top-level `stateless_input` field is rejected before canonical deserialization with this migration error:

```text
legacy fixture format with top-level stateless_input is no longer supported; provide an EEST blockchain_tests fixture containing statelessInputBytes and statelessOutputBytes
```

The old fixture generator crates and image are discontinued. Existing fixture, metric, proof, and published image files remain untouched, but legacy fixture JSON must be replaced with canonical EEST `blockchain_tests` input before it can be benchmarked.
