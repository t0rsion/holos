//! Persistent-class JSON records shared by computation and circular input.

use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    Bar, BasisClassId, Cocycle, CocycleTerm, ExplainedDiagram, IntervalGroupId, PersistentClass,
    PersistentClassProvenance,
};

use super::input::{read_bounded_artifact, write_via_temporary};

const FORMAT: &str = "holos-h1-class-spaces-v2";
const PROVENANCE_SCHEMA: &str = "holos-persistent-class-source-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassDocument {
    format: String,
    spaces: Vec<SpaceRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceRecord {
    id: String,
    birth: f64,
    death: Option<f64>,
    essential: bool,
    multiplicity: usize,
    basis: Vec<BasisRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasisRecord {
    id: String,
    index: usize,
    modulus: u32,
    scale: f64,
    terms: Vec<(usize, usize, u32)>,
    provenance: Option<ProvenanceRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceRecord {
    schema: String,
    active_graph_digest: String,
    class_digest: String,
    interval: (usize, f64, Option<f64>),
    birth: f64,
    death: Option<f64>,
    modulus: u32,
    scale: f64,
}

pub(super) fn write_representatives(
    path: &Path,
    explained: &ExplainedDiagram,
) -> crate::Result<()> {
    let document = ClassDocument {
        format: FORMAT.to_string(),
        spaces: explained.spaces.iter().map(space_record).collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&document).map_err(|error| {
        crate::Error::InvalidInput(format!("cannot encode H1 class records: {error}"))
    })?;
    bytes.push(b'\n');
    write_via_temporary(path, &bytes)?;
    eprintln!(
        "H1 representatives: wrote {} classes to {}",
        explained.class_count(),
        path.display()
    );
    Ok(())
}

pub(super) fn read_persistent_class(
    path: &Path,
    space_index: usize,
    basis_index: usize,
    maximum_bytes: usize,
) -> crate::Result<PersistentClass> {
    let document = read_class_document(path, maximum_bytes)?;
    let (space, basis) = select_class_records(&document, space_index, basis_index)?;
    build_persistent_class(space, basis, basis_index)
}

fn read_class_document(path: &Path, maximum_bytes: usize) -> crate::Result<ClassDocument> {
    let bytes = read_bounded_artifact(path, maximum_bytes, "persistent class record")?;
    let document: ClassDocument = serde_json::from_slice(&bytes).map_err(|error| {
        crate::Error::InvalidInput(format!(
            "persistent class record {} is invalid JSON: {error}",
            path.display()
        ))
    })?;
    if document.format != FORMAT {
        return Err(crate::Error::InvalidInput(format!(
            "persistent class record format must be {FORMAT}"
        )));
    }
    Ok(document)
}

fn select_class_records(
    document: &ClassDocument,
    space_index: usize,
    basis_index: usize,
) -> crate::Result<(&SpaceRecord, &BasisRecord)> {
    let space = document.spaces.get(space_index).ok_or_else(|| {
        crate::Error::InvalidInput(format!(
            "persistent class space index {space_index} is outside 0..{}",
            document.spaces.len()
        ))
    })?;
    validate_space(space)?;
    let basis = space.basis.get(basis_index).ok_or_else(|| {
        crate::Error::InvalidInput(format!(
            "persistent class basis index {basis_index} is outside 0..{}",
            space.basis.len()
        ))
    })?;
    if basis.index != basis_index {
        return Err(crate::Error::InvalidInput(format!(
            "persistent class basis position {basis_index} declares index {}",
            basis.index
        )));
    }
    Ok((space, basis))
}

fn build_persistent_class(
    space: &SpaceRecord,
    basis: &BasisRecord,
    basis_index: usize,
) -> crate::Result<PersistentClass> {
    let interval = Bar {
        dim: 1,
        birth: space.birth,
        death: space.death.unwrap_or(f64::INFINITY),
    };
    let provenance = basis.provenance.as_ref().ok_or_else(|| {
        crate::Error::InvalidInput(
            "selected persistent class has no scalar source provenance".into(),
        )
    })?;
    validate_provenance_record(provenance, interval, basis)?;
    let class_digest = parse_digest(&provenance.class_digest, "class digest")?;
    let class_id = BasisClassId::from_bytes(parse_digest(&basis.id, "class id")?);
    let group_id = IntervalGroupId::from_bytes(parse_digest(&space.id, "class space id")?);
    let terms = basis
        .terms
        .iter()
        .map(|&(u, v, coefficient)| CocycleTerm { u, v, coefficient })
        .collect();
    let source_graph_digest = parse_digest(&provenance.active_graph_digest, "active graph digest")?;
    Ok(PersistentClass {
        id: class_id,
        group_id,
        basis_index,
        interval,
        cocycle: Cocycle {
            modulus: basis.modulus,
            scale: basis.scale,
            terms,
        },
        provenance: Some(PersistentClassProvenance::from_parts(
            source_graph_digest,
            class_digest,
            interval,
            provenance.modulus,
            provenance.scale,
        )),
    })
}

fn space_record(space: &crate::PersistentClassSpace) -> SpaceRecord {
    SpaceRecord {
        id: space.id.to_string(),
        birth: space.interval.birth,
        death: finite_death(space.interval),
        essential: space.interval.is_essential(),
        multiplicity: space.basis.len(),
        basis: space.basis.iter().map(basis_record).collect(),
    }
}

fn basis_record(class: &PersistentClass) -> BasisRecord {
    BasisRecord {
        id: class.id.to_string(),
        index: class.basis_index,
        modulus: class.cocycle.modulus,
        scale: class.cocycle.scale,
        terms: class
            .cocycle
            .terms
            .iter()
            .map(|term| (term.u, term.v, term.coefficient))
            .collect(),
        provenance: class.provenance().map(|provenance| ProvenanceRecord {
            schema: PROVENANCE_SCHEMA.to_string(),
            active_graph_digest: hex_digest(provenance.source_graph_digest()),
            class_digest: hex_digest(provenance.class_digest()),
            interval: (
                provenance.interval().dim,
                provenance.interval().birth,
                finite_death(provenance.interval()),
            ),
            birth: provenance.interval().birth,
            death: finite_death(provenance.interval()),
            modulus: provenance.modulus(),
            scale: provenance.scale(),
        }),
    }
}

fn validate_space(space: &SpaceRecord) -> crate::Result<()> {
    if space.essential != space.death.is_none() {
        return Err(crate::Error::InvalidInput(
            "persistent class space has inconsistent essential status".into(),
        ));
    }
    if space.multiplicity != space.basis.len() {
        return Err(crate::Error::InvalidInput(
            "persistent class space multiplicity differs from its basis".into(),
        ));
    }
    Ok(())
}

fn validate_provenance_record(
    provenance: &ProvenanceRecord,
    interval: Bar,
    basis: &BasisRecord,
) -> crate::Result<()> {
    let death = finite_death(interval);
    let matches = provenance.schema == PROVENANCE_SCHEMA
        && provenance.interval.0 == 1
        && same_number(provenance.interval.1, interval.birth)
        && same_optional_number(provenance.interval.2, death)
        && same_number(provenance.birth, interval.birth)
        && same_optional_number(provenance.death, death)
        && provenance.modulus == basis.modulus
        && same_number(provenance.scale, basis.scale);
    if !matches {
        return Err(crate::Error::InvalidInput(
            "persistent class provenance differs from its class record".into(),
        ));
    }
    Ok(())
}

fn parse_digest(value: &str, label: &str) -> crate::Result<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(crate::Error::InvalidInput(format!(
            "persistent class {label} must be 64 lowercase hexadecimal digits"
        )));
    }
    let mut digest = [0u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[2 * index..2 * index + 2], 16)
            .expect("validated hexadecimal digits");
    }
    Ok(digest)
}

fn finite_death(interval: Bar) -> Option<f64> {
    interval.death.is_finite().then_some(interval.death)
}

fn same_number(left: f64, right: f64) -> bool {
    left.to_bits() == right.to_bits()
}

fn same_optional_number(left: Option<f64>, right: Option<f64>) -> bool {
    left.map(f64::to_bits) == right.map(f64::to_bits)
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut value = String::with_capacity(64);
    for byte in digest {
        write!(value, "{byte:02x}").expect("writing to a string cannot fail");
    }
    value
}
