use super::*;
use crate::{
    Bigrade, BipersistenceRegion, BipersistenceTerm, CircularCoordinateParams,
    DegreeRipsBifiltration, DegreeRipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{BipersistenceProofLimits, verify_bipersistence};
use sha2::{Digest, Sha256};

fn graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
        ],
    )
    .unwrap()
}

fn cycle_graph(vertices: usize) -> SparseDistanceMatrix {
    let mut edges = (0..vertices - 1)
        .map(|u| (u, u + 1, 1.0))
        .collect::<Vec<_>>();
    edges.push((0, vertices - 1, 1.0));
    SparseDistanceMatrix::from_triplets(vertices, &edges).unwrap()
}

fn failure_artifact() -> BipersistenceArtifact {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&cycle_graph(8), DegreeRipsParams::default()).unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    let atlas = module
        .class_atlas(
            Bigrade::new(1, 5),
            &[BipersistenceTerm {
                basis_index: 0,
                coefficient: 1,
            }],
        )
        .unwrap();
    artifact
        .record_class_atlas(&module, &atlas, limits)
        .unwrap();
    artifact
        .record_circular_family(
            &module,
            &atlas,
            CircularCoordinateParams::default().with_max_iterations(1),
            limits,
        )
        .unwrap();
    artifact
}

fn append_digest(payload: &mut Vec<u8>) {
    let mut hash = Sha256::new();
    hash.update(b"holos-bipersistence-v2");
    hash.update(payload.as_slice());
    payload.extend_from_slice(&hash.finalize());
}

fn refinalize(bytes: &mut Vec<u8>) {
    let split = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-bipersistence-v2");
    hash.update(&bytes[..split]);
    let digest = hash.finalize();
    bytes.truncate(split);
    bytes.extend_from_slice(&digest);
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PayloadCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn skip(&mut self, count: usize) {
        self.position += count;
    }

    fn u8(&mut self) -> u8 {
        let value = self.bytes[self.position];
        self.position += 1;
        value
    }

    fn u64(&mut self) -> u64 {
        let end = self.position + 8;
        let value = u64::from_be_bytes(self.bytes[self.position..end].try_into().unwrap());
        self.position = end;
        value
    }

    fn usize(&mut self) -> usize {
        self.u64().try_into().unwrap()
    }

    fn skip_source(&mut self) {
        self.skip(8);
        let count = self.usize();
        self.skip(count * 24);
    }

    fn skip_counted(&mut self, width: usize) {
        let count = self.usize();
        self.skip(count * width);
    }

    fn grade(&mut self) {
        self.skip(16);
    }

    fn terms(&mut self) {
        let count = self.usize();
        for _ in 0..count {
            self.skip(8);
            self.skip(4);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CircularEntryOffsets {
    extension: usize,
    extension_code: u8,
    status: usize,
}

fn circular_entry_offsets(bytes: &[u8]) -> Vec<CircularEntryOffsets> {
    let payload_len = bytes.len() - 32;
    let mut reader = PayloadCursor::new(&bytes[..payload_len]);
    skip_header(&mut reader);
    skip_claims(&mut reader);
    let offsets = read_circular_families(&mut reader);
    assert_eq!(reader.position, payload_len);
    offsets
}

fn skip_header(reader: &mut PayloadCursor<'_>) {
    reader.skip(11);
    reader.skip_source();
    reader.skip(12);
    reader.skip_counted(8);
    reader.skip_counted(8);
}

fn skip_claims(reader: &mut PayloadCursor<'_>) {
    skip_nodes(reader);
    skip_maps(reader);
    skip_rectangles(reader);
    skip_regions(reader);
    skip_atlases(reader);
}

fn skip_nodes(reader: &mut PayloadCursor<'_>) {
    let count = reader.usize();
    for _ in 0..count {
        reader.grade();
        reader.skip(40);
    }
}

fn skip_maps(reader: &mut PayloadCursor<'_>) {
    let count = reader.usize();
    for _ in 0..count {
        skip_map(reader);
    }
}

fn skip_map(reader: &mut PayloadCursor<'_>) {
    reader.grade();
    reader.grade();
    reader.skip(64);
    reader.skip(8);
    let count = reader.usize();
    for _ in 0..count {
        reader.skip(8);
        reader.terms();
    }
}

fn skip_rectangles(reader: &mut PayloadCursor<'_>) {
    let count = reader.usize();
    reader.skip(count * 40);
}

fn skip_regions(reader: &mut PayloadCursor<'_>) {
    let count = reader.usize();
    for _ in 0..count {
        let grade_count = reader.usize();
        reader.skip(grade_count * 16 + 8);
    }
}

fn skip_atlases(reader: &mut PayloadCursor<'_>) {
    let count = reader.usize();
    for _ in 0..count {
        skip_atlas(reader);
    }
}

fn skip_atlas(reader: &mut PayloadCursor<'_>) {
    reader.grade();
    reader.terms();
    let extension_count = reader.usize();
    for _ in 0..extension_count {
        skip_extension(reader);
    }
    let region_count = reader.usize();
    for _ in 0..region_count {
        reader.skip(17);
        let grade_count = reader.usize();
        reader.skip(grade_count * 16);
    }
}

fn skip_extension(reader: &mut PayloadCursor<'_>) {
    reader.grade();
    reader.skip(1);
    reader.terms();
    let ambiguity_count = reader.usize();
    for _ in 0..ambiguity_count {
        reader.terms();
    }
}

fn read_circular_families(reader: &mut PayloadCursor<'_>) -> Vec<CircularEntryOffsets> {
    let count = reader.usize();
    let mut offsets = Vec::new();
    for _ in 0..count {
        read_circular_family(reader, &mut offsets);
    }
    offsets
}

fn read_circular_family(reader: &mut PayloadCursor<'_>, offsets: &mut Vec<CircularEntryOffsets>) {
    reader.grade();
    reader.terms();
    reader.skip(16);
    let count = reader.usize();
    for _ in 0..count {
        read_circular_entry(reader, offsets);
    }
}

fn read_circular_entry(reader: &mut PayloadCursor<'_>, offsets: &mut Vec<CircularEntryOffsets>) {
    reader.grade();
    let extension = reader.position;
    let extension_code = reader.u8();
    let status = reader.position;
    let status_code = reader.u8();
    offsets.push(CircularEntryOffsets {
        extension,
        extension_code,
        status,
    });
    if status_code == 3 {
        let count = reader.usize();
        reader.skip(count);
    }
}

#[test]
fn artifact_round_trips_and_independent_checker_accepts() {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph(), DegreeRipsParams::default()).unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    artifact
        .record_rectangle(
            &module,
            crate::BipersistenceRectangle::new(Bigrade::new(1, 1), Bigrade::new(2, 3)).unwrap(),
            limits,
        )
        .unwrap();
    artifact
        .record_region(
            &module,
            BipersistenceRegion::new(vec![
                Bigrade::new(0, 2),
                Bigrade::new(1, 2),
                Bigrade::new(1, 1),
                Bigrade::new(2, 1),
            ])
            .unwrap(),
            limits,
        )
        .unwrap();
    let atlas = module
        .class_atlas(
            Bigrade::new(1, 1),
            &[BipersistenceTerm {
                basis_index: 0,
                coefficient: 2,
            }],
        )
        .unwrap();
    artifact
        .record_class_atlas(&module, &atlas, limits)
        .unwrap();
    artifact
        .record_circular_family(&module, &atlas, CircularCoordinateParams::default(), limits)
        .unwrap();
    let bytes = artifact.encode(limits).unwrap();
    let decoded = BipersistenceArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    assert_eq!(decoded.summary().rectangles, 1);
    assert_eq!(decoded.summary().regions, 1);
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.nodes, module.nodes().len());
    assert_eq!(checked.rectangles, 1);
    assert_eq!(checked.regions, 1);
    assert_eq!(checked.class_atlases, 1);
    assert_eq!(checked.circular_families, 1);
}

#[test]
fn declared_grid_round_trips_and_independent_checker_accepts() {
    let degree_rips = DegreeRipsBifiltration::from_graph_on_grid(
        &graph(),
        vec![1.0, 2.0],
        vec![2, 0],
        DegreeRipsParams {
            threshold: Some(2.0),
            ..DegreeRipsParams::default()
        },
    )
    .unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    artifact
        .record_rectangle(
            &module,
            crate::BipersistenceRectangle::new(Bigrade::new(0, 0), Bigrade::new(1, 1)).unwrap(),
            limits,
        )
        .unwrap();
    let bytes = artifact.encode(limits).unwrap();
    let decoded = BipersistenceArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.scales, 2);
    assert_eq!(checked.density_levels, 2);
    assert_eq!(checked.nodes, 4);
}

#[test]
fn reused_grid_artifact_preserves_complete_maps_for_independent_checker() {
    let cycle = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let degree_rips = DegreeRipsBifiltration::from_graph_on_grid(
        &cycle,
        vec![1.0, 2.0, 3.0],
        vec![3, 0],
        DegreeRipsParams {
            threshold: Some(3.0),
            ..DegreeRipsParams::default()
        },
    )
    .unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    assert_eq!(artifact.summary().nodes, 6);
    assert_eq!(artifact.summary().cover_maps, 7);
    let bytes = artifact.encode(limits).unwrap();
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.scales, 3);
    assert_eq!(checked.density_levels, 2);
    assert_eq!(checked.nodes, module.nodes().len());
    assert_eq!(checked.cover_maps, module.cover_maps().len());
}

#[test]
fn artifact_rejects_changed_claim() {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph(), DegreeRipsParams::default()).unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (artifact, _) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    let mut bytes = artifact.encode(limits).unwrap();
    let position = bytes.len() - 33;
    bytes[position] ^= 1;
    assert!(BipersistenceArtifact::decode(&bytes, limits).is_err());
    assert!(verify_bipersistence(&bytes, BipersistenceProofLimits::default()).is_err());
}

fn artifact_and_module() -> (BipersistenceArtifact, crate::BipersistenceModule) {
    let filtration =
        DegreeRipsBifiltration::from_graph(&graph(), DegreeRipsParams::default()).unwrap();
    BipersistenceArtifact::build(&filtration, 47, BipersistenceArtifactLimits::default()).unwrap()
}

fn assert_collection_limits(
    mut artifact: BipersistenceArtifact,
    limits_for: impl Fn(usize) -> BipersistenceArtifactLimits,
    record: impl Fn(&mut BipersistenceArtifact, bool, BipersistenceArtifactLimits) -> crate::Result<()>,
) {
    for capacity in [0, 1] {
        let limits = limits_for(capacity);
        if capacity == 1 {
            record(&mut artifact, false, limits).unwrap();
        }
        let bytes = artifact.encode(limits).unwrap();
        let digest = artifact.digest;
        let error = record(&mut artifact, capacity == 1, limits).unwrap_err();
        assert!(
            error.to_string().contains("count exceeds the limit"),
            "{error}"
        );
        assert_eq!(artifact.digest, digest);
        assert_eq!(artifact.encode(limits).unwrap(), bytes);
        artifact.verify(limits).unwrap();
    }
    record(&mut artifact, false, limits_for(1)).unwrap();
    artifact.verify(limits_for(1)).unwrap();
}

#[test]
fn rectangle_collection_limit_is_transactional() {
    let (artifact, module) = artifact_and_module();
    let rectangles = [
        crate::BipersistenceRectangle::new(Bigrade::new(1, 1), Bigrade::new(2, 3)).unwrap(),
        crate::BipersistenceRectangle::new(Bigrade::new(0, 0), Bigrade::new(1, 1)).unwrap(),
    ];
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_rectangles: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_rectangle(&module, rectangles[usize::from(alternate)], limits)
        },
    );
}

#[test]
fn region_collection_limit_is_transactional() {
    let (artifact, module) = artifact_and_module();
    let regions = [
        BipersistenceRegion::new(vec![Bigrade::new(1, 1), Bigrade::new(1, 2)]).unwrap(),
        BipersistenceRegion::new(vec![Bigrade::new(0, 0), Bigrade::new(0, 1)]).unwrap(),
    ];
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_regions: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_region(&module, regions[usize::from(alternate)].clone(), limits)
        },
    );
}

fn class_atlases(module: &crate::BipersistenceModule) -> [crate::CohomologyClassAtlas; 2] {
    [1, 2].map(|coefficient| {
        module
            .class_atlas(
                Bigrade::new(1, 1),
                &[BipersistenceTerm {
                    basis_index: 0,
                    coefficient,
                }],
            )
            .unwrap()
    })
}

#[test]
fn class_atlas_collection_limit_is_transactional() {
    let (artifact, module) = artifact_and_module();
    let atlases = class_atlases(&module);
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_class_atlases: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_class_atlas(&module, &atlases[usize::from(alternate)], limits)
        },
    );
}

#[test]
fn circular_family_collection_limit_is_transactional() {
    let (mut artifact, module) = artifact_and_module();
    let atlases = class_atlases(&module);
    for atlas in &atlases {
        artifact
            .record_class_atlas(&module, atlas, Default::default())
            .unwrap();
    }
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_circular_families: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_circular_family(
                &module,
                &atlases[usize::from(alternate)],
                Default::default(),
                limits,
            )
        },
    );
}

#[test]
fn circular_family_status_shape_rejects_missing_or_nonunique_coordinates() {
    use super::model::{ArtifactCircularEntry, ArtifactCircularStatus};

    let unique = ArtifactCircularEntry {
        grade: Bigrade::new(0, 0),
        extension: crate::ClassExtensionKind::Unique,
        status: ArtifactCircularStatus::NotAttempted,
    };
    assert!(super::verify::validate_circular_entry_shape(&unique).is_err());

    let empty_success = ArtifactCircularEntry {
        status: ArtifactCircularStatus::Success(Vec::new()),
        ..unique.clone()
    };
    assert!(super::verify::validate_circular_entry_shape(&empty_success).is_err());

    let ambiguous_success = ArtifactCircularEntry {
        extension: crate::ClassExtensionKind::Ambiguous,
        status: ArtifactCircularStatus::Success(vec![1]),
        ..unique
    };
    assert!(super::verify::validate_circular_entry_shape(&ambiguous_success).is_err());
}

#[test]
fn circular_family_iteration_limits_bound_record_decode_and_replay() {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&cycle_graph(8), DegreeRipsParams::default()).unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    let atlas = module
        .class_atlas(
            Bigrade::new(1, 5),
            &[BipersistenceTerm {
                basis_index: 0,
                coefficient: 1,
            }],
        )
        .unwrap();
    artifact
        .record_class_atlas(&module, &atlas, limits)
        .unwrap();
    let digest = artifact.digest;
    let strict = BipersistenceArtifactLimits {
        max_circular_iterations: 1,
        ..limits
    };
    let error = artifact
        .record_circular_family(&module, &atlas, CircularCoordinateParams::default(), strict)
        .unwrap_err();
    assert!(error.to_string().contains("artifact limit"));
    assert_eq!(artifact.digest, digest);

    let bounded = failure_artifact();
    let bytes = bounded.encode(limits).unwrap();
    let decode_error = BipersistenceArtifact::decode(
        &bytes,
        BipersistenceArtifactLimits {
            max_circular_iterations: 0,
            ..limits
        },
    )
    .unwrap_err();
    assert!(decode_error.to_string().contains("iteration count"));

    let mut malformed = bounded;
    malformed.circular_families[0].max_iterations = usize::MAX;
    let verify_error = malformed.verify(limits).unwrap_err();
    assert!(verify_error.to_string().contains("artifact limit"));
}

#[test]
fn circular_family_failure_statuses_round_trip_through_checker() {
    let artifact = failure_artifact();
    let limits = BipersistenceArtifactLimits::default();
    let statuses =
        artifact.circular_families[0]
            .entries
            .iter()
            .fold([0usize; 4], |mut counts, entry| {
                let position = match &entry.status {
                    super::model::ArtifactCircularStatus::Success(_) => 0,
                    super::model::ArtifactCircularStatus::LiftFailed => 1,
                    super::model::ArtifactCircularStatus::SolveFailed => 2,
                    super::model::ArtifactCircularStatus::NotAttempted => 3,
                };
                counts[position] += 1;
                counts
            });
    assert!(statuses[1] + statuses[2] > 0);

    let bytes = artifact.encode(limits).unwrap();
    let decoded = BipersistenceArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.circular_family_successes, statuses[0]);
    assert_eq!(checked.circular_family_lift_failures, statuses[1]);
    assert_eq!(checked.circular_family_solve_failures, statuses[2]);
    assert_eq!(checked.circular_family_not_attempted, statuses[3]);
}

#[test]
fn checker_accepts_failure_annotation_that_linked_producer_replay_rejects() {
    let artifact = failure_artifact();
    let limits = BipersistenceArtifactLimits::default();
    let mut forged = artifact.clone();
    let entry = forged.circular_families[0]
        .entries
        .iter_mut()
        .find(|entry| {
            matches!(
                &entry.status,
                super::model::ArtifactCircularStatus::SolveFailed
            )
        })
        .expect("the bounded cycle family has a failed solve");
    entry.status = super::model::ArtifactCircularStatus::LiftFailed;
    let mut bytes = super::wire::encode_payload(&forged).unwrap();
    append_digest(&mut bytes);

    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default());
    assert!(checked.is_ok());
    let replay_error = BipersistenceArtifact::decode(&bytes, limits).unwrap_err();
    assert!(replay_error.to_string().contains("exact replay"));
}

#[test]
fn checker_rejects_unknown_family_status_after_digest_repair() {
    let artifact = failure_artifact();
    let mut bytes = artifact
        .encode(BipersistenceArtifactLimits::default())
        .unwrap();
    let offset = circular_entry_offsets(&bytes)[0];
    bytes[offset.status] = 4;
    refinalize(&mut bytes);
    let error = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap_err();
    assert!(error.to_string().contains("status is invalid"));
}

#[test]
fn checker_rejects_topology_status_mismatch_after_digest_repair() {
    let artifact = failure_artifact();
    let mut bytes = artifact
        .encode(BipersistenceArtifactLimits::default())
        .unwrap();
    let offset = circular_entry_offsets(&bytes)
        .into_iter()
        .find(|offset| offset.extension_code == 1)
        .expect("the family has a unique extension");
    bytes[offset.extension] = 2;
    refinalize(&mut bytes);
    let error = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("differs from its class extension")
    );
}
