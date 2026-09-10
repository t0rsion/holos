from __future__ import annotations


def _square_rows(matrix, message, size=None):
    rows = [list(row) for row in matrix]
    expected = len(rows) if size is None else size
    if len(rows) != expected or any(len(row) != expected for row in rows):
        raise ValueError(message)
    return rows


def _condensed_rows(rows):
    return [
        rows[left][right]
        for left in range(len(rows))
        for right in range(left + 1, len(rows))
    ]


def _float_values(values):
    return [float(value) for value in values]


def _points(points):
    return [list(map(float, point)) for point in points]


def _cocycle(cocycle):
    return [(int(u), int(v), int(coefficient)) for u, v, coefficient in cocycle]


def _persistent_class_input(persistent_class):
    """Convert a scalar class record into the checked core representation."""
    fields = _read_persistent_class_fields(persistent_class)
    _validate_persistent_class_fields(fields)
    (
        group_id,
        class_id,
        basis_index,
        birth,
        death,
        modulus,
        scale,
        terms,
        _schema,
        digest,
        class_digest,
        _interval,
        provenance_birth,
        provenance_death,
        provenance_modulus,
        provenance_scale,
    ) = fields
    return (
        birth,
        death,
        modulus,
        scale,
        terms,
        (group_id, class_id, basis_index, digest, class_digest),
        (provenance_birth, provenance_death, provenance_modulus, provenance_scale),
    )


def _read_persistent_class_fields(persistent_class):
    try:
        group_id = persistent_class["group_id"]
        class_id = persistent_class["id"]
        basis_index = int(persistent_class["basis_index"])
        birth = float(persistent_class["birth"])
        death = persistent_class["death"]
        death = None if death is None else float(death)
        modulus = int(persistent_class["modulus"])
        scale = float(persistent_class["scale"])
        terms = [
            (int(u), int(v), int(coefficient))
            for u, v, coefficient in persistent_class["terms"]
        ]
        provenance = persistent_class["provenance"]
        schema = provenance["schema"]
        digest = provenance["active_graph_digest"]
        class_digest = provenance["class_digest"]
        interval = tuple(provenance["interval"])
        provenance_birth = float(provenance["birth"])
        provenance_death = provenance["death"]
        provenance_death = None if provenance_death is None else float(provenance_death)
        provenance_modulus = int(provenance["modulus"])
        provenance_scale = float(provenance["scale"])
    except (KeyError, TypeError, ValueError, OverflowError) as error:
        raise ValueError(
            "persistent_class must be a scalar class record with provenance"
        ) from error

    return (
        group_id,
        class_id,
        basis_index,
        birth,
        death,
        modulus,
        scale,
        terms,
        schema,
        digest,
        class_digest,
        interval,
        provenance_birth,
        provenance_death,
        provenance_modulus,
        provenance_scale,
    )


def _validate_persistent_class_fields(fields):
    (
        group_id,
        class_id,
        _basis_index,
        birth,
        death,
        modulus,
        scale,
        _terms,
        schema,
        digest,
        class_digest,
        interval,
        provenance_birth,
        provenance_death,
        provenance_modulus,
        provenance_scale,
    ) = fields
    if schema != "holos-persistent-class-source-v1":
        raise ValueError("persistent class provenance schema is not supported")
    _validate_persistent_class_identifiers(group_id, class_id, digest, class_digest)
    _validate_persistent_class_interval(
        interval,
        birth,
        death,
        provenance_birth,
        provenance_death,
        modulus,
        provenance_modulus,
        scale,
        provenance_scale,
    )


def _validate_persistent_class_identifiers(group_id, class_id, digest, class_digest):
    if not isinstance(group_id, str) or not isinstance(class_id, str):
        raise ValueError(  # noqa: TRY004
            "persistent class identifiers must be hexadecimal strings"
        )
    if not isinstance(digest, str):
        raise ValueError(  # noqa: TRY004
            "persistent class identifiers must be hexadecimal strings"
        )
    if not isinstance(class_digest, str):
        raise ValueError(  # noqa: TRY004
            "persistent class identity digest must be hexadecimal"
        )


def _validate_persistent_class_interval(
    interval,
    birth,
    death,
    provenance_birth,
    provenance_death,
    modulus,
    provenance_modulus,
    scale,
    provenance_scale,
):
    if interval != (1, birth, death):
        raise ValueError("persistent class interval differs from its provenance")
    if provenance_birth != birth or provenance_death != death:
        raise ValueError("persistent class interval differs from its provenance")
    if provenance_modulus != modulus:
        raise ValueError("persistent class field differs from its provenance")
    if provenance_scale != scale:
        raise ValueError("persistent class scale differs from its provenance")


def _triplets(triplets):
    return [(int(i), int(j), float(distance)) for i, j, distance in triplets]
