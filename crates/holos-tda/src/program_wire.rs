//! Portable proof-carrying compositional persistence programs.
//!
//! A `HOLOSPRG` envelope binds the complete listed graph, its articulation
//! decomposition, one independently checked atlas per cyclic atom, and the
//! composed H0 and H1 diagram. Verification does not call the persistence
//! solver.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::factorization::program_blocks;
use crate::program::{ProgramAtomState, atom_infos, compose_result, local_matrix};
use crate::{
    AtlasArtifact, AtlasDecodeLimits, Bar, CertificateLimits, Diagram, EdgeKey, PersistenceProgram,
    RipsParams, SparseDistanceMatrix,
};

const MAGIC: &[u8; 8] = b"HOLOSPRG";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Failure while producing, decoding, or checking a program artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramArtifactError {
    message: String,
}

impl ProgramArtifactError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated artifact rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProgramArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "program artifact: {}", self.message)
    }
}

impl std::error::Error for ProgramArtifactError {}

/// Decoder limits applied before program collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProgramDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted cyclic-atom count.
    pub max_atoms: usize,
    /// Largest accepted total atom vertex count.
    pub max_atom_vertices: usize,
    /// Largest accepted total atom edge count.
    pub max_atom_edges: usize,
    /// Largest accepted diagram bar count.
    pub max_bars: usize,
    /// Largest accepted total nested atlas bytes.
    pub max_atlas_bytes: usize,
    /// Limits for each nested atlas envelope.
    pub atlas: AtlasDecodeLimits,
}

impl Default for ProgramDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 1_000_000,
            max_atoms: 50_000_000,
            max_atom_vertices: 100_000_000,
            max_atom_edges: 100_000_000,
            max_bars: 100_000_000,
            max_atlas_bytes: 1 << 30,
            atlas: AtlasDecodeLimits::default(),
        }
    }
}

/// One cyclic atom and its nested proof-carrying atlas.
#[derive(Debug, Clone)]
pub struct ProgramAtomArtifact {
    id: usize,
    vertices: Vec<usize>,
    edges: Vec<EdgeKey>,
    atlas: AtlasArtifact,
}

impl ProgramAtomArtifact {
    /// Position in the complete articulation decomposition.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Original labeled vertices.
    pub fn vertices(&self) -> &[usize] {
        &self.vertices
    }

    /// Original labeled edges.
    pub fn edges(&self) -> &[EdgeKey] {
        &self.edges
    }

    /// Nested independently checked atlas.
    pub fn atlas(&self) -> &AtlasArtifact {
        &self.atlas
    }
}

/// Input binding, articulation program, and independently checked atom proofs.
#[derive(Debug, Clone)]
pub struct ProgramArtifact {
    vertex_count: usize,
    threshold: Option<f64>,
    modulus: u32,
    input_digest: [u8; 32],
    diagram: Diagram,
    atoms: Vec<ProgramAtomArtifact>,
}

impl ProgramArtifact {
    /// Capture the current checked state of a compiled program.
    ///
    /// This operation runs no persistence reduction. The returned artifact
    /// binds the graph most recently supplied to the program.
    pub fn from_program(
        program: &PersistenceProgram,
    ) -> std::result::Result<Self, ProgramArtifactError> {
        Self::capture(program.current_graph(), program)
    }

    /// Produce a proof-carrying compositional program.
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramArtifactError> {
        Self::compile(input, params, certificate_limits).map(|(artifact, _)| artifact)
    }

    /// Produce an artifact and retain its ready-to-update program.
    pub fn compile(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<(Self, PersistenceProgram), ProgramArtifactError> {
        let program = PersistenceProgram::compile(input, params, certificate_limits)
            .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
        let artifact = Self::capture(input, &program)?;
        Ok((artifact, program))
    }

    pub(crate) fn capture(
        input: &SparseDistanceMatrix,
        program: &PersistenceProgram,
    ) -> std::result::Result<Self, ProgramArtifactError> {
        let atoms = program
            .states()
            .iter()
            .map(|state| ProgramAtomArtifact {
                id: state.info_index,
                vertices: state.vertices.clone(),
                edges: state.edges.clone(),
                atlas: state.artifact.clone(),
            })
            .collect();
        let artifact = Self {
            vertex_count: input.len(),
            threshold: program.params().threshold,
            modulus: program.params().modulus,
            input_digest: program_graph_digest(input, program.params().threshold),
            diagram: program.result().diagram.clone(),
            atoms,
        };
        artifact.check_structure(ProgramDecodeLimits::default())?;
        Ok(artifact)
    }

    /// Diagram carried by the program.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// Number of vertices bound to the program.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Cyclic atoms and their nested proofs.
    pub fn atoms(&self) -> &[ProgramAtomArtifact] {
        &self.atoms
    }

    /// Encode the canonical `HOLOSPRG` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, ProgramArtifactError> {
        self.check_structure(ProgramDecodeLimits::default())?;
        let nested = self
            .atoms
            .iter()
            .map(|atom| {
                atom.atlas
                    .encode()
                    .map_err(|error| ProgramArtifactError::new(error.to_string()))
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u16(&mut out, WIRE_VERSION);
        out.push(F64_BITS_CODEC);
        put_u32(&mut out, self.modulus);
        put_usize(&mut out, self.vertex_count, "vertex count")?;
        put_optional_f64(&mut out, self.threshold);
        put_usize(&mut out, self.diagram.bars.len(), "bar count")?;
        put_usize(&mut out, self.atoms.len(), "atom count")?;
        out.extend_from_slice(&self.input_digest);
        for bar in &self.diagram.bars {
            put_usize(&mut out, bar.dim, "bar dimension")?;
            put_u64(&mut out, bar.birth.to_bits());
            put_u64(&mut out, bar.death.to_bits());
        }
        for (atom, atlas) in self.atoms.iter().zip(nested) {
            put_usize(&mut out, atom.id, "atom identifier")?;
            put_usize(&mut out, atom.vertices.len(), "atom vertex count")?;
            put_usize(&mut out, atom.edges.len(), "atom edge count")?;
            put_usize(&mut out, atlas.len(), "nested atlas byte count")?;
            for &vertex in &atom.vertices {
                put_usize(&mut out, vertex, "atom vertex")?;
            }
            for edge in &atom.edges {
                put_usize(&mut out, edge.u, "atom edge endpoint")?;
                put_usize(&mut out, edge.v, "atom edge endpoint")?;
            }
            out.extend_from_slice(&atlas);
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded program envelope.
    pub fn decode(
        bytes: &[u8],
        limits: ProgramDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramArtifactError> {
        if bytes.len() > limits.max_bytes {
            return Err(ProgramArtifactError::new(format!(
                "{} bytes exceed the decoder limit {}",
                bytes.len(),
                limits.max_bytes
            )));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != MAGIC {
            return Err(ProgramArtifactError::new("wrong magic bytes"));
        }
        let version = reader.u16()?;
        if version != WIRE_VERSION {
            return Err(ProgramArtifactError::new(format!(
                "unsupported wire version {version}"
            )));
        }
        if reader.u8()? != F64_BITS_CODEC {
            return Err(ProgramArtifactError::new("unsupported scalar codec"));
        }
        let modulus = reader.u32()?;
        let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
        let threshold = reader.optional_f64()?;
        let bar_count = reader.bounded_usize("bar count", limits.max_bars)?;
        let atom_count = reader.bounded_usize("atom count", limits.max_atoms)?;
        let input_digest = reader.array32()?;
        let minimum = bar_count
            .checked_mul(24)
            .and_then(|bars| {
                atom_count
                    .checked_mul(32)
                    .and_then(|atoms| bars.checked_add(atoms))
            })
            .ok_or_else(|| ProgramArtifactError::new("minimum record bytes overflow usize"))?;
        if minimum > reader.remaining() {
            return Err(ProgramArtifactError::new(
                "record counts exceed the remaining bytes",
            ));
        }
        let mut bars = Vec::with_capacity(bar_count);
        for _ in 0..bar_count {
            bars.push(Bar {
                dim: reader.usize()?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            });
        }
        let mut atoms = Vec::with_capacity(atom_count);
        let mut total_vertices = 0usize;
        let mut total_edges = 0usize;
        let mut total_atlas_bytes = 0usize;
        for _ in 0..atom_count {
            let id = reader.usize()?;
            let vertex_count = reader.usize()?;
            let edge_count = reader.usize()?;
            let atlas_bytes = reader.usize()?;
            total_vertices = bounded_sum(
                total_vertices,
                vertex_count,
                limits.max_atom_vertices,
                "atom vertices",
            )?;
            total_edges =
                bounded_sum(total_edges, edge_count, limits.max_atom_edges, "atom edges")?;
            total_atlas_bytes = bounded_sum(
                total_atlas_bytes,
                atlas_bytes,
                limits.max_atlas_bytes,
                "nested atlas bytes",
            )?;
            let fixed = vertex_count
                .checked_mul(8)
                .and_then(|vertices| {
                    edge_count
                        .checked_mul(16)
                        .and_then(|edges| vertices.checked_add(edges))
                })
                .and_then(|bytes| bytes.checked_add(atlas_bytes))
                .ok_or_else(|| ProgramArtifactError::new("atom record bytes overflow usize"))?;
            if fixed > reader.remaining() {
                return Err(ProgramArtifactError::new(
                    "atom record exceeds the remaining bytes",
                ));
            }
            let mut vertices = Vec::with_capacity(vertex_count);
            for _ in 0..vertex_count {
                vertices.push(reader.usize()?);
            }
            let mut edges = Vec::with_capacity(edge_count);
            for _ in 0..edge_count {
                edges.push(EdgeKey {
                    u: reader.usize()?,
                    v: reader.usize()?,
                });
            }
            let atlas =
                AtlasArtifact::decode(reader.take(atlas_bytes)?, limits.atlas, certificate_limits)
                    .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
            atoms.push(ProgramAtomArtifact {
                id,
                vertices,
                edges,
                atlas,
            });
        }
        if reader.remaining() != 0 {
            return Err(ProgramArtifactError::new(format!(
                "{} trailing bytes after the envelope",
                reader.remaining()
            )));
        }
        let artifact = Self {
            vertex_count,
            threshold,
            modulus,
            input_digest,
            diagram: Diagram { bars },
            atoms,
        };
        artifact.check_structure(limits)?;
        Ok(artifact)
    }

    /// Verify all atom proofs and reconstruct the program without the solver.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<PersistenceProgram, ProgramArtifactError> {
        self.check_structure(ProgramDecodeLimits::default())?;
        if input.len() != self.vertex_count
            || program_graph_digest(input, self.threshold) != self.input_digest
        {
            return Err(ProgramArtifactError::new(
                "complete input graph binding does not match",
            ));
        }
        let mut params = RipsParams::new(1).with_modulus(self.modulus);
        params.threshold = self.threshold;
        let (_, blocks) = program_blocks(input, self.threshold)
            .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
        let infos = atom_infos(input, &blocks);
        let cyclic: Vec<_> = infos.iter().filter(|atom| atom.cyclic).collect();
        if cyclic.len() != self.atoms.len() {
            return Err(ProgramArtifactError::new(
                "cyclic atom count differs from the checked decomposition",
            ));
        }
        let mut states = Vec::with_capacity(self.atoms.len());
        let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        for (record, expected) in self.atoms.iter().zip(cyclic) {
            if record.id != expected.id
                || record.vertices != expected.vertices
                || record.edges != expected.edges
            {
                return Err(ProgramArtifactError::new(format!(
                    "atom {} differs from the checked decomposition",
                    record.id
                )));
            }
            let local = local_matrix(&record.vertices, &record.edges, input)
                .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
            record
                .atlas
                .verify(&local, certificate_limits)
                .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
            let region = record
                .atlas
                .reduction_certificate()
                .compile_region(&local, certificate_limits)
                .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
            states.push(ProgramAtomState {
                info_index: record.id,
                vertices: record.vertices.clone(),
                edges: record.edges.clone(),
                edge_positions: record
                    .edges
                    .iter()
                    .map(|edge| {
                        topology
                            .binary_search(edge)
                            .expect("checked atom edge is in the program topology")
                    })
                    .collect(),
                artifact: record.atlas.clone(),
                certified_graph: local,
                region,
                explained: record.atlas.explained().clone(),
            });
        }
        let result = compose_result(input, &params, &states)
            .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
        if !diagram_bits_equal(&result.diagram, &self.diagram) {
            return Err(ProgramArtifactError::new(
                "composed diagram differs from the recorded diagram",
            ));
        }
        Ok(PersistenceProgram::from_verified_parts(
            input,
            &params,
            certificate_limits,
            infos,
            states,
            result,
        ))
    }

    fn check_structure(
        &self,
        limits: ProgramDecodeLimits,
    ) -> std::result::Result<(), ProgramArtifactError> {
        if self.vertex_count > limits.max_vertices {
            return Err(ProgramArtifactError::new(format!(
                "{} vertices exceed the limit {}",
                self.vertex_count, limits.max_vertices
            )));
        }
        checked_threshold(self.threshold)?;
        if self.atoms.len() > limits.max_atoms {
            return Err(ProgramArtifactError::new(format!(
                "{} atoms exceed the limit {}",
                self.atoms.len(),
                limits.max_atoms
            )));
        }
        let mut canonical = self.diagram.clone();
        canonical.canonicalize();
        if !diagram_bits_equal(&canonical, &self.diagram) {
            return Err(ProgramArtifactError::new("bars are not in canonical order"));
        }
        for bar in &self.diagram.bars {
            if bar.dim > 1
                || !bar.birth.is_finite()
                || bar.birth < 0.0
                || bar.death.is_nan()
                || bar.death < 0.0
                || bar.death <= bar.birth
            {
                return Err(ProgramArtifactError::new("diagram contains an invalid bar"));
            }
        }
        let mut total_vertices = 0usize;
        let mut total_edges = 0usize;
        let mut previous_id = None;
        for atom in &self.atoms {
            if previous_id.is_some_and(|previous| previous >= atom.id) {
                return Err(ProgramArtifactError::new(
                    "atom identifiers are not strictly ordered",
                ));
            }
            previous_id = Some(atom.id);
            total_vertices = bounded_sum(
                total_vertices,
                atom.vertices.len(),
                limits.max_atom_vertices,
                "atom vertices",
            )?;
            total_edges = bounded_sum(
                total_edges,
                atom.edges.len(),
                limits.max_atom_edges,
                "atom edges",
            )?;
            if atom.vertices.is_empty()
                || !atom.vertices.windows(2).all(|pair| pair[0] < pair[1])
                || atom
                    .vertices
                    .iter()
                    .any(|&vertex| vertex >= self.vertex_count)
            {
                return Err(ProgramArtifactError::new(format!(
                    "atom {} has noncanonical vertices",
                    atom.id
                )));
            }
            if atom.edges.len() < atom.vertices.len()
                || !atom.edges.windows(2).all(|pair| pair[0] < pair[1])
                || atom.edges.iter().any(|edge| {
                    edge.u >= edge.v
                        || atom.vertices.binary_search(&edge.u).is_err()
                        || atom.vertices.binary_search(&edge.v).is_err()
                })
            {
                return Err(ProgramArtifactError::new(format!(
                    "atom {} has noncanonical edges",
                    atom.id
                )));
            }
            if atom.atlas.vertex_count() != atom.vertices.len()
                || atom.atlas.threshold().map(f64::to_bits) != self.threshold.map(f64::to_bits)
                || atom.atlas.modulus() != self.modulus
            {
                return Err(ProgramArtifactError::new(format!(
                    "atom {} atlas header differs from the program",
                    atom.id
                )));
            }
        }
        Ok(())
    }
}

fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> std::result::Result<usize, ProgramArtifactError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProgramArtifactError::new(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(ProgramArtifactError::new(format!(
            "{next} {label} exceed the limit {limit}"
        )));
    }
    Ok(next)
}

fn checked_threshold(threshold: Option<f64>) -> std::result::Result<f64, ProgramArtifactError> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(ProgramArtifactError::new(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

fn program_graph_digest(input: &SparseDistanceMatrix, threshold: Option<f64>) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-program-graph-v1");
    hash.update((input.len() as u64).to_be_bytes());
    match threshold {
        None => hash.update([0]),
        Some(value) => {
            hash.update([1]);
            hash.update(value.to_bits().to_be_bytes());
        }
    }
    hash.update((input.num_edges() as u64).to_be_bytes());
    for (u, v, value) in input.edges() {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), ProgramArtifactError> {
    let value = u64::try_from(value)
        .map_err(|_| ProgramArtifactError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

fn put_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value.to_bits());
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], ProgramArtifactError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProgramArtifactError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(ProgramArtifactError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, ProgramArtifactError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, ProgramArtifactError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> std::result::Result<u32, ProgramArtifactError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, ProgramArtifactError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, ProgramArtifactError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProgramArtifactError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, ProgramArtifactError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProgramArtifactError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_f64(&mut self) -> std::result::Result<Option<f64>, ProgramArtifactError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(ProgramArtifactError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }

    fn array32(&mut self) -> std::result::Result<[u8; 32], ProgramArtifactError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn graph() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, 4.5),
            ],
        )
        .unwrap()
    }

    #[test]
    fn program_round_trips_and_verifies_without_the_solver() {
        let input = graph();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let artifact =
                ProgramArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
            let bytes = artifact.encode().unwrap();
            let decoded = ProgramArtifact::decode(
                &bytes,
                ProgramDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(decoded.encode().unwrap(), bytes);
            let program = decoded
                .verify(&input, CertificateLimits::default())
                .unwrap();
            assert!(diagram_bits_equal(
                &program.result().diagram,
                artifact.diagram()
            ));
        }
    }

    #[test]
    fn mutation_wrong_input_and_limits_are_rejected() {
        let input = graph();
        let params = RipsParams::new(1);
        let artifact =
            ProgramArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
        let bytes = artifact.encode().unwrap();
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        assert!(
            ProgramArtifact::decode(
                &changed,
                ProgramDecodeLimits::default(),
                CertificateLimits::default()
            )
            .is_err()
        );
        assert!((0..bytes.len()).all(|end| {
            ProgramArtifact::decode(
                &bytes[..end],
                ProgramDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .is_err()
        }));
        let other = SparseDistanceMatrix::from_triplets(
            7,
            &[(0, 1, 1.0), (1, 2, 2.0), (2, 3, 3.0), (0, 3, 5.0)],
        )
        .unwrap();
        assert!(
            artifact
                .verify(&other, CertificateLimits::default())
                .is_err()
        );
        let limits = ProgramDecodeLimits {
            max_atoms: 1,
            ..ProgramDecodeLimits::default()
        };
        assert!(ProgramArtifact::decode(&bytes, limits, CertificateLimits::default()).is_err());
    }

    proptest! {
        #[test]
        fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = ProgramArtifact::decode(
                &bytes,
                ProgramDecodeLimits::default(),
                CertificateLimits::default(),
            );
        }
    }
}
