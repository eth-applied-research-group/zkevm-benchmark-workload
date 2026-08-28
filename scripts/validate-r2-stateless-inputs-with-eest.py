#!/usr/bin/env python3
"""Validate public R2 stateless fixture batches with the EEST spec guest."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import zstandard
except ImportError:
    zstandard = None

DEFAULT_CATALOG_URL = (
    "https://pub-df22334654034ebab51bc096137a59d8.r2.dev/"
    "devnets/glamsterdam-devnet-8"
)
REQUEST_TIMEOUT_SECONDS = 60
DOWNLOAD_CHUNK_SIZE = 1024 * 1024
FAILURE_MARKDOWN_LIMIT = 20
BATCH_MANIFEST_PATH = ".meta/manifest.json"
ARTIFACT_SCHEMA_VERSION = 2


class ValidationError(Exception):
    """Raised when a downloaded artifact is structurally invalid."""


@dataclass(frozen=True)
class BatchEntry:
    """One public batch entry from batches.jsonl."""

    network: str
    batch_start_block: int
    batch_end_block: int
    batch_size: int
    artifact_count: int
    byte_length: int
    sha256: str
    path: str

    @classmethod
    def from_json(cls, value: dict[str, Any]) -> "BatchEntry":
        return cls(
            network=str(value["network"]),
            batch_start_block=int(value["batchStartBlock"]),
            batch_end_block=int(value["batchEndBlock"]),
            batch_size=int(value["batchSize"]),
            artifact_count=int(value["artifactCount"]),
            byte_length=int(value["byteLength"]),
            sha256=str(value["sha256"]),
            path=str(value["path"]),
        )

    def as_summary(self) -> dict[str, Any]:
        return {
            "network": self.network,
            "batchStartBlock": self.batch_start_block,
            "batchEndBlock": self.batch_end_block,
            "batchSize": self.batch_size,
            "artifactCount": self.artifact_count,
            "byteLength": self.byte_length,
            "sha256": self.sha256,
            "path": self.path,
        }


@dataclass(frozen=True)
class EestGuest:
    """EEST Amsterdam guest functions."""

    bytes_type: Any
    run_stateless_guest: Any
    deserialize_stateless_output: Any
    deserialize_stateless_input: Any


def main() -> int:
    args = parse_args()
    started_at = time.monotonic()
    try:
        summary = run_validation(args, started_at)
        exit_code = 1 if summary["totals"]["failures"] else 0
    except Exception as error:
        summary = fatal_summary(args, started_at, error)
        exit_code = 1

    write_outputs(summary, args)
    print(render_console_summary(summary), end="")
    return exit_code


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate R2 stateless fixture batches with EEST",
    )
    parser.add_argument(
        "--catalog-url",
        default=DEFAULT_CATALOG_URL,
        help="Public catalog root URL, index.html URL, or manifest.json URL",
    )
    parser.add_argument(
        "--batch-count",
        default=1,
        type=positive_int,
        help="Number of latest complete batches to validate",
    )
    parser.add_argument(
        "--max-artifacts",
        type=positive_int,
        help="Stop after validating this many matching artifacts",
    )
    parser.add_argument(
        "--block-number",
        type=non_negative_int,
        help="Validate artifacts for one block number; selects its batch",
    )
    parser.add_argument(
        "--summary-json",
        required=True,
        type=Path,
        help="Path to write machine-readable validation summary",
    )
    parser.add_argument(
        "--summary-md",
        required=True,
        type=Path,
        help="Path to write Markdown validation summary",
    )
    parser.add_argument(
        "--eest-ref",
        default="unknown",
        help="EEST ref used by the caller, recorded for provenance",
    )
    parser.add_argument(
        "--eest-commit",
        default="unknown",
        help="EEST commit used by the caller, recorded for provenance",
    )
    return parser.parse_args()


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def non_negative_int(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be zero or greater")
    return parsed


def run_validation(args: argparse.Namespace, started_at: float) -> dict[str, Any]:
    catalog_base_url = normalize_catalog_url(args.catalog_url)
    manifest_url = urllib.parse.urljoin(catalog_base_url, "manifest.json")
    manifest = fetch_json(manifest_url)
    batches_path = manifest.get("paths", {}).get("batches", "batches.jsonl")
    batches_url = urllib.parse.urljoin(catalog_base_url, batches_path)
    batches = fetch_batches(batches_url)
    selected_batches = select_batches(
        batches,
        args.batch_count,
        args.block_number,
    )
    if zstandard is None:
        raise RuntimeError("Python package 'zstandard' is required")
    guest = load_eest_guest()

    print(f"Catalog: {catalog_base_url}")
    print(f"EEST ref: {args.eest_ref}")
    print(f"EEST commit: {args.eest_commit}")
    print(f"Selected {len(selected_batches)} batch(es)")
    if args.block_number is not None:
        print(f"Block filter: {args.block_number}")
    if args.max_artifacts is not None:
        print(f"Artifact limit: {args.max_artifacts}")

    all_failures: list[dict[str, Any]] = []
    batch_summaries: list[dict[str, Any]] = []
    processed_batches: list[BatchEntry] = []
    total_artifacts = 0
    total_successes = 0
    remaining_artifacts = args.max_artifacts

    with tempfile.TemporaryDirectory(prefix="eest-r2-stateless-") as temp_dir:
        temp_root = Path(temp_dir)
        for batch in selected_batches:
            batch_summary, failures = validate_batch(
                catalog_base_url,
                batch,
                guest,
                temp_root,
                block_number=args.block_number,
                max_artifacts=remaining_artifacts,
            )
            processed_batches.append(batch)
            batch_summaries.append(batch_summary)
            all_failures.extend(failures)
            total_artifacts += batch_summary["artifactsValidated"]
            total_successes += batch_summary["successfulArtifacts"]
            if remaining_artifacts is not None:
                remaining_artifacts -= batch_summary["artifactsValidated"]
                if remaining_artifacts <= 0:
                    break

    return {
        "catalogUrl": catalog_base_url,
        "manifestUrl": manifest_url,
        "batchesUrl": batches_url,
        "eest": {
            "ref": args.eest_ref,
            "commit": args.eest_commit,
        },
        "selection": {
            "batchCount": args.batch_count,
            "maxArtifacts": args.max_artifacts,
            "blockNumber": args.block_number,
        },
        "selectedBatches": [batch.as_summary() for batch in processed_batches],
        "batches": batch_summaries,
        "failures": all_failures,
        "totals": {
            "selectedBatches": len(selected_batches),
            "artifactsValidated": total_artifacts,
            "successfulArtifacts": total_successes,
            "failures": len(all_failures),
            "durationSeconds": round(time.monotonic() - started_at, 3),
        },
    }


def load_eest_guest() -> EestGuest:
    try:
        from ethereum.forks.amsterdam.stateless_guest import (
            deserialize_stateless_input,
            run_stateless_guest,
        )
        from ethereum.forks.amsterdam.stateless_host import (
            deserialize_stateless_output,
        )
        from ethereum_types.bytes import Bytes
    except Exception as error:
        raise RuntimeError(
            "failed to import EEST Amsterdam stateless guest modules"
        ) from error

    return EestGuest(
        bytes_type=Bytes,
        run_stateless_guest=run_stateless_guest,
        deserialize_stateless_output=deserialize_stateless_output,
        deserialize_stateless_input=deserialize_stateless_input,
    )


def normalize_catalog_url(catalog_url: str) -> str:
    catalog_url = catalog_url.strip()
    if not catalog_url:
        raise ValidationError("catalog URL must not be empty")
    for suffix in ("/index.html", "/manifest.json", "/batches.jsonl"):
        if catalog_url.endswith(suffix):
            catalog_url = catalog_url[: -len(suffix)]
            break
    return catalog_url.rstrip("/") + "/"


def fetch_json(url: str) -> dict[str, Any]:
    data = fetch_bytes(url)
    try:
        value = json.loads(data)
    except json.JSONDecodeError as error:
        raise ValidationError(f"failed to decode JSON from {url}") from error
    if not isinstance(value, dict):
        raise ValidationError(f"expected JSON object from {url}")
    return value


def fetch_batches(url: str) -> list[BatchEntry]:
    data = fetch_bytes(url)
    batches = []
    for line_number, raw_line in enumerate(data.decode().splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        try:
            value = json.loads(line)
            batches.append(BatchEntry.from_json(value))
        except Exception as error:
            raise ValidationError(
                f"failed to parse batch entry at {url}:{line_number}"
            ) from error
    if not batches:
        raise ValidationError(f"no batches found in {url}")
    return batches


def fetch_bytes(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": user_agent()})
    with urllib.request.urlopen(
        request,
        timeout=REQUEST_TIMEOUT_SECONDS,
    ) as response:
        return response.read()


def select_batches(
    batches: list[BatchEntry],
    batch_count: int,
    block_number: int | None,
) -> list[BatchEntry]:
    if block_number is not None:
        containing_batches = [
            batch
            for batch in batches
            if batch.batch_start_block <= block_number <= batch.batch_end_block
        ]
        if not containing_batches:
            raise ValidationError(
                f"block {block_number} is not covered by any batch"
            )
        return sorted(
            containing_batches,
            key=lambda batch: (batch.batch_end_block, batch.batch_start_block),
        )

    ordered = sorted(
        batches,
        key=lambda batch: (batch.batch_end_block, batch.batch_start_block),
    )
    return ordered[-batch_count:]


def validate_batch(
    catalog_base_url: str,
    batch: BatchEntry,
    guest: EestGuest,
    temp_root: Path,
    block_number: int | None,
    max_artifacts: int | None,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    batch_url = urllib.parse.urljoin(catalog_base_url, batch.path)
    archive_name = batch.path.rsplit("/", maxsplit=1)[-1]
    archive_path = temp_root / archive_name
    print(
        "Validating "
        f"{batch.path} ({batch.batch_start_block}-{batch.batch_end_block})"
    )
    downloaded_sha256, downloaded_byte_length = download_file(
        batch_url,
        archive_path,
    )
    expected_sha256 = normalize_sha256(batch.sha256)
    failures: list[dict[str, Any]] = []

    if downloaded_sha256 != expected_sha256:
        failures.append(
            batch_failure(
                batch,
                "archive",
                (
                    "batch SHA-256 mismatch: "
                    f"expected {expected_sha256}, got {downloaded_sha256}"
                ),
            )
        )
    if downloaded_byte_length != batch.byte_length:
        failures.append(
            batch_failure(
                batch,
                "archive",
                (
                    "batch byte length mismatch: "
                    f"expected {batch.byte_length}, got {downloaded_byte_length}"
                ),
            )
        )

    artifact_count = 0
    successful_artifacts = 0
    batch_manifest: dict[str, Any] | None = None
    observed_artifacts: dict[str, dict[str, Any]] = {}
    stopped_early = False
    full_batch_validation = block_number is None and max_artifacts is None

    with archive_path.open("rb") as archive_file:
        reader = zstandard.ZstdDecompressor().stream_reader(archive_file)
        with reader, tarfile.open(fileobj=reader, mode="r|") as archive:
            for member in archive:
                if not member.isfile():
                    continue
                member_name = member.name
                extracted = archive.extractfile(member)
                if extracted is None:
                    continue
                if member_name == BATCH_MANIFEST_PATH:
                    batch_manifest = read_json_member(extracted, member_name)
                elif is_artifact_member(member_name):
                    if block_number is not None:
                        member_block_number = block_number_from_member_name(
                            member_name
                        )
                        if (
                            member_block_number is not None
                            and member_block_number != block_number
                        ):
                            continue

                    artifact_count += 1
                    artifact = None
                    try:
                        artifact, fixture_bytes = read_artifact_member(
                            extracted,
                            member_name,
                        )
                        if (
                            block_number is not None
                            and fixture_block_number(artifact) != block_number
                        ):
                            artifact_count -= 1
                            continue
                        validate_artifact(batch, member_name, artifact, guest)
                        observed_artifacts[member_name] = {
                            "archivePath": member_name,
                            "fixtureByteLength": len(fixture_bytes),
                            "fixtureSha256": "0x"
                            + hashlib.sha256(fixture_bytes).hexdigest(),
                        }
                        successful_artifacts += 1
                    except Exception as error:
                        failures.append(
                            artifact_failure(batch, member_name, error, artifact)
                        )

                    if max_artifacts is not None and artifact_count >= max_artifacts:
                        stopped_early = True
                        break

    if block_number is not None and artifact_count == 0:
        failures.append(
            batch_failure(
                batch,
                "archive",
                f"blockNumber {block_number} not found in batch archive",
            )
        )
    if full_batch_validation:
        failures.extend(
            validate_batch_manifest(
                batch,
                batch_manifest,
                artifact_count,
                observed_artifacts,
            )
        )

    return (
        {
            **batch.as_summary(),
            "url": batch_url,
            "downloadedByteLength": downloaded_byte_length,
            "downloadedSha256": "0x" + downloaded_sha256,
            "artifactsValidated": artifact_count,
            "successfulArtifacts": successful_artifacts,
            "batchManifestValidated": full_batch_validation,
            "stoppedEarly": stopped_early,
            "failures": len(failures),
        },
        failures,
    )


def download_file(url: str, path: Path) -> tuple[str, int]:
    request = urllib.request.Request(url, headers={"User-Agent": user_agent()})
    hasher = hashlib.sha256()
    total_bytes = 0
    with urllib.request.urlopen(
        request,
        timeout=REQUEST_TIMEOUT_SECONDS,
    ) as response:
        with path.open("wb") as output:
            while True:
                chunk = response.read(DOWNLOAD_CHUNK_SIZE)
                if not chunk:
                    break
                output.write(chunk)
                hasher.update(chunk)
                total_bytes += len(chunk)
    return hasher.hexdigest(), total_bytes


def is_artifact_member(member_name: str) -> bool:
    return (
        member_name.startswith("blockchain_tests/")
        and member_name.endswith(".json")
    )


def block_number_from_member_name(member_name: str) -> int | None:
    filename = member_name.rsplit("/", maxsplit=1)[-1]
    block_number, separator, _ = filename.partition("-")
    if not separator:
        return None
    try:
        return int(block_number)
    except ValueError:
        return None


def read_json_member(member_file: Any, member_name: str) -> dict[str, Any]:
    try:
        value = json.load(member_file)
    except json.JSONDecodeError as error:
        raise ValidationError(f"failed to decode {member_name} JSON") from error
    if not isinstance(value, dict):
        raise ValidationError(f"{member_name} must contain a JSON object")
    return value


def read_artifact_member(
    member_file: Any,
    member_name: str,
) -> tuple[dict[str, Any], bytes]:
    data = member_file.read()
    try:
        value = json.loads(data)
    except json.JSONDecodeError as error:
        raise ValidationError(f"failed to decode {member_name} JSON") from error
    if not isinstance(value, dict):
        raise ValidationError(f"{member_name} must contain a JSON object")
    return value, data


def validate_artifact(
    batch: BatchEntry,
    archive_path: str,
    artifact: dict[str, Any],
    guest: EestGuest,
) -> None:
    _, test, block, metadata = fixture_parts(artifact)
    if test.get("network") != "Amsterdam":
        raise ValidationError("EEST fixture network must be Amsterdam")
    if metadata.get("schemaVersion") != ARTIFACT_SCHEMA_VERSION:
        raise ValidationError(
            "unsupported witness_generator schemaVersion: "
            f"expected {ARTIFACT_SCHEMA_VERSION}, "
            f"got {metadata.get('schemaVersion')}"
        )
    if metadata.get("network") != batch.network:
        raise ValidationError(
            "witness_generator.network mismatch: "
            f"expected {batch.network}, got {metadata.get('network')}"
        )

    input_bytes = decode_hex_bytes(
        "statelessInputBytes",
        block.get("statelessInputBytes"),
    )
    if not input_bytes:
        raise ValidationError("statelessInputBytes must not be empty")
    expected_length = int_required(
        "statelessInputByteLength",
        metadata.get("statelessInputByteLength"),
    )
    if len(input_bytes) != expected_length:
        raise ValidationError(
            "statelessInputByteLength mismatch: "
            f"expected {expected_length}, got {len(input_bytes)}"
        )

    schema_id = str_required(
        "statelessInputSchemaId",
        metadata.get("statelessInputSchemaId"),
    ).lower()
    actual_schema_id = "0x" + input_bytes[:2].hex()
    if len(input_bytes) < 2 or schema_id != actual_schema_id:
        raise ValidationError(
            "statelessInputSchemaId mismatch: "
            f"expected {schema_id}, got {actual_schema_id}"
        )

    block_number = fixture_block_number(artifact)
    if block_number < batch.batch_start_block or block_number > batch.batch_end_block:
        raise ValidationError(
            "blockNumber outside selected batch range: "
            f"{block_number} not in "
            f"{batch.batch_start_block}-{batch.batch_end_block}"
        )

    expected_output_bytes = decode_hex_bytes(
        "statelessOutputBytes",
        block.get("statelessOutputBytes"),
    )
    if not expected_output_bytes:
        raise ValidationError("statelessOutputBytes must not be empty")
    expected_output = guest.deserialize_stateless_output(
        guest.bytes_type(expected_output_bytes)
    )
    if not bool(expected_output.successful_validation):
        raise ValidationError(
            "stored statelessOutputBytes does not expect successful validation "
            f"({stateless_output_diagnostics(expected_output)})"
        )
    config = test.get("config")
    if not isinstance(config, dict):
        raise ValidationError("EEST fixture is missing config")
    chain_id = parse_json_u64("config.chainid", config.get("chainid"))
    input_schema_id = int.from_bytes(input_bytes[:2], "big")
    if int(expected_output.chain_id) != chain_id:
        raise ValidationError(
            "statelessOutputBytes chain ID mismatch: "
            f"expected {chain_id}, got {int(expected_output.chain_id)}"
        )
    if int(expected_output.schema_id) != input_schema_id:
        raise ValidationError(
            "statelessOutputBytes schema ID mismatch: "
            f"expected 0x{input_schema_id:04x}, "
            f"got 0x{int(expected_output.schema_id):04x}"
        )

    actual_output_bytes = bytes(
        guest.run_stateless_guest(guest.bytes_type(input_bytes))
    )
    actual_output = guest.deserialize_stateless_output(
        guest.bytes_type(actual_output_bytes)
    )
    if actual_output_bytes != expected_output_bytes:
        raise ValidationError(
            "statelessOutputBytes mismatch: "
            f"expected {stateless_output_diagnostics(expected_output)}, "
            f"got {stateless_output_diagnostics(actual_output)}; "
            f"{stateless_input_diagnostics(input_bytes, guest)}"
        )
    if not bool(actual_output.successful_validation):
        raise ValidationError(
            "EEST stateless guest returned unsuccessful_validation "
            f"({stateless_output_diagnostics(actual_output)}; "
            f"{stateless_input_diagnostics(input_bytes, guest)})"
        )


def fixture_parts(
    artifact: dict[str, Any],
) -> tuple[str, dict[str, Any], dict[str, Any], dict[str, Any]]:
    if len(artifact) != 1:
        raise ValidationError("schema-v2 EEST fixture must contain exactly one test")
    test_name, test = next(iter(artifact.items()))
    if not isinstance(test, dict):
        raise ValidationError(f"EEST test {test_name} must be an object")
    blocks = test.get("blocks")
    if not isinstance(blocks, list) or len(blocks) != 1:
        raise ValidationError(
            f"EEST test {test_name} must contain exactly one block"
        )
    block = blocks[0]
    if not isinstance(block, dict):
        raise ValidationError(f"EEST test {test_name} block must be an object")
    info = test.get("_info")
    if not isinstance(info, dict):
        raise ValidationError(f"EEST test {test_name} is missing _info")
    metadata_container = info.get("metadata")
    if not isinstance(metadata_container, dict):
        raise ValidationError(
            f"EEST test {test_name} is missing _info.metadata"
        )
    metadata = metadata_container.get("witness_generator")
    if not isinstance(metadata, dict):
        raise ValidationError(
            f"EEST test {test_name} is missing witness_generator metadata"
        )
    return test_name, test, block, metadata


def fixture_block_number(artifact: dict[str, Any]) -> int:
    _, _, block, _ = fixture_parts(artifact)
    block_header = block.get("blockHeader")
    if not isinstance(block_header, dict):
        raise ValidationError("EEST fixture block is missing blockHeader")
    return parse_json_u64(
        "blockHeader.number",
        block_header.get("number"),
    )


def stateless_output_diagnostics(output: Any) -> str:
    output_root = bytes(output.new_payload_request_root).hex()
    return (
        f"newPayloadRequestRoot=0x{output_root}, "
        f"successfulValidation={bool(output.successful_validation)}, "
        f"chainId={int(output.chain_id)}, "
        f"schemaId=0x{int(output.schema_id):04x}"
    )


def stateless_input_diagnostics(input_bytes: bytes, guest: EestGuest) -> str:
    try:
        stateless_input = guest.deserialize_stateless_input(
            guest.bytes_type(input_bytes)
        )
        payload = stateless_input.new_payload_request.execution_payload
        schema_id = int.from_bytes(input_bytes[:2], "big")
        return (
            f"chainId={int(stateless_input.chain_id)}, "
            f"schemaId=0x{schema_id:04x}, "
            f"payloadBlockNumber={int(payload.block_number)}, "
            f"payloadBlockHash=0x{bytes(payload.block_hash).hex()}"
        )
    except Exception as error:
        return (
            "statelessInputDiagnostics="
            f"{type(error).__name__}: {error}"
        )


def validate_batch_manifest(
    batch: BatchEntry,
    manifest: dict[str, Any] | None,
    artifact_count: int,
    observed_artifacts: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    failures = []
    if manifest is None:
        return [
            batch_failure(
                batch,
                BATCH_MANIFEST_PATH,
                "missing batch manifest",
            )
        ]

    checks = [
        ("schemaVersion", ARTIFACT_SCHEMA_VERSION),
        ("network", batch.network),
        ("batchStartBlock", batch.batch_start_block),
        ("batchEndBlock", batch.batch_end_block),
        ("batchSize", batch.batch_size),
        ("artifactCount", batch.artifact_count),
    ]
    for field_name, expected in checks:
        actual = manifest.get(field_name)
        if actual != expected:
            failures.append(
                batch_failure(
                    batch,
                    BATCH_MANIFEST_PATH,
                    f"{field_name} mismatch: expected {expected}, got {actual}",
                )
            )
    if artifact_count != batch.artifact_count:
        failures.append(
            batch_failure(
                batch,
                BATCH_MANIFEST_PATH,
                (
                    "artifact count mismatch: "
                    f"expected {batch.artifact_count}, got {artifact_count}"
                ),
            )
        )
    manifest_artifacts = manifest.get("artifacts")
    if not isinstance(manifest_artifacts, list):
        failures.append(
            batch_failure(
                batch,
                BATCH_MANIFEST_PATH,
                "artifacts must be an array",
            )
        )
        return failures
    by_path = {
        entry.get("archivePath"): entry
        for entry in manifest_artifacts
        if isinstance(entry, dict) and isinstance(entry.get("archivePath"), str)
    }
    for archive_path, observed in observed_artifacts.items():
        expected = by_path.get(archive_path)
        if expected is None:
            failures.append(
                batch_failure(
                    batch,
                    BATCH_MANIFEST_PATH,
                    f"missing manifest artifact entry for {archive_path}",
                )
            )
            continue
        for field_name in ("fixtureByteLength", "fixtureSha256"):
            if expected.get(field_name) != observed[field_name]:
                failures.append(
                    batch_failure(
                        batch,
                        BATCH_MANIFEST_PATH,
                        (
                            f"{archive_path} {field_name} mismatch: "
                            f"expected {expected.get(field_name)}, "
                            f"got {observed[field_name]}"
                        ),
                    )
                )
    return failures


def parse_json_u64(field_name: str, value: Any) -> int:
    raw = str_required(field_name, value).strip()
    try:
        return int(raw, 16 if raw.lower().startswith("0x") else 10)
    except ValueError as error:
        raise ValidationError(f"{field_name} is not a valid integer") from error


def decode_hex_bytes(field_name: str, value: Any) -> bytes:
    raw = str_required(field_name, value)
    hex_data = raw[2:] if raw.startswith(("0x", "0X")) else raw
    if len(hex_data) % 2:
        raise ValidationError(f"{field_name} must have an even hex length")
    try:
        return bytes.fromhex(hex_data)
    except ValueError as error:
        raise ValidationError(f"{field_name} is not valid hex") from error


def str_required(field_name: str, value: Any) -> str:
    if not isinstance(value, str) or not value:
        raise ValidationError(f"{field_name} must be a non-empty string")
    return value


def int_required(field_name: str, value: Any) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValidationError(f"{field_name} must be an integer")
    return value


def normalize_sha256(value: str) -> str:
    digest = value[2:] if value.startswith(("0x", "0X")) else value
    digest = digest.lower()
    if len(digest) != 64:
        raise ValidationError(f"invalid SHA-256 length for {value}")
    try:
        bytes.fromhex(digest)
    except ValueError as error:
        raise ValidationError(f"invalid SHA-256 hex for {value}") from error
    return digest


def artifact_failure(
    batch: BatchEntry,
    archive_path: str,
    error: Exception,
    artifact: dict[str, Any] | None = None,
) -> dict[str, Any]:
    failure = batch_failure(
        batch,
        archive_path,
        f"{type(error).__name__}: {error}",
    )
    if artifact is not None:
        try:
            _, _, _, metadata = fixture_parts(artifact)
            failure["blockNumber"] = fixture_block_number(artifact)
            failure["blockHash"] = metadata.get("blockHash")
        except Exception:
            pass
    return failure


def batch_failure(
    batch: BatchEntry,
    archive_path: str,
    error: str,
) -> dict[str, Any]:
    return {
        "batchPath": batch.path,
        "batchStartBlock": batch.batch_start_block,
        "batchEndBlock": batch.batch_end_block,
        "archivePath": archive_path,
        "error": error,
    }


def fatal_summary(
    args: argparse.Namespace,
    started_at: float,
    error: Exception,
) -> dict[str, Any]:
    return {
        "catalogUrl": args.catalog_url,
        "eest": {
            "ref": args.eest_ref,
            "commit": args.eest_commit,
        },
        "selectedBatches": [],
        "batches": [],
        "failures": [],
        "selection": {
            "batchCount": args.batch_count,
            "maxArtifacts": args.max_artifacts,
            "blockNumber": args.block_number,
        },
        "fatalError": {
            "type": type(error).__name__,
            "message": str(error),
        },
        "totals": {
            "selectedBatches": 0,
            "artifactsValidated": 0,
            "successfulArtifacts": 0,
            "failures": 1,
            "durationSeconds": round(time.monotonic() - started_at, 3),
        },
    }


def write_outputs(summary: dict[str, Any], args: argparse.Namespace) -> None:
    args.summary_json.parent.mkdir(parents=True, exist_ok=True)
    args.summary_md.parent.mkdir(parents=True, exist_ok=True)
    args.summary_json.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    args.summary_md.write_text(render_markdown_summary(summary), encoding="utf-8")


def render_console_summary(summary: dict[str, Any]) -> str:
    totals = summary["totals"]
    lines = [
        "",
        "Validation summary:",
        f"  batches: {totals['selectedBatches']}",
        f"  artifacts validated: {totals['artifactsValidated']}",
        f"  successful artifacts: {totals['successfulArtifacts']}",
        f"  failures: {totals['failures']}",
        f"  duration seconds: {totals['durationSeconds']}",
    ]
    if "fatalError" in summary:
        fatal_error = summary["fatalError"]
        lines.append(
            f"  fatal error: {fatal_error['type']}: {fatal_error['message']}"
        )
    return "\n".join(lines) + "\n"


def render_markdown_summary(summary: dict[str, Any]) -> str:
    totals = summary["totals"]
    eest = summary["eest"]
    lines = [
        "# EEST R2 Stateless Fixture Validation",
        "",
        f"- Catalog: `{summary['catalogUrl']}`",
        f"- EEST ref: `{eest['ref']}`",
        f"- EEST commit: `{eest['commit']}`",
        f"- Selected batches: `{totals['selectedBatches']}`",
        f"- Artifacts validated: `{totals['artifactsValidated']}`",
        f"- Successful artifacts: `{totals['successfulArtifacts']}`",
        f"- Failures: `{totals['failures']}`",
        f"- Duration seconds: `{totals['durationSeconds']}`",
        "",
    ]
    selection = summary.get("selection", {})
    if selection:
        lines.extend(
            [
                "## Selection",
                "",
                f"- Batch count: `{selection.get('batchCount')}`",
                f"- Max artifacts: `{selection.get('maxArtifacts')}`",
                f"- Block number: `{selection.get('blockNumber')}`",
                "",
            ]
        )

    if "fatalError" in summary:
        fatal_error = summary["fatalError"]
        lines.extend(
            [
                "## Fatal Error",
                "",
                f"`{fatal_error['type']}: {fatal_error['message']}`",
                "",
            ]
        )

    if summary["batches"]:
        lines.extend(
            [
                "## Batches",
                "",
                (
                    "| Batch | Artifacts | Successful | Failures | "
                    "Downloaded bytes | SHA-256 |"
                ),
                "| --- | ---: | ---: | ---: | ---: | --- |",
            ]
        )
        for batch in summary["batches"]:
            lines.append(
                "| "
                f"{markdown_cell(batch['path'])} | "
                f"{batch['artifactsValidated']} | "
                f"{batch['successfulArtifacts']} | "
                f"{batch['failures']} | "
                f"{batch['downloadedByteLength']} | "
                f"`{short_hash(batch['downloadedSha256'])}` |"
            )
        lines.append("")

    if summary["failures"]:
        lines.extend(["## Failures", ""])
        lines.extend(
            [
                "| Batch | Block | Block hash | Archive path | Error |",
                "| --- | ---: | --- | --- | --- |",
            ]
        )
        for failure in summary["failures"][:FAILURE_MARKDOWN_LIMIT]:
            lines.append(
                "| "
                f"{markdown_cell(failure['batchPath'])} | "
                f"{markdown_cell(failure.get('blockNumber', ''))} | "
                f"{markdown_cell(short_hash_or_empty(failure.get('blockHash')))} | "
                f"{markdown_cell(failure['archivePath'])} | "
                f"{markdown_cell(truncate(failure['error'], 220))} |"
            )
        remaining = len(summary["failures"]) - FAILURE_MARKDOWN_LIMIT
        if remaining > 0:
            lines.append("")
            lines.append(
                f"Showing first {FAILURE_MARKDOWN_LIMIT} failures; "
                f"{remaining} more are in the JSON artifact."
            )
        lines.append("")

    return "\n".join(lines)


def markdown_cell(value: Any) -> str:
    return str(value).replace("\n", " ").replace("|", "\\|")


def truncate(value: str, max_length: int) -> str:
    if len(value) <= max_length:
        return value
    return value[: max_length - 3] + "..."


def short_hash(value: str) -> str:
    digest = value[2:] if value.startswith("0x") else value
    return "0x" + digest[:16] + "..."


def short_hash_or_empty(value: Any) -> str:
    if not value:
        return ""
    return short_hash(str(value))


def user_agent() -> str:
    return "zkevm-benchmark-workload-eest-r2-validator/1.0"


if __name__ == "__main__":
    sys.exit(main())
