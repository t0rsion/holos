use holos_tda::{
    Bigrade, BipersistenceArtifact, BipersistenceMap, BipersistenceTerm, CircularCoordinate,
    CircularCoordinateFamily, CircularCoordinateFamilyStatus, ClassExtension, ClassExtensionKind,
    CohomologyClassAtlas,
};

pub(super) type Grade = (usize, usize);
pub(super) type Term = (usize, u32);
type MapColumn = (usize, Vec<Term>);
pub(super) type MapRecord = (Grade, Grade, usize, Vec<MapColumn>);
type AtlasExtensionRecord = (Grade, String, Vec<Term>, Vec<Vec<Term>>);
type AtlasRegionRecord = (usize, String, usize, Vec<Grade>);
pub(super) type AtlasRecord = (
    Grade,
    Vec<Term>,
    Vec<AtlasExtensionRecord>,
    Vec<AtlasRegionRecord>,
);
type CircularCoordinateRecord = (
    String,
    u32,
    f64,
    u32,
    Vec<Term>,
    Vec<(Vec<usize>, u32)>,
    Vec<(usize, usize, i64)>,
    u64,
    Vec<f64>,
    Vec<f64>,
    (f64, f64, f64, usize, f64),
);
type CircularEntryRecord = (Grade, String, String, Option<CircularCoordinateRecord>);
pub(super) type CircularFamilyRecord = (Grade, Vec<Term>, Vec<CircularEntryRecord>);
pub(super) type ArtifactSummary = (usize, usize, usize, usize, usize, usize, usize, usize);

pub(super) fn to_grade(value: Grade) -> Bigrade {
    Bigrade::new(value.0, value.1)
}

pub(super) fn from_grade(value: Bigrade) -> Grade {
    (value.scale(), value.density())
}

pub(super) fn extension_kind(value: ClassExtensionKind) -> &'static str {
    match value {
        ClassExtensionKind::Unique => "unique",
        ClassExtensionKind::Ambiguous => "ambiguous",
        ClassExtensionKind::NoExtension => "no_extension",
    }
}

pub(super) fn terms(value: &[BipersistenceTerm]) -> Vec<Term> {
    value
        .iter()
        .map(|term| (term.basis_index, term.coefficient))
        .collect()
}

pub(super) fn to_terms(value: Vec<Term>) -> Vec<BipersistenceTerm> {
    value
        .into_iter()
        .map(|(basis_index, coefficient)| BipersistenceTerm {
            basis_index,
            coefficient,
        })
        .collect()
}

pub(super) fn map_record(value: &BipersistenceMap) -> MapRecord {
    (
        from_grade(value.lower_grade),
        from_grade(value.upper_grade),
        value.rank,
        value
            .columns
            .iter()
            .map(|column| (column.source_basis_index, terms(&column.image)))
            .collect(),
    )
}

fn extension_record(value: &ClassExtension) -> AtlasExtensionRecord {
    (
        from_grade(value.grade),
        extension_kind(value.kind).into(),
        terms(&value.class),
        value.ambiguity.iter().map(|item| terms(item)).collect(),
    )
}

pub(super) fn atlas_record(value: &CohomologyClassAtlas) -> AtlasRecord {
    (
        from_grade(value.base_grade),
        terms(&value.base_class),
        value.extensions.iter().map(extension_record).collect(),
        value
            .regions
            .iter()
            .map(|region| {
                (
                    region.region_index,
                    extension_kind(region.kind).into(),
                    region.ambiguity_rank,
                    region.grades.iter().copied().map(from_grade).collect(),
                )
            })
            .collect(),
    )
}

pub(super) fn coordinate_record(value: &CircularCoordinate) -> CircularCoordinateRecord {
    (
        value.space.to_string(),
        value.modulus,
        value.scale,
        value.field_multiplier,
        value
            .class
            .iter()
            .map(|term| (term.basis_index, term.coefficient))
            .collect(),
        value
            .source
            .iter()
            .map(|term| (vec![term.u, term.v], term.coefficient))
            .collect(),
        value
            .integral
            .iter()
            .map(|term| (term.u, term.v, term.coefficient))
            .collect(),
        value.divisibility,
        value.potential.clone(),
        value.phase.clone(),
        (
            value.energy,
            value.max_residual,
            value.relative_residual,
            value.iterations,
            value.tolerance,
        ),
    )
}

pub(super) fn family_record(value: &CircularCoordinateFamily) -> CircularFamilyRecord {
    (
        from_grade(value.base_grade),
        terms(&value.base_class),
        value
            .entries
            .iter()
            .map(|entry| {
                let (status, coordinate) = match &entry.status {
                    CircularCoordinateFamilyStatus::NotAttempted => ("not_attempted", None),
                    CircularCoordinateFamilyStatus::LiftFailed => ("lift_failed", None),
                    CircularCoordinateFamilyStatus::SolveFailed => ("solve_failed", None),
                    CircularCoordinateFamilyStatus::Success(coordinate) => {
                        ("success", Some(coordinate_record(coordinate)))
                    }
                };
                (
                    from_grade(entry.grade),
                    extension_kind(entry.extension).into(),
                    status.into(),
                    coordinate,
                )
            })
            .collect(),
    )
}

pub(super) fn artifact_summary(value: &BipersistenceArtifact) -> ArtifactSummary {
    let summary = value.summary();
    (
        summary.vertices,
        summary.edges,
        summary.nodes,
        summary.cover_maps,
        summary.rectangles,
        summary.regions,
        summary.class_atlases,
        summary.circular_families,
    )
}
