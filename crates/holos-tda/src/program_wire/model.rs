use std::fmt;

use crate::{
    AtlasDecodeLimits, CertificateLimits, Diagram, PersistenceProgram, RipsParams,
    SparseDistanceMatrix,
};

use super::{decode, encode, verification};

/// Failure while producing, decoding, or checking a program artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramArtifactError {
    message: String,
}

impl ProgramArtifactError {
    pub(super) fn new(message: impl Into<String>) -> Self {
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

/// One cyclic atom and its nested atlas.
#[derive(Debug, Clone)]
pub struct ProgramAtomArtifact {
    pub(super) id: usize,
    pub(super) vertices: Vec<usize>,
    pub(super) edges: Vec<crate::EdgeKey>,
    pub(super) atlas: crate::AtlasArtifact,
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
    pub fn edges(&self) -> &[crate::EdgeKey] {
        &self.edges
    }

    /// Nested atlas.
    pub fn atlas(&self) -> &crate::AtlasArtifact {
        &self.atlas
    }
}

/// Input binding, articulation program, and nested atom atlases.
#[derive(Debug, Clone)]
pub struct ProgramArtifact {
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) modulus: u32,
    pub(super) input_digest: [u8; 32],
    pub(super) diagram: Diagram,
    pub(super) atoms: Vec<ProgramAtomArtifact>,
}

impl ProgramArtifact {
    /// Capture the current state of a compiled program.
    ///
    /// Persistence reduction does not run. The artifact binds the graph
    /// most recently supplied to the program.
    pub fn from_program(
        program: &PersistenceProgram,
    ) -> std::result::Result<Self, ProgramArtifactError> {
        Self::capture(program.current_graph(), program)
    }

    /// Produce a compositional program artifact.
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
            input_digest: verification::program_graph_digest(input, program.params().threshold),
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
        let nested = encode::encode_atlases(&self.atoms)?;
        let mut out = Vec::new();
        encode::encode_program_header(&mut out, self)?;
        encode::encode_bars(&mut out, &self.diagram.bars)?;
        for (atom, atlas) in self.atoms.iter().zip(nested) {
            encode::encode_atom(&mut out, atom, &atlas)?;
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded program envelope.
    pub fn decode(
        bytes: &[u8],
        limits: ProgramDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramArtifactError> {
        decode::check_envelope_size(bytes, limits.max_bytes)?;
        let mut reader = super::primitives::Reader::new(bytes);
        let header = decode::decode_program_header(&mut reader, limits)?;
        decode::check_minimum_record_bytes(&reader, header.bar_count, header.atom_count)?;
        let bars = decode::decode_bars(&mut reader, header.bar_count)?;
        let atoms =
            decode::decode_atoms(&mut reader, header.atom_count, limits, certificate_limits)?;
        decode::check_no_trailing_bytes(&reader)?;
        let artifact = Self {
            vertex_count: header.vertex_count,
            threshold: header.threshold,
            modulus: header.modulus,
            input_digest: header.input_digest,
            diagram: Diagram { bars },
            atoms,
        };
        artifact.check_structure(limits)?;
        Ok(artifact)
    }

    /// Verify all atom proofs and reconstruct the program.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<PersistenceProgram, ProgramArtifactError> {
        self.check_structure(ProgramDecodeLimits::default())?;
        verification::check_input_binding(self, input)?;
        let mut params = RipsParams::new(1).with_modulus(self.modulus);
        params.threshold = self.threshold;
        let (_, blocks) = crate::factorization::program_blocks(input, self.threshold)
            .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
        let infos = crate::program::atom_infos(input, &blocks);
        let topology: Vec<_> = input
            .edges()
            .map(|(u, v, _)| crate::EdgeKey::new(u, v))
            .collect();
        let states =
            verification::verify_atom_states(self, input, &infos, &topology, certificate_limits)?;
        let result = crate::program::compose_result(input, &params, &states)
            .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
        verification::check_composed_diagram(&result.diagram, &self.diagram)?;
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
        verification::check_program_limits(self, limits)?;
        verification::check_program_diagram(&self.diagram)?;
        verification::check_program_atoms(self, limits)?;
        Ok(())
    }
}
