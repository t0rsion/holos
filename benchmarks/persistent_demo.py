#!/usr/bin/env python3
"""Produce, independently check, and plot the small persistent H1 demo.

Build the extension and checker before running this script::

    maturin develop --release --manifest-path crates/holos-tda-py/Cargo.toml
    cargo build --release -p holos-tda-check

Run it from the repository root with::

    HOLOS_CHECK_BIN=target/release/holos-check \
        python benchmarks/persistent_demo.py

The default output directory is local and ignored. The source graph and four
checked artifacts use eight synthetic vertices. The record separates stale
digest integrity controls from rehashed semantic mutations.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import struct
import subprocess
import sys
from typing import Optional

import holos_tda

from v09_bipersistence.evidence import sha256


MODULUS = 47
THRESHOLD = 5.0
VERTEX_COUNT = 8
EDGES = (
    (0, 1, 1.0),
    (1, 2, 1.0),
    (2, 3, 1.0),
    (0, 3, 1.0),
    (0, 2, 4.0),
    (4, 5, 2.0),
    (5, 6, 2.0),
    (6, 7, 2.0),
    (4, 7, 2.0),
    (4, 6, 5.0),
)
POSITIONS = {
    0: (0.0, 1.0),
    1: (1.0, 1.0),
    2: (1.0, 0.0),
    3: (0.0, 0.0),
    4: (3.0, 1.0),
    5: (4.0, 1.0),
    6: (4.0, 0.0),
    7: (3.0, 0.0),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="write and check the synthetic overlapping-bar demo"
    )
    parser.add_argument(
        "--checker",
        type=Path,
        default=Path(os.environ.get("HOLOS_CHECK_BIN", "target/release/holos-check")),
        help="independent holos-check executable",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("local/records/persistent_demo"),
        help="directory for the source, artifacts, record, and plots",
    )
    parser.add_argument(
        "--no-plot",
        action="store_true",
        help="skip optional Matplotlib SVG and PNG output",
    )
    return parser.parse_args()


def write_source(path: Path) -> None:
    lines = [
        "# overlapping-bar fixture",
        "# vertex_count=8",
    ]
    lines.extend(f"{u} {v} {weight:.17g}" for u, v, weight in EDGES)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def interval_overlap(left: dict, right: dict) -> bool:
    left_death = math.inf if left["death"] is None else left["death"]
    right_death = math.inf if right["death"] is None else right["death"]
    return max(left["birth"], right["birth"]) < min(left_death, right_death)


def select_classes(bars, classes) -> tuple[dict, dict]:
    finite = [item for item in classes if item["death"] is not None]
    if len(finite) != 2 or not interval_overlap(finite[0], finite[1]):
        raise RuntimeError(
            "fixture did not produce exactly two finite overlapping H1 classes"
        )
    diagram_intervals = {
        (dimension, birth, death) for dimension, birth, death in bars
    }
    for item in finite:
        interval = (1, item["birth"], item["death"])
        if interval not in diagram_intervals:
            raise RuntimeError("class record is absent from the serialized diagram")
    return finite[0], finite[1]


def class_selector(classes: list[dict], target: dict) -> tuple[int, int]:
    groups = []
    for item in classes:
        if item["group_id"] not in groups:
            groups.append(item["group_id"])
    return groups.index(target["group_id"]), target["basis_index"]


def build_artifacts(classes: list[dict], target: dict) -> tuple[dict, dict]:
    space_index, basis_index = class_selector(classes, target)
    class_result = holos_tda.persistent_class_sparse(
        VERTEX_COUNT,
        EDGES,
        max_dim=1,
        threshold=THRESHOLD,
        modulus=MODULUS,
        threads=1,
        space_index=space_index,
        basis_index=basis_index,
    )
    coordinate_result = holos_tda.persistent_circular_sparse(
        VERTEX_COUNT,
        EDGES,
        max_dim=1,
        threshold=THRESHOLD,
        modulus=MODULUS,
        tolerance=1e-10,
        max_iterations=10_000,
        space_index=space_index,
        basis_index=basis_index,
    )
    if class_result["class"]["id"] != target["id"]:
        raise RuntimeError("persistent-class selector returned another class")
    if coordinate_result["class"]["id"] != target["id"]:
        raise RuntimeError("persistent-circular selector returned another class")
    return class_result, coordinate_result


def check_artifact(checker: Path, path: Path) -> str:
    returncode, response = run_checker(checker, path)
    if returncode != 0:
        raise RuntimeError(
            f"holos-check rejected {path}:\n{response}"
        )
    return response


def write_phase(path: Path, phase: list[float]) -> None:
    rows = [f"{vertex} {value:.17g}" for vertex, value in enumerate(phase)]
    path.write_text("\n".join(rows) + "\n", encoding="utf-8")


def artifact_record(
    checker_output: str,
    path: Path,
    coordinate: Optional[dict] = None,
) -> dict:
    record = {
        "path": path.name,
        "bytes": path.stat().st_size,
        "sha256": sha256(path),
        "checker": checker_output,
    }
    if coordinate is not None:
        record["field_multiplier"] = int(coordinate["field_multiplier"])
        record["divisibility"] = int(coordinate["divisibility"])
        record["relative_residual"] = float(coordinate["relative_residual"])
        record["iterations"] = int(coordinate["iterations"])
    return record


class HolospcReader:
    """Read big-endian fields while retaining the current wire offset."""

    def __init__(self, data: bytes):
        self.data = data
        self.offset = 11

    def skip(self, size: int) -> None:
        self.offset += size

    def u32(self) -> int:
        value = struct.unpack_from(">I", self.data, self.offset)[0]
        self.skip(4)
        return value

    def u64(self) -> int:
        value = struct.unpack_from(">Q", self.data, self.offset)[0]
        self.skip(8)
        return value

    def f64(self) -> float:
        value = struct.unpack_from(">d", self.data, self.offset)[0]
        self.skip(8)
        return value


def put_u32(data: bytearray, offset: int, value: int) -> None:
    data[offset : offset + 4] = struct.pack(">I", value)


def put_u64(data: bytearray, offset: int, value: int) -> None:
    data[offset : offset + 8] = struct.pack(">Q", value)


def put_i64(data: bytearray, offset: int, value: int) -> None:
    data[offset : offset + 8] = struct.pack(">q", value)


def put_f64(data: bytearray, offset: int, value: float) -> None:
    data[offset : offset + 8] = struct.pack(">d", value)


def packed_u64(value: int) -> bytes:
    return struct.pack(">Q", value)


def packed_f64(value: float) -> bytes:
    return struct.pack(">d", value)


def read_u64_at(data: bytes, offset: int) -> int:
    return struct.unpack_from(">Q", data, offset)[0]


def read_i64_at(data: bytes, offset: int) -> int:
    return struct.unpack_from(">q", data, offset)[0]


def read_source_edge(reader: HolospcReader) -> dict:
    offset = reader.offset
    return {
        "u": reader.u64(),
        "v": reader.u64(),
        "value": reader.f64(),
        "u_offset": offset,
        "v_offset": offset + 8,
        "value_offset": offset + 16,
    }


def read_cocycle_term(reader: HolospcReader) -> dict:
    offset = reader.offset
    return {
        "u": reader.u64(),
        "v": reader.u64(),
        "coefficient": reader.u32(),
        "offset": offset,
        "coefficient_offset": offset + 16,
    }


def read_cycle_term(reader: HolospcReader) -> dict:
    return read_cocycle_term(reader)


def read_chain_term(reader: HolospcReader) -> dict:
    offset = reader.offset
    vertices = (reader.u64(), reader.u64(), reader.u64())
    return {
        "vertices": vertices,
        "coefficient": reader.u32(),
        "offset": offset,
        "coefficient_offset": offset + 24,
    }


def check_holospc_prefix(data: bytes, payload_end: int) -> None:
    if payload_end < 31:
        raise RuntimeError("HOLOSPC payload is too short")
    if data[:8] != b"HOLOSPC\0" or data[8:11] != b"\0\x01\x01":
        raise RuntimeError("mutation input is not HOLOSPC version 1")


def read_tagged_f64(data: bytes, reader: HolospcReader) -> None:
    tag = data[reader.offset]
    reader.skip(1)
    if tag == 1:
        reader.skip(8)
    elif tag != 0:
        raise RuntimeError("HOLOSPC threshold tag is not canonical")


def read_graph_header(reader: HolospcReader) -> dict:
    modulus_offset = reader.offset
    modulus = reader.u32()
    vertex_count_offset = reader.offset
    vertex_count = reader.u64()
    source_count = reader.u64()
    source = [read_source_edge(reader) for _ in range(source_count)]
    return {
        "modulus_offset": modulus_offset,
        "modulus": modulus,
        "vertex_count_offset": vertex_count_offset,
        "vertex_count": vertex_count,
        "source": source,
    }


def read_class_header(reader: HolospcReader) -> dict:
    group_id_offset = reader.offset
    reader.skip(32)
    class_id_offset = reader.offset
    reader.skip(32)
    basis_index = reader.u64()
    birth_offset = reader.offset
    birth = reader.f64()
    death_offset = reader.offset
    death = reader.f64()
    scale = reader.f64()
    return {
        "group_id_offset": group_id_offset,
        "class_id_offset": class_id_offset,
        "basis_index": basis_index,
        "birth_offset": birth_offset,
        "birth": birth,
        "death_offset": death_offset,
        "death": death,
        "scale": scale,
    }


def read_witnesses(data: bytes, reader: HolospcReader) -> dict:
    cocycle_count_offset = reader.offset
    cocycle = [read_cocycle_term(reader) for _ in range(reader.u64())]
    pair_birth_offset = reader.offset
    pair_birth_u_offset = reader.offset
    pair_birth_u = reader.u64()
    pair_birth_v_offset = reader.offset
    pair_birth_v = reader.u64()
    death_tag = data[reader.offset]
    reader.skip(1)
    if death_tag == 1:
        reader.skip(24)
    elif death_tag != 0:
        raise RuntimeError("HOLOSPC death tag is not canonical")
    cycle = [read_cycle_term(reader) for _ in range(reader.u64())]
    chain = [read_chain_term(reader) for _ in range(reader.u64())]
    return {
        "cocycle_count_offset": cocycle_count_offset,
        "cocycle": cocycle,
        "pair_birth_offset": pair_birth_offset,
        "pair_birth_u_offset": pair_birth_u_offset,
        "pair_birth_v_offset": pair_birth_v_offset,
        "pair_birth": (pair_birth_u, pair_birth_v),
        "cycle": cycle,
        "chain": chain,
    }


def walk_holospc(data: bytes) -> dict:
    """Return semantic field offsets for a version 1 HOLOSPC payload."""

    payload_end = len(data) - 32
    check_holospc_prefix(data, payload_end)
    reader = HolospcReader(data)
    graph = read_graph_header(reader)
    read_tagged_f64(data, reader)
    class_header = read_class_header(reader)
    witnesses = read_witnesses(data, reader)
    if reader.offset != payload_end:
        raise RuntimeError("HOLOSPC walker did not reach the payload digest")
    return {
        "payload_end": payload_end,
        **graph,
        **class_header,
        **witnesses,
    }


def terms_as_tuples(terms: list[dict]) -> list[tuple[int, int, int]]:
    return [(term["u"], term["v"], term["coefficient"]) for term in terms]


def term_bytes(term: tuple[int, int, int]) -> bytes:
    u, v, coefficient = term
    return packed_u64(u) + packed_u64(v) + struct.pack(">I", coefficient)


def cocycle_bytes(terms: list[tuple[int, int, int]]) -> bytes:
    return packed_u64(len(terms)) + b"".join(term_bytes(term) for term in terms)


def group_digest(
    birth: float,
    death: float,
    modulus: int,
    scale: float,
    terms: list[tuple[int, int, int]],
) -> bytes:
    encoded = b"".join(
        (
            b"holos-h1-class-space-v1",
            packed_u64(1),
            packed_f64(birth),
            packed_f64(death),
            struct.pack(">I", modulus),
            packed_u64(1),
            packed_f64(scale),
            packed_u64(len(terms)),
            b"".join(term_bytes(term) for term in terms),
        )
    )
    return hashlib.sha256(encoded).digest()


def class_digest(
    group: bytes,
    basis_index: int,
    scale: float,
    terms: list[tuple[int, int, int]],
) -> bytes:
    encoded = b"".join(
        (
            b"holos-h1-basis-class-v1",
            group,
            packed_u64(basis_index),
            packed_f64(scale),
            b"".join(term_bytes(term) for term in terms),
        )
    )
    return hashlib.sha256(encoded).digest()


def recompute_ids(data: bytearray) -> dict:
    view = walk_holospc(data)
    if view["basis_index"] != 0:
        raise RuntimeError("the compact demo ID mutator expects basis index zero")
    terms = terms_as_tuples(view["cocycle"])
    group = group_digest(
        view["birth"], view["death"], view["modulus"], view["scale"], terms
    )
    class_id = class_digest(group, 0, view["scale"], terms)
    data[view["group_id_offset"] : view["group_id_offset"] + 32] = group
    data[view["class_id_offset"] : view["class_id_offset"] + 32] = class_id
    return {"group_id": group.hex(), "class_id": class_id.hex()}


def reseal_payload(data: bytearray) -> bytes:
    payload = bytes(data[:-32])
    digest = hashlib.sha256(payload).digest()
    data[-32:] = digest
    if bytes(data[-32:]) != digest:
        raise RuntimeError("failed to reseal the mutation payload")
    return bytes(data)


def stored_payload_digest(data: bytes) -> str:
    return data[-32:].hex()


def check_holosph_prefix(data: bytes, payload_end: int) -> None:
    if payload_end < 27 or data[:8] != b"HOLOSPH\0" or data[8:11] != b"\0\x01\x01":
        raise RuntimeError("mutation input is not HOLOSPH version 1")


def read_coordinate_integral(data: bytes, offset: int) -> dict:
    return {
        "u": read_u64_at(data, offset),
        "v": read_u64_at(data, offset + 8),
        "coefficient": read_i64_at(data, offset + 16),
        "coefficient_offset": offset + 16,
    }


def read_coordinate_potential(data: bytes, offset: int) -> dict:
    return {"value": struct.unpack_from(">d", data, offset)[0], "offset": offset}


def walk_holosph(data: bytes) -> dict:
    """Return semantic field offsets for a version 1 HOLOSPH payload."""

    payload_end = len(data) - 32
    check_holosph_prefix(data, payload_end)
    nested_start = 27
    nested_count = read_u64_at(data, 19)
    nested_end = nested_start + nested_count
    if nested_end > payload_end:
        raise RuntimeError("HOLOSPH nested class exceeds the payload")
    multiplier_offset = nested_end
    divisibility_offset = multiplier_offset + 4
    integral_count_offset = divisibility_offset + 8
    integral_count = read_u64_at(data, integral_count_offset)
    integral_start = integral_count_offset + 8
    integral = [
        read_coordinate_integral(data, integral_start + index * 24)
        for index in range(integral_count)
    ]
    potential_count_offset = integral_start + integral_count * 24
    potential_count = read_u64_at(data, potential_count_offset)
    potential_start = potential_count_offset + 8
    potential = [
        read_coordinate_potential(data, potential_start + index * 8)
        for index in range(potential_count)
    ]
    if potential_start + potential_count * 8 != payload_end:
        raise RuntimeError("HOLOSPH walker did not reach the payload digest")
    return {
        "payload_end": payload_end,
        "nested_start": nested_start,
        "nested_end": nested_end,
        "multiplier_offset": multiplier_offset,
        "multiplier": struct.unpack_from(">I", data, multiplier_offset)[0],
        "divisibility_offset": divisibility_offset,
        "divisibility": read_u64_at(data, divisibility_offset),
        "integral": integral,
        "potential": potential,
    }


def mutate_interval(data: bytearray, view: dict, field: str) -> dict:
    old = view[field]
    new = old + 0.25
    if field == "birth" and new >= view["scale"]:
        raise RuntimeError("interval mutation would leave the representative scale")
    put_f64(data, view[f"{field}_offset"], new)
    details = {"offset": view[f"{field}_offset"], "old": old, "new": new}
    details.update(recompute_ids(data))
    return details


def mutate_wrong_birth(data: bytearray, view: dict, _other: list) -> dict:
    return mutate_interval(data, view, "birth")


def mutate_wrong_death(data: bytearray, view: dict, _other: list) -> dict:
    if not math.isfinite(view["death"]):
        raise RuntimeError("wrong-death mutation requires a finite demo class")
    return mutate_interval(data, view, "death")


def mutate_active_source(data: bytearray, view: dict, _other: list) -> dict:
    edge = view["pair_birth"]
    source_edge = next(item for item in view["source"] if (item["u"], item["v"]) == edge)
    new_value = (source_edge["value"] + view["scale"]) / 2.0
    put_f64(data, source_edge["value_offset"], new_value)
    return {
        "edge": [source_edge["u"], source_edge["v"]],
        "offset": source_edge["value_offset"],
        "old": source_edge["value"],
        "new": new_value,
    }


def mutate_source_endpoint(data: bytearray, view: dict, _other: list) -> dict:
    source_edge = next(
        item for item in view["source"] if (item["u"], item["v"]) == (1, 2)
    )
    new_v = 3
    put_u64(data, source_edge["v_offset"], new_v)
    return {
        "old_edge": [source_edge["u"], source_edge["v"]],
        "new_edge": [source_edge["u"], new_v],
        "offset": source_edge["v_offset"],
    }


def mutate_vertex_binding(data: bytearray, view: dict, _other: list) -> dict:
    old_vertex_count = view["vertex_count"]
    new_vertex_count = old_vertex_count + 1
    old_pair = view["pair_birth"]
    new_pair = (old_pair[0], old_vertex_count)
    put_u64(data, view["vertex_count_offset"], new_vertex_count)
    put_u64(data, view["pair_birth_v_offset"], new_pair[1])
    return {
        "vertex_count_offset": view["vertex_count_offset"],
        "old_vertex_count": old_vertex_count,
        "new_vertex_count": new_vertex_count,
        "pair_birth_u_offset": view["pair_birth_u_offset"],
        "pair_birth_v_offset": view["pair_birth_v_offset"],
        "old_pair": list(old_pair),
        "new_pair": list(new_pair),
        **recompute_ids(data),
    }


def mutate_orientation(data: bytearray, view: dict, _other: list) -> dict:
    edge = view["pair_birth"]
    term = next(
        item for item in view["cycle"] if (item["u"], item["v"]) == edge
    )
    old = term["coefficient"]
    new = view["modulus"] - old
    put_u32(data, term["coefficient_offset"], new)
    return {
        "edge": list(edge),
        "offset": term["coefficient_offset"],
        "old": old,
        "new": new,
    }


def mutate_wrong_field(data: bytearray, view: dict, _other: list) -> dict:
    new_modulus = 53
    put_u32(data, view["modulus_offset"], new_modulus)
    return {
        "offset": view["modulus_offset"],
        "old": view["modulus"],
        "new": new_modulus,
        **recompute_ids(data),
    }


def mutate_bounding_chain(data: bytearray, view: dict, _other: list) -> dict:
    if not view["chain"]:
        raise RuntimeError("broken-chain mutation requires a finite demo class")
    term = view["chain"][-1]
    old = term["coefficient"]
    new = 2 if old != 2 else 3
    put_u32(data, term["coefficient_offset"], new)
    return {
        "triangle": list(term["vertices"]),
        "offset": term["coefficient_offset"],
        "old": old,
        "new": new,
    }


def replace_cocycle(data: bytearray, terms: list[tuple[int, int, int]]) -> dict:
    view = walk_holospc(data)
    old_terms = terms_as_tuples(view["cocycle"])
    start = view["cocycle_count_offset"]
    end = view["pair_birth_offset"]
    encoded = cocycle_bytes(terms)
    data[start:end] = encoded
    ids = recompute_ids(data)
    return {
        "old_terms": [list(term) for term in old_terms],
        "new_terms": [list(term) for term in terms],
        "count_offset": start,
        "old_bytes": end - start,
        "new_bytes": len(encoded),
        **ids,
    }


def mutate_other_cocycle(data: bytearray, _view: dict, other: list) -> dict:
    return replace_cocycle(data, other)


def add_cocycles(
    left: list[tuple[int, int, int]],
    right: list[tuple[int, int, int]],
    modulus: int,
) -> list[tuple[int, int, int]]:
    coefficients = {}
    for u, v, coefficient in left + right:
        coefficients[(u, v)] = (coefficients.get((u, v), 0) + coefficient) % modulus
    return [
        (u, v, coefficient)
        for (u, v), coefficient in sorted(coefficients.items())
        if coefficient
    ]


def mutate_mixed_cocycle(data: bytearray, view: dict, other: list) -> dict:
    left = terms_as_tuples(view["cocycle"])
    terms = add_cocycles(left, other, view["modulus"])
    if terms == left:
        raise RuntimeError("mixed-cocycle mutation did not change the selected cochain")
    return replace_cocycle(data, terms)


def mutate_inactive_cocycle(data: bytearray, view: dict, _other: list) -> dict:
    inactive_edge = next(
        item for item in view["source"] if (item["u"], item["v"]) == (4, 6)
    )
    terms = terms_as_tuples(view["cocycle"]) + [(4, 6, 1)]
    details = replace_cocycle(data, sorted(terms))
    details.update(
        {
            "inactive_edge": [inactive_edge["u"], inactive_edge["v"]],
            "source_weight": inactive_edge["value"],
            "representative_scale": view["scale"],
        }
    )
    return details


def mutate_potential(data: bytearray, view: dict) -> dict:
    if len(view["potential"]) < 2:
        raise RuntimeError("potential mutation requires a non-root vertex")
    item = view["potential"][1]
    old = item["value"]
    new = old + 0.25
    put_f64(data, item["offset"], new)
    return {"index": 1, "offset": item["offset"], "old": old, "new": new}


def mutate_integral_lift(data: bytearray, view: dict) -> dict:
    if not view["integral"]:
        raise RuntimeError("integral-lift mutation requires a lift term")
    item = view["integral"][0]
    old = item["coefficient"]
    new = old + 1 if old != -1 else old + 2
    put_i64(data, item["coefficient_offset"], new)
    return {
        "edge": [item["u"], item["v"]],
        "offset": item["coefficient_offset"],
        "old": old,
        "new": new,
    }


def mutate_field_multiplier(data: bytearray, view: dict) -> dict:
    old = view["multiplier"]
    new = 2 if old != 2 else 3
    put_u32(data, view["multiplier_offset"], new)
    return {"offset": view["multiplier_offset"], "old": old, "new": new}


def mutate_divisibility(data: bytearray, view: dict) -> dict:
    old = view["divisibility"]
    new = old + 1
    put_u64(data, view["divisibility_offset"], new)
    return {"offset": view["divisibility_offset"], "old": old, "new": new}


def run_checker(checker: Path, path: Path) -> tuple[int, str]:
    completed = subprocess.run(
        [str(checker), str(path)], capture_output=True, text=True, check=False
    )
    response = "\n".join(
        part.strip() for part in (completed.stdout, completed.stderr) if part.strip()
    )
    return completed.returncode, response


def require_mutation_rejection(
    returncode: int, response: str, mutation_type: str, rejection: str
) -> None:
    if returncode == 0:
        raise RuntimeError(f"checker accepted {rejection} mutation {mutation_type}")
    if not response:
        raise RuntimeError(f"checker gave no response for mutation {mutation_type}")
    if rejection == "semantic" and any(
        word in response.lower()
        for word in ("digest", "truncated", "unsupported", "canonical")
    ):
        raise RuntimeError(
            f"checker rejected {mutation_type} before semantics: {response}"
        )


def rejected_mutation_record(
    checker: Path,
    artifact: Path,
    directory: Path,
    mutation_type: str,
    original: bytes,
    mutated: bytes,
    details: dict,
    digest_resealed: bool,
    rejection: str,
    suffix: str,
) -> dict:
    destination = directory / "mutations" / artifact.stem / f"{mutation_type}{suffix}"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(mutated)
    returncode, response = run_checker(checker, destination)
    require_mutation_rejection(returncode, response, mutation_type, rejection)
    return {
        "type": mutation_type,
        "artifact": artifact.name,
        "mutated_path": str(destination.relative_to(directory)),
        "original_sha256": sha256(artifact),
        "mutated_sha256": sha256(destination),
        "original_payload_digest": stored_payload_digest(original),
        "mutated_payload_digest": stored_payload_digest(mutated),
        "digest_resealed": digest_resealed,
        "rejection": rejection,
        "checker_returncode": returncode,
        "checker_response": response,
        "details": details,
    }


def semantic_mutation_record(
    checker: Path,
    artifact: Path,
    directory: Path,
    mutation_type: str,
    mutator,
    other: list,
) -> dict:
    original = artifact.read_bytes()
    data = bytearray(original)
    details = mutator(data, walk_holospc(data), other)
    mutated = reseal_payload(data)
    return rejected_mutation_record(
        checker,
        artifact,
        directory,
        mutation_type,
        original,
        mutated,
        details,
        True,
        "semantic",
        ".hspc",
    )


def coordinate_mutation_record(
    checker: Path,
    artifact: Path,
    directory: Path,
    mutation_type: str,
    mutator,
) -> dict:
    original = artifact.read_bytes()
    data = bytearray(original)
    details = mutator(data, walk_holosph(data))
    mutated = reseal_payload(data)
    return rejected_mutation_record(
        checker,
        artifact,
        directory,
        mutation_type,
        original,
        mutated,
        details,
        True,
        "semantic",
        ".hsph",
    )


def semantic_mutation_records(checker: Path, directory: Path) -> list[dict]:
    target = directory / "class-0.hspc"
    other = terms_as_tuples(
        walk_holospc((directory / "class-1.hspc").read_bytes())["cocycle"]
    )
    mutations = [
        ("wrong_birth", mutate_wrong_birth, []),
        ("wrong_death", mutate_wrong_death, []),
        ("source_endpoint", mutate_source_endpoint, []),
        ("vertex_binding", mutate_vertex_binding, []),
        ("cycle_orientation", mutate_orientation, []),
        ("wrong_field", mutate_wrong_field, []),
        ("changed_active_source_weight", mutate_active_source, []),
        ("broken_bounding_chain", mutate_bounding_chain, []),
        ("inactive_cochain_term", mutate_inactive_cocycle, []),
        ("other_bar_cochain", mutate_other_cocycle, other),
        ("mixed_cochain", mutate_mixed_cocycle, other),
    ]
    return [
        semantic_mutation_record(checker, target, directory, name, mutator, terms)
        for name, mutator, terms in mutations
    ]


def coordinate_mutation_records(checker: Path, directory: Path) -> list[dict]:
    target = directory / "coordinate-0.hsph"
    mutations = [
        ("coordinate_potential", mutate_potential),
        ("coordinate_integral_lift", mutate_integral_lift),
        ("coordinate_field_multiplier", mutate_field_multiplier),
        ("coordinate_divisibility", mutate_divisibility),
    ]
    return [
        coordinate_mutation_record(checker, target, directory, name, mutator)
        for name, mutator in mutations
    ]


def corruption_record(checker: Path, artifact: Path, directory: Path) -> dict:
    original = artifact.read_bytes()
    data = bytearray(original)
    offset = len(data) - 33
    if offset < 0:
        raise RuntimeError("stale-digest mutation requires a payload byte")
    data[offset] ^= 1
    return rejected_mutation_record(
        checker,
        artifact,
        directory,
        "stale_payload_digest",
        original,
        bytes(data),
        {"offset": offset},
        False,
        "integrity",
        artifact.suffix,
    )


def selected_record(
    rank: int,
    classes: list[dict],
    target: dict,
    result: tuple[dict, dict],
    output: Path,
    checker: Path,
) -> dict:
    class_result, coordinate_result = result
    class_path = output / f"class-{rank}.hspc"
    coordinate_path = output / f"coordinate-{rank}.hsph"
    phase_path = output / f"coordinate-{rank}.phase"
    class_path.write_bytes(class_result["artifact"])
    coordinate_path.write_bytes(coordinate_result["artifact"])
    write_phase(phase_path, coordinate_result["coordinate"]["phase"])
    class_check = check_artifact(checker, class_path)
    coordinate_check = check_artifact(checker, coordinate_path)
    return {
        "space_index": class_selector(classes, target)[0],
        "basis_index": target["basis_index"],
        "class_id": target["id"],
        "interval": {
            "birth": float(target["birth"]),
            "death": float(target["death"]),
        },
        "representative_scale": float(target["scale"]),
        "cycle": result[0]["cycle"],
        "bounding_chain": result[0]["bounding_chain"],
        "class_artifact": artifact_record(class_check, class_path),
        "coordinate_artifact": artifact_record(
            coordinate_check, coordinate_path, coordinate_result["coordinate"]
        ),
        "phase_path": phase_path.name,
        "phase": [
            float(value) for value in coordinate_result["coordinate"]["phase"]
        ],
    }


def graph_vertices(cycle: list[dict]) -> list[int]:
    return sorted(
        {vertex for term in cycle for vertex in (term["u"], term["v"])}
    )


def draw_source_edges(ax, vertices: list[int]) -> None:
    for u, v, weight in EDGES:
        if u not in vertices or v not in vertices:
            continue
        x0, y0 = POSITIONS[u]
        x1, y1 = POSITIONS[v]
        ax.plot((x0, x1), (y0, y1), color="0.65", linewidth=1.5, zorder=1)
        ax.text(
            (x0 + x1) / 2,
            (y0 + y1) / 2,
            f"w={weight:g}",
            color="0.2",
            fontsize=8,
            ha="center",
            va="center",
            bbox={"facecolor": "white", "edgecolor": "none", "pad": 1.2},
            zorder=4,
        )


def draw_selected_cycle(ax, cycle: list[dict]) -> None:
    for term in cycle:
        x0, y0 = POSITIONS[term["u"]]
        x1, y1 = POSITIONS[term["v"]]
        ax.plot(
            (x0, x1),
            (y0, y1),
            color="tab:red",
            linewidth=3.5,
            solid_capstyle="round",
            zorder=2,
        )


def draw_phase_vertices(ax, phase: list[float], vertices: list[int]):
    scatter = ax.scatter(
        [POSITIONS[v][0] for v in vertices],
        [POSITIONS[v][1] for v in vertices],
        c=[phase[v] for v in vertices],
        cmap="twilight",
        vmin=0.0,
        vmax=1.0,
        edgecolors="black",
        zorder=3,
    )
    for vertex in vertices:
        x, y = POSITIONS[vertex]
        ax.text(x, y + 0.12, str(vertex), ha="center")
    return scatter


def set_graph_bounds(ax, vertices: list[int]) -> None:
    x_values = [POSITIONS[v][0] for v in vertices]
    y_values = [POSITIONS[v][1] for v in vertices]
    ax.set_xlim(min(x_values) - 0.3, max(x_values) + 0.3)
    ax.set_ylim(min(y_values) - 0.3, max(y_values) + 0.35)


def plot_graph(ax, phase: list[float], cycle: list[dict], title: str):
    vertices = graph_vertices(cycle)
    draw_source_edges(ax, vertices)
    draw_selected_cycle(ax, cycle)
    scatter = draw_phase_vertices(ax, phase, vertices)
    ax.set_title(title, pad=10)
    ax.set_aspect("equal")
    set_graph_bounds(ax, vertices)
    ax.axis("off")
    return scatter


def h1_plot_limits(bars) -> tuple[list, float]:
    h1 = [bar for bar in bars if bar[0] == 1]
    finite_deaths = [death for _, _, death in h1 if math.isfinite(death)]
    right = max(finite_deaths, default=THRESHOLD)
    return h1, right + max(0.25, abs(right) * 0.1)


def selected_bar(
    selected: tuple[dict, dict], birth: float, death: float
) -> Optional[dict]:
    for item in selected:
        if item["birth"] == birth and item["death"] == death:
            return item
    return None


def draw_bar(
    ax,
    row: int,
    birth: float,
    death: float,
    right: float,
    selected: tuple[dict, dict],
) -> None:
    match = selected_bar(selected, birth, death)
    color = "tab:blue" if match is not None else "0.6"
    end = death if math.isfinite(death) else right
    ax.plot(
        (birth, end),
        (row, row),
        color=color,
        linewidth=5,
        solid_capstyle="butt",
        zorder=2,
    )
    if not math.isfinite(death):
        ax.annotate("∞", (end, row), va="center")


def plot_bars(ax, bars, selected: tuple[dict, dict]) -> None:
    h1, right = h1_plot_limits(bars)
    for row, (_, birth, death) in enumerate(h1):
        draw_bar(ax, row, birth, death, right, selected)
    ax.set_yticks(range(len(h1)))
    ax.set_yticklabels([f"H1 bar {row}" for row in range(len(h1))])
    ax.set_xlabel("filtration scale")
    ax.set_title("H1 persistence bars")
    ax.set_xlim(left=0.0, right=right + 0.25)
    if h1:
        ax.set_ylim(-0.5, len(h1) - 0.5)
    ax.grid(axis="x", alpha=0.25)


def write_plot(
    path: Path,
    bars,
    selected: tuple[dict, dict],
    selected_rows: list[dict],
) -> dict:
    try:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError as error:
        return {"status": "unavailable", "reason": str(error)}
    figure, axes = plt.subplots(
        1,
        3,
        figsize=(12, 4.5),
        gridspec_kw={"width_ratios": (1, 1, 1.25)},
        constrained_layout=True,
    )
    scatters = []
    for rank, row in enumerate(selected_rows):
        interval = row["interval"]
        title = (
            f"H1 bar {rank}: {interval['birth']:g} to {interval['death']:g}"
        )
        scatters.append(
            plot_graph(axes[rank], row["phase"], row["cycle"], title)
        )
    figure.colorbar(scatters[0], ax=axes[:2], label="phase", pad=0.03)
    plot_bars(axes[2], bars, selected)
    figure.savefig(path.with_suffix(".svg"))
    figure.savefig(path.with_suffix(".png"), dpi=160)
    plt.close(figure)
    return {
        "status": "written",
        "svg": path.with_suffix(".svg").name,
        "png": path.with_suffix(".png").name,
    }


def bar_record(dimension, birth, death) -> dict:
    birth = float(birth)
    death = None if death is None else float(death)
    if not math.isfinite(birth):
        raise RuntimeError("diagram contains a non-finite birth")
    if death is not None and math.isnan(death):
        raise RuntimeError("diagram contains a NaN death")
    normalized_death = None if death is None or math.isinf(death) else death
    return {
        "dimension": int(dimension),
        "birth": birth,
        "death": normalized_death,
    }


def execution_identity(checker: Path) -> dict:
    module_path = getattr(holos_tda, "__file__", None)
    producer = {
        "package": "holos-tda",
        "version": getattr(holos_tda, "__version__", None),
        "git_hash": getattr(holos_tda, "GIT_HASH", None),
        "module": str(Path(module_path).resolve()) if module_path else None,
    }
    return {
        "producer": producer,
        "checker": {"path": str(checker), "sha256": sha256(checker)},
        "execution": {
            "command": [sys.executable, *sys.argv],
            "python": platform.python_version(),
            "implementation": platform.python_implementation(),
            "platform": platform.platform(),
        },
    }


def main() -> None:
    options = parse_args()
    checker = options.checker
    if not checker.is_file() or not os.access(checker, os.X_OK):
        raise RuntimeError(f"checker is not executable: {checker}")
    options.output.mkdir(parents=True, exist_ok=True)
    source = options.output / "source.sparse"
    write_source(source)
    bars, classes = holos_tda.rips_sparse_classes(
        VERTEX_COUNT,
        EDGES,
        max_dim=1,
        threshold=THRESHOLD,
        modulus=MODULUS,
        threads=1,
    )
    selected = select_classes(bars, classes)
    selected_rows = []
    for rank, target in enumerate(selected):
        result = build_artifacts(classes, target)
        selected_rows.append(
            selected_record(rank, classes, target, result, options.output, checker)
        )
    mutations = [
        corruption_record(
            checker, options.output / f"class-{rank}.hspc", options.output
        )
        for rank in range(len(selected))
    ]
    mutations.extend(
        corruption_record(
            checker, options.output / f"coordinate-{rank}.hsph", options.output
        )
        for rank in range(len(selected))
    )
    semantic_mutations = semantic_mutation_records(checker, options.output)
    coordinate_mutations = coordinate_mutation_records(checker, options.output)
    plot = {"status": "disabled"}
    if not options.no_plot:
        plot = write_plot(options.output / "demo", bars, selected, selected_rows)
    record = {
        "format": "holos-persistent-demo-v1",
        "vertices": VERTEX_COUNT,
        "modulus": MODULUS,
        "threshold": THRESHOLD,
        "identity": execution_identity(checker),
        "source": {
            "path": source.name,
            "bytes": source.stat().st_size,
            "sha256": sha256(source),
            "edges": [
                {"u": u, "v": v, "weight": weight} for u, v, weight in EDGES
            ],
        },
        "bars": [
            bar_record(dimension, birth, death)
            for dimension, birth, death in bars
        ],
        "selected": selected_rows,
        "payload_corruption": mutations,
        "semantic_mutations": semantic_mutations,
        "coordinate_semantic_mutations": coordinate_mutations,
        "plot": plot,
    }
    (options.output / "record.json").write_text(
        json.dumps(record, allow_nan=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {"output": str(options.output), "record": "record.json", "plot": plot}
        )
    )


if __name__ == "__main__":
    main()
