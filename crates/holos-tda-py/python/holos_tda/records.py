def _class_result(raw):
    bars, records = raw
    classes = []
    for (
        group_id,
        class_id,
        basis_index,
        birth,
        death,
        modulus,
        scale,
        terms,
        active_graph_digest,
        class_digest,
    ) in records:
        classes.append(
            {
                "group_id": group_id,
                "id": class_id,
                "basis_index": basis_index,
                "birth": birth,
                "death": death,
                "essential": death is None,
                "modulus": modulus,
                "scale": scale,
                "terms": terms,
                "provenance": {
                    "schema": "holos-persistent-class-source-v1",
                    "active_graph_digest": active_graph_digest,
                    "class_digest": class_digest,
                    "interval": (1, birth, death),
                    "birth": birth,
                    "death": death,
                    "modulus": modulus,
                    "scale": scale,
                },
            }
        )
    return bars, classes


def _class_record(record):
    group_id, class_id, basis_index, birth, death, modulus, scale, terms = record
    return {
        "group_id": group_id,
        "id": class_id,
        "basis_index": basis_index,
        "birth": birth,
        "death": death,
        "essential": death is None,
        "modulus": modulus,
        "scale": scale,
        "terms": terms,
    }


def _critical_records(critical):
    return [
        {
            "birth": {"vertices": pair[0], "value": pair[1]},
            "death": None
            if pair[2] is None
            else {"vertices": pair[2][0], "value": pair[2][1]},
        }
        for pair in critical
    ]


def _atlas_result(raw):
    bars, raw_spaces, raw_sensitivities = raw
    spaces = []
    for lineage, group_id, birth, death, basis, critical in raw_spaces:
        spaces.append(
            {
                "lineage": lineage,
                "group_id": group_id,
                "birth": birth,
                "death": death,
                "essential": death is None,
                "multiplicity": len(basis),
                "basis": [_class_record(record) for record in basis],
                "critical_pairs": _critical_records(critical),
            }
        )
    sensitivities = []
    for lineage, birth, death in raw_sensitivities:
        sensitivities.append(
            {
                "lineage": lineage,
                "birth": {"kind": birth[0], "edges": birth[1]},
                "death": {"kind": death[0], "edges": death[1]},
            }
        )
    return {
        "bars": bars,
        "spaces": spaces,
        "sensitivities": sensitivities,
    }


def _event_record(record):
    kind, first, second, old_first, new_first, old_second, new_second = record
    return {
        "kind": kind,
        "first": first,
        "second": second,
        "old_first": old_first,
        "new_first": new_first,
        "old_second": old_second,
        "new_second": new_second,
    }


def _program_result(raw):
    bars, raw_spaces = raw
    spaces = []
    for group_id, birth, death, basis, critical in raw_spaces:
        spaces.append(
            {
                "group_id": group_id,
                "birth": birth,
                "death": death,
                "essential": death is None,
                "multiplicity": len(basis),
                "basis": [_class_record(record) for record in basis],
                "critical_pairs": _critical_records(critical),
            }
        )
    return {"bars": bars, "spaces": spaces}


def _program_work(raw):
    names = (
        "edges_checked",
        "h0_edges_scanned",
        "guards_checked",
        "atoms_touched",
        "atoms_reused",
        "atoms_repaired",
        "atoms_rebuilt",
        "reduction_columns_reused",
        "reduction_columns_reduced",
        "reduction_column_additions",
    )
    return dict(zip(names, raw))


def _program_event(raw):
    kind, atom, edge, guard = raw
    return {"kind": kind, "atom": atom, "edge": edge, "guard": guard}


def _continuation(raw):
    kind, old_spaces, new_spaces, transport = raw
    return {
        "kind": kind,
        "old_spaces": old_spaces,
        "new_spaces": new_spaces,
        "transport": [
            {"old": old, "new": new, "coefficient": coefficient}
            for old, new, coefficient in transport
        ],
    }


def _correspondence(raw):
    (
        old_space,
        new_space,
        scale,
        old_rank,
        new_rank,
        old_image_rank,
        new_image_rank,
        relation_rank,
        basis,
    ) = raw
    return {
        "old_space": old_space,
        "new_space": new_space,
        "scale": scale,
        "old_rank": old_rank,
        "new_rank": new_rank,
        "old_image_rank": old_image_rank,
        "new_image_rank": new_image_rank,
        "relation_rank": relation_rank,
        "is_isomorphism": (
            relation_rank == old_rank == new_rank
            and old_image_rank == old_rank
            and new_image_rank == new_rank
        ),
        "basis": [
            {
                "old": [
                    {"basis": item[0], "coefficient": item[1]} for item in vector[0]
                ],
                "new": [
                    {"basis": item[0], "coefficient": item[1]} for item in vector[1]
                ],
            }
            for vector in basis
        ],
    }


def _program_update(raw):
    mode, events, continuation, correspondence, work, result = raw
    return {
        "mode": mode,
        "events": [_program_event(record) for record in events],
        "continuation": [_continuation(record) for record in continuation],
        "correspondence": [_correspondence(record) for record in correspondence],
        "work": _program_work(work),
        "result": _program_result(result),
    }


def _index_work(raw):
    names = (
        "edges_checked",
        "nodes_touched",
        "nodes_shared",
        "nodes_repaired",
        "nodes_rebuilt",
        "nodes_composed",
        "relative_nodes_rebuilt",
        "relative_nodes_composed",
        "relative_input_cells",
        "relative_core_cells",
        "relative_cancellations",
        "reduction_columns_reused",
        "reduction_columns_reduced",
        "reduction_column_additions",
    )
    raw = list(raw)
    relative = list(raw.pop(6))
    return dict(zip(names, raw[:6] + relative + raw[6:]))


def _index_event(raw):
    kind, node, edge = raw
    return {"kind": kind, "node": node, "edge": edge}


def _index_update(raw):
    (
        mode,
        bars,
        removed,
        added,
        events,
        correspondence,
        work,
        version,
        delta_proof,
        snapshot_proof,
    ) = raw
    return {
        "mode": mode,
        "bars": bars,
        "diagram_delta": {"removed": removed, "added": added},
        "events": [_index_event(record) for record in events],
        "correspondence": [_correspondence(record) for record in correspondence],
        "work": _index_work(work),
        "version": version,
        "delta_proof": (None if delta_proof is None else bytes(delta_proof)),
        "snapshot_proof": (None if snapshot_proof is None else bytes(snapshot_proof)),
    }


def _intervention(raw):
    status, target, lower, upper, edits, result, artifact = raw
    return {
        "status": status,
        "target": target,
        "lower_bound": lower,
        "upper_bound": upper,
        "edits": [
            {"edge": edge, "before": before, "after": after}
            for edge, before, after in edits
        ],
        "result": None if result is None else _program_result(result),
        "artifact": None if artifact is None else bytes(artifact),
    }
