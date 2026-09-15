use std::collections::BTreeMap;

use crate::proof::Graph;
use crate::{ProofError, inverse_mod, is_prime};

use super::claim::CocycleTermClaim;
use super::class_verify::{basis_class_id, canonical_basis, group_id};
use super::model::ProgramProofLimits;
use super::reduction::Complex;

type Sparse = BTreeMap<usize, u64>;

/// One normalized finite-field coefficient on an active edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReplayCocycleTerm {
    pub(crate) u: usize,
    pub(crate) v: usize,
    pub(crate) coefficient: u32,
}

/// One pair-linked cohomology seed before equal-interval row reduction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawPairSeed {
    pub(crate) pair: ReplayPair,
    pub(crate) scale: f64,
    /// The normalized edge cocycle obtained from this seed at `scale`.
    pub(crate) terms: Vec<ReplayCocycleTerm>,
}

/// One H1 interval and its checked creator and destroyer simplices.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplayPair {
    pub(crate) interval: crate::proof::ProofBar,
    pub(crate) birth: [usize; 2],
    pub(crate) death: Option<[usize; 3]>,
}

/// One deterministic canonical basis for an equal-interval group.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplayGroup {
    pub(crate) interval: crate::proof::ProofBar,
    pub(crate) scale: f64,
    pub(crate) pairs: Vec<ReplayPair>,
    pub(crate) basis: Vec<Vec<ReplayCocycleTerm>>,
    pub(crate) id: [u8; 32],
    pub(crate) class_ids: Vec<[u8; 32]>,
}

/// Complete reverse-cohomology replay output for one filtered graph.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplayResult {
    pub(crate) seeds: Vec<RawPairSeed>,
    pub(crate) groups: Vec<ReplayGroup>,
}

/// Replay the fixed scalar H1 class profile and construct canonical groups.
///
/// The replay uses all filtered edges and triangles. It first reproduces the
/// producer's dimension-zero cycle-edge order, then reduces the explicit
/// coboundary matrix in reverse edge order. Equal-interval seeds are passed to
/// the checker canonicalization code after their representative scale is
/// applied.
pub(crate) fn replay_h1(
    graph: &Graph,
    threshold: Option<f64>,
    modulus: u32,
    limits: ProgramProofLimits,
) -> Result<ReplayResult, ProofError> {
    check_modulus(modulus)?;
    check_threshold(threshold)?;
    if graph.vertex_count > limits.max_vertices {
        return Err(ProofError::new("replay vertex count exceeds the limit"));
    }
    // Each triangle contributes three entries to the edge-to-triangle incidence.
    let triangle_limit = limits.max_triangles.min(limits.max_terms / 3);
    let complex = Complex::build(graph, threshold, limits.max_edges, triangle_limit)?;
    let terminal = terminal_level(graph, threshold);
    let incidence = cochain_incidence(&complex, modulus, limits.max_terms)?;
    let cycle_edges = cycle_edges(&complex);
    let mut replay = CohomologyReplay::new(&complex, incidence, modulus, terminal, limits);
    for &edge in cycle_edges.iter().rev() {
        replay.reduce_edge(edge)?;
    }
    replay.finish(graph)
}

struct CohomologyReplay<'a> {
    complex: &'a Complex,
    incidence: Vec<Vec<(usize, u64)>>,
    modulus: u64,
    terminal: f64,
    limits: ProgramProofLimits,
    pivots: BTreeMap<usize, ReducedColumn>,
    seeds: Vec<RawPairSeed>,
    stored_terms: usize,
    cocycle_terms: usize,
}

#[derive(Clone)]
struct ReducedColumn {
    row: Sparse,
    transform: Sparse,
}

struct CanonicalGroup {
    basis: Vec<Vec<ReplayCocycleTerm>>,
    class_ids: Vec<[u8; 32]>,
    id: [u8; 32],
}

impl<'a> CohomologyReplay<'a> {
    fn new(
        complex: &'a Complex,
        incidence: Vec<Vec<(usize, u64)>>,
        modulus: u32,
        terminal: f64,
        limits: ProgramProofLimits,
    ) -> Self {
        Self {
            complex,
            incidence,
            modulus: modulus as u64,
            terminal,
            limits,
            pivots: BTreeMap::new(),
            seeds: Vec::new(),
            stored_terms: 0,
            cocycle_terms: 0,
        }
    }

    fn reduce_edge(&mut self, edge: usize) -> Result<(), ProofError> {
        let (row, transform) = self.initial_column(edge)?;
        let (row, transform) = self.reduce_column(row, transform)?;
        self.finish_edge(edge, row, transform)
    }

    fn initial_column(&self, edge: usize) -> Result<(Sparse, Sparse), ProofError> {
        let row = self
            .incidence
            .get(edge)
            .ok_or_else(|| ProofError::new("replay edge incidence is outside the complex"))?
            .iter()
            .copied()
            .collect::<Sparse>();
        let mut transform = Sparse::new();
        self.check_working_terms(row.len(), 1)?;
        transform.insert(edge, 1);
        Ok((row, transform))
    }

    fn reduce_column(
        &self,
        mut row: Sparse,
        mut transform: Sparse,
    ) -> Result<(Sparse, Sparse), ProofError> {
        while let Some((&pivot, &coefficient)) = row.first_key_value() {
            let Some(reducer) = self.pivots.get(&pivot) else {
                break;
            };
            let factor = (self.modulus
                - coefficient * inverse_mod(reducer.row[&pivot], self.modulus) % self.modulus)
                % self.modulus;
            add_scaled(
                &mut row,
                &reducer.row,
                factor,
                self.modulus,
                transform.len(),
                self.limits.max_terms,
            )?;
            add_scaled(
                &mut transform,
                &reducer.transform,
                factor,
                self.modulus,
                row.len(),
                self.limits.max_terms,
            )?;
            self.check_working_terms(row.len(), transform.len())?;
        }
        Ok((row, transform))
    }

    fn finish_edge(
        &mut self,
        edge: usize,
        row: Sparse,
        transform: Sparse,
    ) -> Result<(), ProofError> {
        match row.first_key_value() {
            Some((&pivot, _)) => self.finish_paired_edge(edge, pivot, row, transform),
            None => self.emit_essential(edge, transform),
        }
    }

    fn finish_paired_edge(
        &mut self,
        edge: usize,
        pivot: usize,
        row: Sparse,
        transform: Sparse,
    ) -> Result<(), ProofError> {
        let birth = self.complex.edges[edge];
        let death = self.complex.triangles[pivot];
        let pair = (death.value > birth.value).then_some(ReplayPair {
            interval: crate::proof::ProofBar {
                dimension: 1,
                birth: birth.value,
                death: death.value,
            },
            birth: birth.vertices,
            death: Some(death.vertices),
        });
        self.insert_reducer(pivot, row, transform)?;
        if let Some(pair) = pair {
            let scale = previous_float(death.value);
            let transform = self
                .pivots
                .get(&pivot)
                .expect("replay pivot was inserted")
                .transform
                .clone();
            self.emit_seed(pair, scale, transform)?;
        }
        Ok(())
    }

    fn emit_essential(&mut self, edge: usize, transform: Sparse) -> Result<(), ProofError> {
        let birth = self.complex.edges[edge];
        let pair = ReplayPair {
            interval: crate::proof::ProofBar {
                dimension: 1,
                birth: birth.value,
                death: f64::INFINITY,
            },
            birth: birth.vertices,
            death: None,
        };
        let scale = self.terminal;
        self.emit_seed(pair, scale, transform)
    }

    fn insert_reducer(
        &mut self,
        pivot: usize,
        row: Sparse,
        transform: Sparse,
    ) -> Result<(), ProofError> {
        if self.pivots.contains_key(&pivot) {
            return Err(ProofError::new(
                "replay produced a duplicate coboundary pivot",
            ));
        }
        let added = row
            .len()
            .checked_add(transform.len())
            .ok_or_else(|| ProofError::new("replay term count overflows usize"))?;
        self.stored_terms = self
            .stored_terms
            .checked_add(added)
            .ok_or_else(|| ProofError::new("replay term count overflows usize"))?;
        if self.stored_terms > self.limits.max_terms {
            return Err(ProofError::new(
                "replay change-of-basis terms exceed the limit",
            ));
        }
        self.pivots.insert(pivot, ReducedColumn { row, transform });
        Ok(())
    }

    fn emit_seed(
        &mut self,
        pair: ReplayPair,
        scale: f64,
        transform: Sparse,
    ) -> Result<(), ProofError> {
        if self.seeds.len() == self.limits.max_critical_pairs {
            return Err(ProofError::new(
                "replay critical-pair count exceeds the limit",
            ));
        }
        let terms = canonical_seed(self.complex, self.modulus, scale, &transform)?;
        check_cocycle(self.complex, scale, &terms, self.modulus)?;
        if terms.len() > self.limits.max_cocycle_terms {
            return Err(ProofError::new("replay cocycle exceeds the term limit"));
        }
        self.cocycle_terms = self
            .cocycle_terms
            .checked_add(terms.len())
            .ok_or_else(|| ProofError::new("replay cocycle term count overflows usize"))?;
        if self.cocycle_terms > self.limits.max_cocycle_terms {
            return Err(ProofError::new("replay cocycle terms exceed the limit"));
        }
        self.seeds.push(RawPairSeed { pair, scale, terms });
        Ok(())
    }

    fn check_working_terms(&self, row: usize, transform: usize) -> Result<(), ProofError> {
        let total = row
            .checked_add(transform)
            .ok_or_else(|| ProofError::new("replay working term count overflows usize"))?;
        if row > self.limits.max_terms
            || transform > self.limits.max_terms
            || total > self.limits.max_terms
        {
            return Err(ProofError::new(
                "replay working column exceeds the term limit",
            ));
        }
        Ok(())
    }

    fn finish(mut self, graph: &Graph) -> Result<ReplayResult, ProofError> {
        self.seeds.sort_by(seed_order);
        let groups = replay_groups(&self.seeds, graph, self.complex, self.modulus, self.limits)?;
        Ok(ReplayResult {
            seeds: self.seeds,
            groups,
        })
    }
}

fn replay_groups(
    seeds: &[RawPairSeed],
    graph: &Graph,
    complex: &Complex,
    modulus: u64,
    limits: ProgramProofLimits,
) -> Result<Vec<ReplayGroup>, ProofError> {
    let mut groups = Vec::new();
    let mut basis_count = 0;
    let mut basis_terms = 0;
    let mut start = 0;
    while start < seeds.len() {
        if groups.len() == limits.max_spaces {
            return Err(ProofError::new(
                "replay class-space count exceeds the limit",
            ));
        }
        let interval = seeds[start].pair.interval;
        let end = interval_end(seeds, start, interval);
        let group = replay_group(&seeds[start..end], graph, complex, modulus, limits)?;
        account_basis_count(&mut basis_count, group.basis.len(), limits.max_basis)?;
        account_basis_terms(&mut basis_terms, &group.basis, limits.max_cocycle_terms)?;
        groups.push(group);
        start = end;
    }
    Ok(groups)
}

fn account_basis_count(total: &mut usize, added: usize, limit: usize) -> Result<(), ProofError> {
    *total = total
        .checked_add(added)
        .ok_or_else(|| ProofError::new("replay basis count overflows usize"))?;
    if *total > limit {
        return Err(ProofError::new("replay basis count exceeds the limit"));
    }
    Ok(())
}

fn account_basis_terms(
    total: &mut usize,
    basis: &[Vec<ReplayCocycleTerm>],
    limit: usize,
) -> Result<(), ProofError> {
    let added = basis.iter().try_fold(0usize, |total, terms| {
        total
            .checked_add(terms.len())
            .ok_or_else(|| ProofError::new("replay basis term count overflows usize"))
    })?;
    *total = total
        .checked_add(added)
        .ok_or_else(|| ProofError::new("replay basis term count overflows usize"))?;
    if *total > limit {
        return Err(ProofError::new("replay basis terms exceed the limit"));
    }
    Ok(())
}

fn interval_end(seeds: &[RawPairSeed], start: usize, interval: crate::proof::ProofBar) -> usize {
    let mut end = start + 1;
    while end < seeds.len() && bar_bits_equal(seeds[end].pair.interval, interval) {
        end += 1;
    }
    end
}

fn replay_group(
    seeds: &[RawPairSeed],
    graph: &Graph,
    complex: &Complex,
    modulus: u64,
    limits: ProgramProofLimits,
) -> Result<ReplayGroup, ProofError> {
    if seeds.len() > limits.max_basis {
        return Err(ProofError::new(
            "replay class-space basis exceeds the limit",
        ));
    }
    let interval = seeds[0].pair.interval;
    let scale = seeds[0].scale;
    check_group_scales(seeds, scale)?;
    let mut pairs: Vec<_> = seeds.iter().map(|seed| seed.pair.clone()).collect();
    pairs.sort_by(pair_critical_order);
    let seed_terms = replay_seed_terms(seeds);
    let basis_claim = canonical_basis(graph, modulus as u32, scale, &seed_terms)?;
    if basis_claim.len() != seeds.len() {
        return Err(ProofError::new(
            "replay class-space rank differs from interval multiplicity",
        ));
    }
    let basis_terms = basis_term_count(&basis_claim)?;
    if basis_terms > limits.max_cocycle_terms {
        return Err(ProofError::new("replay basis terms exceed the limit"));
    }
    let canonical = replay_basis(basis_claim, interval, scale, modulus as u32, complex)?;
    Ok(ReplayGroup {
        interval,
        scale,
        pairs,
        basis: canonical.basis,
        id: canonical.id,
        class_ids: canonical.class_ids,
    })
}

fn check_group_scales(seeds: &[RawPairSeed], scale: f64) -> Result<(), ProofError> {
    if seeds
        .iter()
        .any(|seed| seed.scale.to_bits() != scale.to_bits())
    {
        return Err(ProofError::new(
            "replay equal-interval seeds have different representative scales",
        ));
    }
    Ok(())
}

fn replay_seed_terms(seeds: &[RawPairSeed]) -> Vec<Vec<CocycleTermClaim>> {
    seeds
        .iter()
        .map(|seed| {
            seed.terms
                .iter()
                .map(|term| CocycleTermClaim {
                    u: term.u,
                    v: term.v,
                    coefficient: term.coefficient,
                })
                .collect()
        })
        .collect()
}

fn basis_term_count(basis: &[Vec<CocycleTermClaim>]) -> Result<usize, ProofError> {
    basis.iter().try_fold(0usize, |total, terms| {
        total
            .checked_add(terms.len())
            .ok_or_else(|| ProofError::new("replay basis term count overflows usize"))
    })
}

fn replay_basis(
    basis_claim: Vec<Vec<CocycleTermClaim>>,
    interval: crate::proof::ProofBar,
    scale: f64,
    modulus: u32,
    complex: &Complex,
) -> Result<CanonicalGroup, ProofError> {
    let id = group_id(interval, modulus, scale, &basis_claim);
    let class_ids = basis_claim
        .iter()
        .enumerate()
        .map(|(index, terms)| basis_class_id(id, index, scale, terms))
        .collect();
    let basis: Vec<Vec<ReplayCocycleTerm>> = basis_claim
        .into_iter()
        .map(|terms| {
            terms
                .into_iter()
                .map(|term| ReplayCocycleTerm {
                    u: term.u,
                    v: term.v,
                    coefficient: term.coefficient,
                })
                .collect()
        })
        .collect();
    for terms in &basis {
        check_cocycle(complex, scale, terms.as_slice(), u64::from(modulus))?;
    }
    Ok(CanonicalGroup {
        basis,
        class_ids,
        id,
    })
}

fn cycle_edges(complex: &Complex) -> Vec<usize> {
    let mut sets = DisjointSet::new(complex.vertex_count);
    let mut cycles = Vec::new();
    for (position, edge) in complex.edges.iter().enumerate() {
        if sets.same(edge.vertices[0], edge.vertices[1]) {
            cycles.push(position);
        } else {
            sets.union(edge.vertices[0], edge.vertices[1]);
        }
    }
    cycles
}

fn cochain_incidence(
    complex: &Complex,
    modulus: u32,
    max_terms: usize,
) -> Result<Vec<Vec<(usize, u64)>>, ProofError> {
    let mut edge_degrees = vec![0usize; complex.edges.len()];
    let mut total_terms = 0usize;
    for simplex in &complex.triangles {
        let entries = triangle_incidence_edges(complex, *simplex, modulus)?;
        total_terms = total_terms
            .checked_add(entries.len())
            .ok_or_else(|| ProofError::new("replay incidence term count overflows usize"))?;
        if total_terms > max_terms {
            return Err(ProofError::new("replay incidence terms exceed the limit"));
        }
        for (edge, _) in entries {
            edge_degrees[edge] = edge_degrees[edge]
                .checked_add(1)
                .ok_or_else(|| ProofError::new("replay edge incidence count overflows usize"))?;
        }
    }
    let mut incidence: Vec<Vec<(usize, u64)>> =
        edge_degrees.into_iter().map(Vec::with_capacity).collect();
    for (triangle, simplex) in complex.triangles.iter().enumerate() {
        for (edge, coefficient) in triangle_incidence_edges(complex, *simplex, modulus)? {
            incidence[edge].push((triangle, coefficient));
        }
    }
    Ok(incidence)
}

fn triangle_incidence_edges(
    complex: &Complex,
    simplex: super::reduction::FilteredTriangle,
    modulus: u32,
) -> Result<[(usize, u64); 3], ProofError> {
    let [u, v, w] = simplex.vertices;
    let edges = [
        complex.edge_position((u, v)),
        complex.edge_position((u, w)),
        complex.edge_position((v, w)),
    ];
    let [Some(uv), Some(uw), Some(vw)] = edges else {
        return Err(ProofError::new("replay triangle references an absent edge"));
    };
    let modulus = u64::from(modulus);
    Ok([(uv, 1), (uw, modulus - 1), (vw, 1)])
}

fn canonical_seed(
    complex: &Complex,
    modulus: u64,
    scale: f64,
    transform: &Sparse,
) -> Result<Vec<ReplayCocycleTerm>, ProofError> {
    let mut coefficients = BTreeMap::<(usize, usize), u64>::new();
    for (&edge, &coefficient) in transform {
        let Some(simplex) = complex.edges.get(edge) else {
            return Err(ProofError::new(
                "replay transform references an absent edge",
            ));
        };
        if simplex.value <= scale {
            let [u, v] = simplex.vertices;
            add_coefficient(
                coefficients.entry((u, v)).or_default(),
                coefficient,
                modulus,
            );
        }
    }
    let mut terms: Vec<_> = coefficients
        .into_iter()
        .filter_map(|((u, v), coefficient)| {
            (coefficient != 0).then_some(ReplayCocycleTerm {
                u,
                v,
                coefficient: coefficient as u32,
            })
        })
        .collect();
    let Some(first) = terms.first() else {
        return Err(ProofError::new(
            "replay produced an empty H1 representative",
        ));
    };
    let inverse = inverse_mod(first.coefficient as u64, modulus);
    for term in &mut terms {
        term.coefficient = (term.coefficient as u64 * inverse % modulus) as u32;
    }
    Ok(terms)
}

fn check_cocycle(
    complex: &Complex,
    scale: f64,
    terms: &[ReplayCocycleTerm],
    modulus: u64,
) -> Result<(), ProofError> {
    let coefficients: BTreeMap<_, _> = terms
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient as u64))
        .collect();
    for simplex in complex
        .triangles
        .iter()
        .filter(|triangle| triangle.value <= scale)
    {
        let [u, v, w] = simplex.vertices;
        let uv = coefficient(&coefficients, (u, v));
        let uw = coefficient(&coefficients, (u, w));
        let vw = coefficient(&coefficients, (v, w));
        if (uv + vw + modulus - uw) % modulus != 0 {
            return Err(ProofError::new(format!(
                "replay representative is not closed on triangle ({u}, {v}, {w})"
            )));
        }
    }
    Ok(())
}

fn coefficient(coefficients: &BTreeMap<(usize, usize), u64>, edge: (usize, usize)) -> u64 {
    coefficients.get(&edge).copied().unwrap_or(0)
}

fn add_coefficient(target: &mut u64, value: u64, modulus: u64) {
    *target = (*target + value) % modulus;
}

fn add_scaled(
    target: &mut Sparse,
    source: &Sparse,
    factor: u64,
    modulus: u64,
    other_len: usize,
    limit: usize,
) -> Result<(), ProofError> {
    if factor == 0 {
        return Ok(());
    }
    for (&position, &coefficient) in source {
        let next = (target.get(&position).copied().unwrap_or(0) + factor * coefficient) % modulus;
        if next == 0 {
            target.remove(&position);
        } else {
            if !target.contains_key(&position) {
                let total = target
                    .len()
                    .checked_add(other_len)
                    .and_then(|length| length.checked_add(1))
                    .ok_or_else(|| ProofError::new("replay working term count overflows usize"))?;
                if total > limit {
                    return Err(ProofError::new(
                        "replay working column exceeds the term limit",
                    ));
                }
            }
            target.insert(position, next);
        }
    }
    Ok(())
}

fn seed_order(left: &RawPairSeed, right: &RawPairSeed) -> std::cmp::Ordering {
    left.pair
        .interval
        .birth
        .total_cmp(&right.pair.interval.birth)
        .then(
            left.pair
                .interval
                .death
                .total_cmp(&right.pair.interval.death),
        )
        .then_with(|| pair_critical_order(&left.pair, &right.pair))
}

fn pair_critical_order(left: &ReplayPair, right: &ReplayPair) -> std::cmp::Ordering {
    left.birth
        .cmp(&right.birth)
        .then_with(|| match (&left.death, &right.death) {
            (Some(a), Some(b)) => a.cmp(b),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

fn bar_bits_equal(left: crate::proof::ProofBar, right: crate::proof::ProofBar) -> bool {
    left.dimension == right.dimension
        && left.birth.to_bits() == right.birth.to_bits()
        && left.death.to_bits() == right.death.to_bits()
}

fn terminal_level(graph: &Graph, threshold: Option<f64>) -> f64 {
    let maximum = graph
        .edges
        .iter()
        .map(|edge| edge.value)
        .fold(0.0, f64::max);
    threshold.map_or(maximum, |value| value.min(maximum))
}

fn previous_float(value: f64) -> f64 {
    f64::from_bits(value.to_bits() - 1)
}

fn check_modulus(modulus: u32) -> Result<(), ProofError> {
    let value = modulus as u64;
    if value >= 32_768 || !is_prime(value) {
        return Err(ProofError::new(
            "replay modulus must be a prime below 32768",
        ));
    }
    Ok(())
}

fn check_threshold(threshold: Option<f64>) -> Result<(), ProofError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 || (value == 0.0 && value.is_sign_negative()) {
        return Err(ProofError::new("replay threshold must be non-negative"));
    }
    Ok(())
}

struct DisjointSet {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl DisjointSet {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, mut value: usize) -> usize {
        while self.parent[value] != value {
            let parent = self.parent[value];
            self.parent[value] = self.parent[parent];
            value = self.parent[value];
        }
        value
    }

    fn same(&mut self, left: usize, right: usize) -> bool {
        self.find(left) == self.find(right)
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left = self.find(left);
        let mut right = self.find(right);
        if left == right {
            return;
        }
        if self.rank[left] < self.rank[right] {
            std::mem::swap(&mut left, &mut right);
        }
        self.parent[right] = left;
        if self.rank[left] == self.rank[right] {
            self.rank[left] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReplayCocycleTerm, previous_float, replay_h1};
    use crate::program::model::ProgramProofLimits;
    use crate::proof::{Graph, ProofEdge};

    fn graph(vertex_count: usize, edges: &[(usize, usize, f64)]) -> Graph {
        let edges = edges
            .iter()
            .map(|&(u, v, value)| ProofEdge { u, v, value })
            .collect::<Vec<_>>();
        Graph::new(vertex_count, &edges).unwrap()
    }

    #[test]
    fn square_replay_keeps_one_threshold_relative_essential_seed() {
        let source = graph(4, &[(0, 1, 1.0), (0, 3, 1.0), (1, 2, 1.0), (2, 3, 1.0)]);
        let replay = replay_h1(&source, None, 2, ProgramProofLimits::default()).unwrap();
        assert_eq!(replay.seeds.len(), 1);
        assert_eq!(replay.groups.len(), 1);
        let seed = &replay.seeds[0];
        assert_eq!(seed.pair.birth, [0, 1]);
        assert_eq!(seed.pair.death, None);
        assert_eq!(seed.scale.to_bits(), 1.0f64.to_bits());
        assert_eq!(
            seed.terms,
            vec![ReplayCocycleTerm {
                u: 0,
                v: 1,
                coefficient: 1,
            }]
        );
        assert_eq!(
            replay.groups[0].basis,
            vec![vec![ReplayCocycleTerm {
                u: 2,
                v: 3,
                coefficient: 1,
            }]]
        );
    }

    #[test]
    fn diagonal_replay_links_the_square_seed_to_a_triangle_death() {
        let source = graph(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 2.0),
                (0, 3, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
            ],
        );
        let replay = replay_h1(&source, None, 2, ProgramProofLimits::default()).unwrap();
        assert_eq!(replay.seeds.len(), 1);
        let seed = &replay.seeds[0];
        assert_eq!(seed.pair.birth, [0, 1]);
        assert_eq!(seed.pair.death, Some([0, 1, 2]));
        assert_eq!(seed.pair.interval.birth.to_bits(), 1.0f64.to_bits());
        assert_eq!(seed.pair.interval.death.to_bits(), 2.0f64.to_bits());
        assert_eq!(seed.scale.to_bits(), previous_float(2.0).to_bits());
    }

    #[test]
    fn threshold_keeps_a_later_destroyer_out_of_an_essential_interval() {
        let source = graph(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 3.0),
                (0, 3, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
            ],
        );
        let replay = replay_h1(&source, Some(2.0), 2, ProgramProofLimits::default()).unwrap();
        assert_eq!(replay.seeds.len(), 1);
        assert_eq!(replay.seeds[0].pair.death, None);
        assert_eq!(replay.seeds[0].scale.to_bits(), 2.0f64.to_bits());
    }

    #[test]
    fn equal_interval_seeds_form_one_ranked_canonical_group() {
        let source = graph(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 1.0),
                (4, 5, 1.0),
                (5, 6, 1.0),
                (0, 6, 1.0),
            ],
        );
        let replay = replay_h1(&source, None, 3, ProgramProofLimits::default()).unwrap();
        assert_eq!(replay.seeds.len(), 2);
        assert_eq!(replay.groups.len(), 1);
        assert_eq!(replay.groups[0].pairs.len(), 2);
        assert_eq!(replay.groups[0].basis.len(), 2);
        assert_ne!(replay.groups[0].class_ids[0], replay.groups[0].class_ids[1]);
    }

    #[test]
    fn incidence_budget_limits_triangle_storage() {
        let source = graph(3, &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0)]);
        let limits = ProgramProofLimits {
            max_terms: 2,
            ..ProgramProofLimits::default()
        };
        assert!(replay_h1(&source, None, 2, limits).is_err());
    }
}
