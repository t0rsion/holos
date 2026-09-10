"""Finite degree-Rips bipersistence with checked module evidence.

The module stores the complete finite H1 diagram on the degree-Rips grid.
Queries return dictionaries with explicit grade, map, class, and coordinate
fields. The underlying ``HOLOSBP`` bytes can be saved as a reproducible claim.
"""

from . import _core
from ._coerce import _triplets

__all__ = [
    "Bipersistence",
    "BipersistenceArtifact",
    "degree_rips_bipersistence",
    "load_bipersistence_artifact",
]


def _term_records(terms):
    return [
        {"basis_index": int(index), "coefficient": int(coefficient)}
        for index, coefficient in terms
    ]


def _map_record(raw):
    lower, upper, rank, columns = raw
    return {
        "lower": tuple(lower),
        "upper": tuple(upper),
        "rank": int(rank),
        "columns": [
            {
                "source_basis_index": int(source),
                "image": _term_records(image),
            }
            for source, image in columns
        ],
    }


def _atlas_record(raw):
    base, base_class, extensions, regions = raw
    return {
        "base": tuple(base),
        "base_class": _term_records(base_class),
        "extensions": [
            {
                "grade": tuple(grade),
                "kind": kind,
                "class": _term_records(class_terms),
                "ambiguity": [_term_records(direction) for direction in ambiguity],
            }
            for grade, kind, class_terms, ambiguity in extensions
        ],
        "regions": [
            {
                "region_index": int(index),
                "kind": kind,
                "ambiguity_rank": int(ambiguity_rank),
                "grades": [tuple(grade) for grade in grades],
            }
            for index, kind, ambiguity_rank, grades in regions
        ],
    }


def _coordinate_record(raw):
    (
        space,
        modulus,
        scale,
        field_multiplier,
        class_terms,
        source,
        integral,
        divisibility,
        potential,
        phase,
        diagnostics,
    ) = raw
    energy, max_residual, relative_residual, iterations, tolerance = diagnostics
    return {
        "space": space,
        "modulus": int(modulus),
        "scale": float(scale),
        "field_multiplier": int(field_multiplier),
        "class": _term_records(class_terms),
        "source": [
            {"simplex": list(simplex), "coefficient": int(coefficient)}
            for simplex, coefficient in source
        ],
        "integral": [
            {"u": int(u), "v": int(v), "coefficient": int(coefficient)}
            for u, v, coefficient in integral
        ],
        "divisibility": int(divisibility),
        "potential": [float(value) for value in potential],
        "phase": [float(value) for value in phase],
        "energy": float(energy),
        "max_residual": float(max_residual),
        "relative_residual": float(relative_residual),
        "iterations": int(iterations),
        "tolerance": float(tolerance),
    }


def _family_record(raw):
    base, base_class, entries = raw
    return {
        "base": tuple(base),
        "base_class": _term_records(base_class),
        "entries": [
            {
                "grade": tuple(grade),
                "kind": kind,
                "coordinate": (
                    None if coordinate is None else _coordinate_record(coordinate)
                ),
            }
            for grade, kind, coordinate in entries
        ],
    }


def _summary(raw):
    (
        vertices,
        edges,
        nodes,
        cover_maps,
        rectangles,
        regions,
        class_atlases,
        circular_families,
    ) = raw
    return {
        "vertices": int(vertices),
        "edges": int(edges),
        "nodes": int(nodes),
        "cover_maps": int(cover_maps),
        "rectangles": int(rectangles),
        "regions": int(regions),
        "class_atlases": int(class_atlases),
        "circular_families": int(circular_families),
    }


class Bipersistence:
    """A checked finite degree-Rips H1 module.

    Construct an instance with :func:`degree_rips_bipersistence`. Grades are
    ``(scale_index, density_index)`` pairs. The density axis addresses the
    descending ``minimum_degrees`` property.
    """

    def __init__(self, inner):
        self._inner = inner

    @property
    def modulus(self):
        """Return the prime coefficient modulus."""
        return int(self._inner.modulus)

    @property
    def scales(self):
        """Return scale values in ascending order."""
        return [float(value) for value in self._inner.scales]

    @property
    def minimum_degrees(self):
        """Return minimum-degree levels in descending order."""
        return [int(value) for value in self._inner.minimum_degrees]

    @property
    def node_ranks(self):
        """Return one record for each grid node."""
        return [
            {
                "grade": (int(scale), int(density)),
                "rank": int(rank),
            }
            for scale, density, rank in self._inner.node_ranks
        ]

    @property
    def cover_maps(self):
        """Return all checked horizontal and vertical cover maps."""
        return [_map_record(value) for value in self._inner.cover_maps]

    def map_rank(self, lower, upper):
        """Return the exact map rank between comparable grades."""
        return int(self._inner.map_rank(tuple(lower), tuple(upper)))

    def map(self, lower, upper):
        """Return a canonical matrix for the map between two grades."""
        return _map_record(self._inner.map(tuple(lower), tuple(upper)))

    def rectangle_rank(self, lower, upper):
        """Return the generalized rank of a closed parameter rectangle."""
        return int(self._inner.rectangle_rank(tuple(lower), tuple(upper)))

    def region_rank(self, grades):
        """Return the generalized rank of a connected finite region."""
        return int(self._inner.region_rank([tuple(grade) for grade in grades]))

    def class_atlas(self, base, class_):
        """Return the exact extension atlas of one nonzero class."""
        return _atlas_record(self._inner.class_atlas(tuple(base), _normalize_terms(class_)))

    def circular_family(
        self, base, class_, tolerance=1e-10, max_iterations=10_000
    ):
        """Return checked circular coordinates for unique class extensions."""
        return _family_record(
            self._inner.circular_family(
                tuple(base),
                _normalize_terms(class_),
                float(tolerance),
                int(max_iterations),
            )
        )

    def record_rectangle(self, lower, upper):
        """Add one checked rectangle-rank claim to the artifact."""
        self._inner.record_rectangle(tuple(lower), tuple(upper))

    def record_region(self, grades):
        """Add one checked connected-region rank claim to the artifact."""
        self._inner.record_region([tuple(grade) for grade in grades])

    def record_class_atlas(self, base, class_):
        """Add one checked class atlas to the artifact."""
        return _atlas_record(
            self._inner.record_class_atlas(tuple(base), _normalize_terms(class_))
        )

    def record_circular_family(
        self, base, class_, tolerance=1e-10, max_iterations=10_000
    ):
        """Add checked circular data for a stored class atlas."""
        return _family_record(
            self._inner.record_circular_family(
                tuple(base),
                _normalize_terms(class_),
                float(tolerance),
                int(max_iterations),
            )
        )

    @property
    def artifact(self):
        """Return canonical ``HOLOSBP`` bytes for the current claims."""
        return bytes(self._inner.artifact)

    @property
    def artifact_summary(self):
        """Return structural counts for the current artifact."""
        return _summary(self._inner.artifact_summary)


class BipersistenceArtifact:
    """A decoded, self-contained, and verified ``HOLOSBP`` artifact."""

    def __init__(self, inner):
        self._inner = inner

    @property
    def bytes(self):
        """Return canonical artifact bytes."""
        return bytes(self._inner.bytes)

    @property
    def summary(self):
        """Return structural counts from the artifact."""
        return _summary(self._inner.summary)

    @property
    def rectangles(self):
        """Return stored generalized rectangle-rank claims."""
        return [
            {
                "lower": tuple(lower),
                "upper": tuple(upper),
                "rank": int(rank),
            }
            for lower, upper, rank in self._inner.rectangles
        ]

    @property
    def regions(self):
        """Return stored generalized connected-region rank claims."""
        return [
            {
                "grades": [tuple(grade) for grade in grades],
                "rank": int(rank),
            }
            for grades, rank in self._inner.regions
        ]

    @property
    def class_atlases(self):
        """Return stored class-extension atlases."""
        return [_atlas_record(value) for value in self._inner.class_atlases]

    def verify(self):
        """Replay and verify the exact artifact claims."""
        self._inner.verify()


def _normalize_terms(class_):
    return [(int(index), int(coefficient)) for index, coefficient in class_]


def degree_rips_bipersistence(
    n,
    triplets,
    threshold=None,
    modulus=47,
    *,
    scales=None,
    minimum_degrees=None,
):
    """Build a checked degree-Rips H1 module from sparse weighted edges.

    ``triplets`` contains ``(u, v, weight)`` rows. By default, the module uses
    every induced critical scale and minimum degree. Pass both ``scales`` and
    ``minimum_degrees`` to declare a smaller exact grid. The scale values must
    increase, and the minimum degrees must decrease and end at zero.
    """
    inner = _core.degree_rips_bipersistence(
        int(n),
        _triplets(triplets),
        None if threshold is None else float(threshold),
        int(modulus),
        scales=(None if scales is None else [float(value) for value in scales]),
        minimum_degrees=(
            None
            if minimum_degrees is None
            else [int(value) for value in minimum_degrees]
        ),
    )
    return Bipersistence(inner)


def load_bipersistence_artifact(data):
    """Decode and verify canonical ``HOLOSBP`` bytes."""
    return BipersistenceArtifact(_core.load_bipersistence_artifact(bytes(data)))
