use super::claim::ProgramClaim;

/// Decoder and verification limits for one `HOLOSPRG` artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProgramProofLimits {
    /// Largest accepted program envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted source vertex count.
    pub max_vertices: usize,
    /// Largest accepted cyclic-atom count.
    pub max_atoms: usize,
    /// Largest accepted total atom vertex count.
    pub max_atom_vertices: usize,
    /// Largest accepted total atom edge count.
    pub max_atom_edges: usize,
    /// Largest accepted total diagram bar count.
    pub max_bars: usize,
    /// Largest accepted total nested atlas bytes.
    pub max_atlas_bytes: usize,
    /// Largest accepted atlas envelope in bytes.
    pub max_nested_atlas_bytes: usize,
    /// Largest accepted atlas class-space count.
    pub max_spaces: usize,
    /// Largest accepted total class-space basis count.
    pub max_basis: usize,
    /// Largest accepted total critical-pair count.
    pub max_critical_pairs: usize,
    /// Largest accepted total cocycle term count.
    pub max_cocycle_terms: usize,
    /// Largest accepted nested reduction envelope in bytes.
    pub max_certificate_bytes: usize,
    /// Largest accepted filtered edge-column count in one reduction.
    pub max_edges: usize,
    /// Largest accepted filtered triangle-column count in one reduction.
    pub max_triangles: usize,
    /// Largest accepted total reduction change-of-basis terms.
    pub max_terms: usize,
}

impl Default for ProgramProofLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 1_000_000,
            max_atoms: 50_000_000,
            max_atom_vertices: 100_000_000,
            max_atom_edges: 100_000_000,
            max_bars: 100_000_000,
            max_atlas_bytes: 1 << 30,
            max_nested_atlas_bytes: 1 << 30,
            max_spaces: 50_000_000,
            max_basis: 100_000_000,
            max_critical_pairs: 100_000_000,
            max_cocycle_terms: 200_000_000,
            max_certificate_bytes: 1 << 30,
            max_edges: 20_000_000,
            max_triangles: 100_000_000,
            max_terms: 200_000_000,
        }
    }
}

/// Summary of a verified `HOLOSPRG` artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedProgram {
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Number of source vertices.
    pub vertices: usize,
    /// Number of listed source edges.
    pub edges: usize,
    /// Number of decomposition atoms.
    pub atoms: usize,
    /// Number of cyclic atoms with nested atlases.
    pub cyclic_atoms: usize,
    /// Number of checked diagram bars.
    pub bars: usize,
    /// Number of checked H0 bars.
    pub h0_bars: usize,
    /// Number of checked H1 bars.
    pub h1_bars: usize,
    /// Number of nested class spaces.
    pub class_spaces: usize,
    /// Number of nested reduction columns checked.
    pub reduction_columns: usize,
}

impl VerifiedProgram {
    pub(super) fn from_claim(
        claim: &ProgramClaim,
        source_edges: usize,
        atoms: usize,
        atlas_spaces: usize,
        reduction_columns: usize,
        h0_bars: usize,
        h1_bars: usize,
    ) -> Self {
        Self {
            modulus: claim.modulus,
            vertices: claim.vertex_count,
            edges: source_edges,
            atoms,
            cyclic_atoms: claim.atoms.len(),
            bars: claim.diagram.len(),
            h0_bars,
            h1_bars,
            class_spaces: atlas_spaces,
            reduction_columns,
        }
    }
}
