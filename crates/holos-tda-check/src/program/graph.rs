use std::collections::BTreeMap;

use crate::ProofError;
use crate::proof::{Graph, ProofEdge};

/// One finite, undirected source edge for a persistence program.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgramEdge {
    /// Lower endpoint.
    pub u: usize,
    /// Higher endpoint.
    pub v: usize,
    /// Non-negative finite filtration value.
    pub value: f64,
}

impl ProgramEdge {
    /// Construct one edge. Endpoints are validated by [`ProgramGraph::new`].
    pub const fn new(u: usize, v: usize, value: f64) -> Self {
        Self { u, v, value }
    }
}

impl From<(usize, usize, f64)> for ProgramEdge {
    fn from((u, v, value): (usize, usize, f64)) -> Self {
        Self::new(u, v, value)
    }
}

impl From<&(usize, usize, f64)> for ProgramEdge {
    fn from(&(u, v, value): &(usize, usize, f64)) -> Self {
        Self::new(u, v, value)
    }
}

impl From<&ProgramEdge> for ProgramEdge {
    fn from(edge: &ProgramEdge) -> Self {
        *edge
    }
}

/// A canonical sparse graph supplied to the program checker.
#[derive(Debug, Clone)]
pub struct ProgramGraph {
    vertex_count: usize,
    edges: Vec<ProgramEdge>,
    positions: BTreeMap<(usize, usize), usize>,
}

impl ProgramGraph {
    /// Build a graph from any iterable of [`ProgramEdge`] values or triples.
    ///
    /// Edges are sorted by endpoint. Repeated pairs are accepted only when
    /// their values agree bit for bit.
    pub fn new<I, E>(vertex_count: usize, edges: I) -> Result<Self, ProofError>
    where
        I: IntoIterator<Item = E>,
        E: Into<ProgramEdge>,
    {
        let mut by_edge = BTreeMap::new();
        for edge in edges {
            let edge = normalize_edge(edge.into(), vertex_count)?;
            insert_edge(&mut by_edge, edge)?;
        }
        let edges: Vec<_> = by_edge.into_values().collect();
        let positions = edges
            .iter()
            .enumerate()
            .map(|(position, edge)| ((edge.u, edge.v), position))
            .collect();
        Ok(Self {
            vertex_count,
            edges,
            positions,
        })
    }

    /// Build a graph from `(u, v, value)` triplets.
    pub fn from_triplets(
        vertex_count: usize,
        triplets: &[(usize, usize, f64)],
    ) -> Result<Self, ProofError> {
        Self::new(vertex_count, triplets)
    }

    /// Build a graph from any iterable of [`ProgramEdge`] values or triples.
    pub fn from_edges<I, E>(vertex_count: usize, edges: I) -> Result<Self, ProofError>
    where
        I: IntoIterator<Item = E>,
        E: Into<ProgramEdge>,
    {
        Self::new(vertex_count, edges)
    }

    /// Parse a sparse graph used by the `holos-check` command.
    ///
    /// The first non-comment line may contain the vertex count. Each remaining
    /// line contains `u v value`, separated by whitespace or commas. When the
    /// count is omitted, it is one more than the largest endpoint. A
    /// `vertex_count=N` comment can preserve isolated trailing vertices.
    pub fn parse_text(bytes: &[u8]) -> Result<Self, ProofError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| ProofError::new("source graph is not UTF-8 text"))?;
        let mut vertex_count = None;
        let mut edges = Vec::new();
        let mut inferred_vertex_count = 0usize;
        let mut first_body = true;
        for (line, text) in text.lines().enumerate() {
            let line = line + 1;
            if let Some(edge) = parse_line(text, line, &mut first_body, &mut vertex_count)? {
                inferred_vertex_count = inferred_vertex_count.max(edge.u.saturating_add(1));
                inferred_vertex_count = inferred_vertex_count.max(edge.v.saturating_add(1));
                edges.push(edge);
            }
        }
        let vertex_count = vertex_count.unwrap_or(inferred_vertex_count);
        if vertex_count < inferred_vertex_count {
            return Err(ProofError::new(format!(
                "source graph declares {vertex_count} vertices but an edge reaches {inferred_vertex_count}"
            )));
        }
        Self::new(vertex_count, edges)
    }

    /// Number of source vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Number of source vertices.
    pub fn len(&self) -> usize {
        self.vertex_count
    }

    /// Return true when the source graph has no vertices.
    pub fn is_empty(&self) -> bool {
        self.vertex_count == 0
    }

    /// Number of listed source edges.
    pub fn num_edges(&self) -> usize {
        self.edges.len()
    }

    /// Canonical source edges in ascending endpoint order.
    pub fn edges(&self) -> &[ProgramEdge] {
        &self.edges
    }

    /// Return the listed value, or positive infinity for an absent pair.
    pub fn get(&self, u: usize, v: usize) -> f64 {
        if u == v {
            return 0.0;
        }
        let edge = if u < v { (u, v) } else { (v, u) };
        self.positions
            .get(&edge)
            .map_or(f64::INFINITY, |&position| self.edges[position].value)
    }

    pub(crate) fn proof_graph(&self) -> Graph {
        let edges: Vec<_> = self
            .edges
            .iter()
            .map(|edge| ProofEdge {
                u: edge.u,
                v: edge.v,
                value: edge.value,
            })
            .collect();
        Graph::new(self.vertex_count, &edges).expect("validated program graph")
    }
}

fn normalize_edge(mut edge: ProgramEdge, vertex_count: usize) -> Result<ProgramEdge, ProofError> {
    if edge.u > edge.v {
        std::mem::swap(&mut edge.u, &mut edge.v);
    }
    if edge.u == edge.v {
        return Err(ProofError::new(format!(
            "source graph contains self-edge ({}, {})",
            edge.u, edge.v
        )));
    }
    if edge.v >= vertex_count {
        return Err(ProofError::new(format!(
            "source edge ({}, {}) is outside {vertex_count} vertices",
            edge.u, edge.v
        )));
    }
    if !edge.value.is_finite() || edge.value < 0.0 {
        return Err(ProofError::new(format!(
            "source edge ({}, {}) has invalid value {}",
            edge.u, edge.v, edge.value
        )));
    }
    if edge.value == 0.0 {
        edge.value = 0.0;
    }
    Ok(edge)
}

fn insert_edge(
    by_edge: &mut BTreeMap<(usize, usize), ProgramEdge>,
    edge: ProgramEdge,
) -> Result<(), ProofError> {
    match by_edge.insert((edge.u, edge.v), edge) {
        Some(previous) if previous.value.to_bits() != edge.value.to_bits() => {
            Err(ProofError::new(format!(
                "source graph has conflicting values for edge ({}, {})",
                edge.u, edge.v
            )))
        }
        Some(previous) => {
            by_edge.insert((edge.u, edge.v), previous);
            Ok(())
        }
        None => Ok(()),
    }
}

fn parse_line(
    text: &str,
    line: usize,
    first_body: &mut bool,
    vertex_count: &mut Option<usize>,
) -> Result<Option<ProgramEdge>, ProofError> {
    let (body, comment) = text.split_once('#').unwrap_or((text, ""));
    if vertex_count.is_none() {
        *vertex_count = comment_vertex_count(comment);
    }
    let body = body.trim();
    if body.is_empty() {
        return Ok(None);
    }
    let fields: Vec<_> = body
        .split(|character: char| character == ',' || character.is_ascii_whitespace())
        .filter(|field| !field.is_empty())
        .collect();
    if *first_body && fields.len() == 1 {
        *vertex_count = Some(fields[0].parse::<usize>().map_err(|_| {
            ProofError::new(format!("source graph line {line} has invalid vertex count"))
        })?);
        *first_body = false;
        return Ok(None);
    }
    *first_body = false;
    Ok(Some(parse_edge_fields(&fields, line)?))
}

fn parse_edge_fields(fields: &[&str], line: usize) -> Result<ProgramEdge, ProofError> {
    if fields.len() != 3 {
        return Err(ProofError::new(format!(
            "source graph line {line} must contain u, v, and value"
        )));
    }
    let u = fields[0]
        .parse::<usize>()
        .map_err(|_| ProofError::new(format!("source graph line {line} has invalid u")))?;
    let v = fields[1]
        .parse::<usize>()
        .map_err(|_| ProofError::new(format!("source graph line {line} has invalid v")))?;
    let value = fields[2]
        .parse::<f64>()
        .map_err(|_| ProofError::new(format!("source graph line {line} has invalid value")))?;
    Ok(ProgramEdge::new(u, v, value))
}

fn comment_vertex_count(comment: &str) -> Option<usize> {
    comment
        .split(|character: char| character.is_ascii_whitespace() || ",;".contains(character))
        .find_map(|field| {
            let (key, value) = field.split_once('=')?;
            if !matches!(key, "vertex_count" | "vertices" | "n") {
                return None;
            }
            value
                .trim_matches(|character: char| ",;()[]".contains(character))
                .parse()
                .ok()
        })
}
