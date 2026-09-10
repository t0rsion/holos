use sha2::{Digest, Sha256};

use crate::proof::{
    Block, Graph, ProofEdge, ProofError, canonicalize_diagram, diagrams_equal, h0_diagram,
    program_blocks,
};

use super::claim::{ProgramAtomClaim, ProgramClaim};
use super::class_verify::{check_spaces, same_optional_f64};
use super::graph::ProgramGraph;
use super::model::{ProgramProofLimits, VerifiedProgram};
use super::reduction::{atlas_digest, verify_reduction};

pub(super) use super::class_verify::{basis_class_id, canonical_basis, group_id};

pub(super) fn verify_program(
    claim: &ProgramClaim,
    source: &ProgramGraph,
    limits: ProgramProofLimits,
) -> Result<VerifiedProgram, ProofError> {
    check_program_binding(claim, source)?;
    let graph = source.proof_graph();
    let blocks = program_blocks(&graph, claim.threshold)?;
    let cyclic = cyclic_blocks(&blocks);
    check_atom_count(claim.atoms.len(), cyclic.len())?;
    let (total_spaces, reduction_columns) = verify_atoms(claim, source, &cyclic, limits)?;
    let diagram = compose_program_diagram(claim, &graph, limits)?;
    let h0_bars = diagram.iter().filter(|bar| bar.dimension == 0).count();
    let h1_bars = diagram.iter().filter(|bar| bar.dimension == 1).count();
    let checked = VerifiedProgram::from_claim(
        claim,
        source.num_edges(),
        blocks.len(),
        total_spaces,
        reduction_columns,
        h0_bars,
        h1_bars,
    );
    Ok(checked)
}

fn check_program_binding(claim: &ProgramClaim, source: &ProgramGraph) -> Result<(), ProofError> {
    if claim.vertex_count != source.vertex_count() {
        return Err(ProofError::new(
            "program vertex count differs from source graph",
        ));
    }
    if program_digest(source, claim.threshold) != claim.input_digest {
        return Err(ProofError::new(
            "program source graph binding does not match",
        ));
    }
    Ok(())
}

fn cyclic_blocks(blocks: &[Block]) -> Vec<(usize, &Block)> {
    blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block.edges.len() >= block.vertices.len())
        .collect()
}

fn check_atom_count(actual: usize, expected: usize) -> Result<(), ProofError> {
    if actual != expected {
        return Err(ProofError::new(format!(
            "program has {actual} atom records but the checked decomposition has {expected} cyclic atoms"
        )));
    }
    Ok(())
}

fn verify_atoms(
    claim: &ProgramClaim,
    source: &ProgramGraph,
    cyclic: &[(usize, &Block)],
    limits: ProgramProofLimits,
) -> Result<(usize, usize), ProofError> {
    let mut total_spaces = 0usize;
    let mut reduction_columns = 0usize;
    for (position, (atom, (block_id, block))) in claim.atoms.iter().zip(cyclic).enumerate() {
        check_atom_decomposition(atom, block, *block_id, position)?;
        let local = local_graph(atom, source)?;
        let checked = verify_atlas(atom, &local, claim.modulus, claim.threshold, limits)?;
        total_spaces = total_spaces
            .checked_add(checked.spaces)
            .ok_or_else(|| ProofError::new("class-space count overflows usize"))?;
        reduction_columns = reduction_columns
            .checked_add(checked.reduction_columns)
            .ok_or_else(|| ProofError::new("reduction-column count overflows usize"))?;
    }
    Ok((total_spaces, reduction_columns))
}

fn compose_program_diagram(
    claim: &ProgramClaim,
    graph: &Graph,
    limits: ProgramProofLimits,
) -> Result<Vec<crate::proof::ProofBar>, ProofError> {
    let mut diagram = h0_diagram(graph, claim.threshold)?;
    if diagram.len() > limits.max_bars {
        return Err(ProofError::new("program H0 diagram exceeds the bar limit"));
    }
    for atom in &claim.atoms {
        let h1_count = atom
            .atlas
            .diagram
            .iter()
            .filter(|bar| bar.dimension == 1)
            .count();
        let next = diagram
            .len()
            .checked_add(h1_count)
            .ok_or_else(|| ProofError::new("program diagram bar count overflows usize"))?;
        if next > limits.max_bars {
            return Err(ProofError::new("program diagram exceeds the bar limit"));
        }
        diagram.extend(
            atom.atlas
                .diagram
                .iter()
                .copied()
                .filter(|bar| bar.dimension == 1),
        );
    }
    canonicalize_diagram(&mut diagram);
    if !diagrams_equal(&diagram, &claim.diagram) {
        return Err(ProofError::new(
            "program diagram differs from the checked H0 and H1 composition",
        ));
    }
    Ok(diagram)
}

fn check_atom_decomposition(
    atom: &ProgramAtomClaim,
    block: &Block,
    block_id: usize,
    position: usize,
) -> Result<(), ProofError> {
    let expected_edges: Vec<_> = block.edges.clone();
    if atom.id != block_id || atom.vertices != block.vertices || atom.edges != expected_edges {
        return Err(ProofError::new(format!(
            "atom {position} differs from the checked articulation decomposition"
        )));
    }
    if atom.vertices.is_empty()
        || !atom.vertices.windows(2).all(|pair| pair[0] < pair[1])
        || atom.edges.len() < atom.vertices.len()
        || !atom.edges.windows(2).all(|pair| pair[0] < pair[1])
        || atom.edges.iter().any(|&[u, v]| {
            u >= v
                || atom.vertices.binary_search(&u).is_err()
                || atom.vertices.binary_search(&v).is_err()
        })
    {
        return Err(ProofError::new(format!(
            "atom {position} has noncanonical vertices or edges"
        )));
    }
    Ok(())
}

fn local_graph(atom: &ProgramAtomClaim, source: &ProgramGraph) -> Result<Graph, ProofError> {
    let edges = atom
        .edges
        .iter()
        .map(|&[u, v]| {
            let local_u = atom
                .vertices
                .binary_search(&u)
                .map_err(|_| ProofError::new("atom edge endpoint is outside its vertices"))?;
            let local_v = atom
                .vertices
                .binary_search(&v)
                .map_err(|_| ProofError::new("atom edge endpoint is outside its vertices"))?;
            Ok(ProofEdge {
                u: local_u,
                v: local_v,
                value: source.get(u, v),
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    Graph::new(atom.vertices.len(), &edges)
}

struct CheckedAtlas {
    spaces: usize,
    reduction_columns: usize,
}

fn verify_atlas(
    atom: &ProgramAtomClaim,
    graph: &Graph,
    program_modulus: u32,
    program_threshold: Option<f64>,
    limits: ProgramProofLimits,
) -> Result<CheckedAtlas, ProofError> {
    let atlas = &atom.atlas;
    check_atlas_header(atlas, graph, program_modulus, program_threshold)?;
    let reduction = &atlas.reduction;
    check_reduction_header(reduction, atlas)?;
    let checked = verify_reduction(graph, reduction, limits)?;
    check_spaces(atlas, graph, &checked.h1_pairs, limits)?;
    let reduction_columns = checked
        .edge_columns
        .checked_add(checked.triangle_columns)
        .ok_or_else(|| ProofError::new("reduction-column count overflows usize"))?;
    Ok(CheckedAtlas {
        spaces: atlas.spaces.len(),
        reduction_columns,
    })
}

fn check_atlas_header(
    atlas: &super::claim::AtlasClaim,
    graph: &Graph,
    program_modulus: u32,
    program_threshold: Option<f64>,
) -> Result<(), ProofError> {
    if atlas.modulus != program_modulus
        || atlas.vertex_count != graph.vertex_count
        || !same_optional_f64(atlas.threshold, program_threshold)
    {
        return Err(ProofError::new(
            "atlas header differs from its program atom",
        ));
    }
    if atlas.input_digest != atlas_digest(graph, atlas.threshold) {
        return Err(ProofError::new(
            "atlas graph binding does not match its atom",
        ));
    }
    Ok(())
}

fn check_reduction_header(
    reduction: &super::claim::ReductionClaim,
    atlas: &super::claim::AtlasClaim,
) -> Result<(), ProofError> {
    if reduction.modulus != atlas.modulus
        || reduction.vertex_count != atlas.vertex_count
        || !same_optional_f64(reduction.threshold, atlas.threshold)
        || !diagrams_equal(&reduction.diagram, &atlas.diagram)
    {
        return Err(ProofError::new(
            "reduction header or diagram differs from the atlas",
        ));
    }
    Ok(())
}

pub(super) fn program_digest(graph: &ProgramGraph, threshold: Option<f64>) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-program-graph-v1");
    hash.update((graph.vertex_count() as u64).to_be_bytes());
    match threshold {
        None => hash.update([0]),
        Some(value) => {
            hash.update([1]);
            hash.update(value.to_bits().to_be_bytes());
        }
    }
    hash.update((graph.num_edges() as u64).to_be_bytes());
    for edge in graph.edges() {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(edge.value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}
