use super::preparation::AdjEntry;

/// Reused buffers for the edge test and the dirty marking.
#[derive(Default)]
pub(super) struct Scratch {
    /// C as `(vertex, b)`, ascending vertex order.
    pub(super) cands: Vec<(usize, f64)>,
    /// `(b, position)` pairs sorted by `b`, then position: the entrant
    /// runs, with the sort key held directly in the element.
    pub(super) by_b: Vec<(f64, u32)>,
    /// `f(apex, cands[q])` for the current apex; 0 at the apex itself.
    pub(super) apex_row: Vec<f64>,
    /// The affected-vertex set S during dirty marking.
    pub(super) marks: Vec<usize>,
}

/// Above this |S|, fine-grained dirty marking costs more than a plain
/// retest of every live edge in the next pass, so the caller falls back.
const MARK_LIMIT: usize = 64;

/// `f(w, x)` for one pair by binary search in `w`'s adjacency list.
fn pair_value(list: &[AdjEntry], x: usize) -> f64 {
    match list.binary_search_by(|probe| probe.0.cmp(&x)) {
        Ok(pos) => list[pos].1,
        Err(_) => f64::INFINITY,
    }
}

/// True when the candidate at `pos` dominates at level `t`: `f(w, x) <= t`
/// for every level member `x`. `members` holds the candidate positions with
/// `b <= t`. A small member set probes by binary search; a large one runs a
/// single forward scan of `adj[w]` against the vertex-sorted candidate
/// list. Both leave on the first violation.
fn dominates(
    adj: &[Vec<AdjEntry>],
    cands: &[(usize, f64)],
    members: &[(f64, u32)],
    pos: usize,
    t: f64,
) -> bool {
    let list = &adj[cands[pos].0];
    if members.len() * 16 < list.len() {
        return members.iter().all(|&(_, q)| {
            let q = q as usize;
            q == pos || pair_value(list, cands[q].0) <= t
        });
    }
    let mut i = 0;
    for (q, &(x, b)) in cands.iter().enumerate() {
        if q == pos || b > t {
            continue;
        }
        while i < list.len() && list[i].0 < x {
            i += 1;
        }
        if i >= list.len() || list[i].0 != x || list[i].1 > t {
            return false;
        }
    }
    true
}

/// Fill `row` with `f(w, x)` over the candidate list, where `w` is the
/// candidate at `pos`: +inf for non-neighbors, 0 at `pos` itself. A short
/// candidate list probes by binary search; a long one merges.
fn fill_row(adj: &[Vec<AdjEntry>], cands: &[(usize, f64)], pos: usize, row: &mut Vec<f64>) {
    row.clear();
    row.resize(cands.len(), f64::INFINITY);
    let list = &adj[cands[pos].0];
    if cands.len() * 16 < list.len() {
        for (q, &(x, _)) in cands.iter().enumerate() {
            row[q] = pair_value(list, x);
        }
    } else {
        let (mut i, mut q) = (0, 0);
        while i < list.len() && q < cands.len() {
            let (x, d, _) = list[i];
            match x.cmp(&cands[q].0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => q += 1,
                std::cmp::Ordering::Equal => {
                    row[q] = d;
                    i += 1;
                    q += 1;
                }
            }
        }
    }
    row[pos] = 0.0;
}

/// Test one live edge `{u, v}` with value `a` against the current graph.
/// Return the witness segments when the edge is removable, `None` when it
/// is not. `s.cands` is left holding `C` so the caller can read `|C|`.
///
/// The sweep walks the entrant runs of the b-sorted candidate order. A kept
/// apex is rechecked against the entrants only: earlier members already
/// satisfied `f(apex, x) <= t'` at a smaller `t'`. A rescan streams one
/// candidate row at a time in increasing vertex order and keeps the first
/// dominating row as the new apex row.
pub(super) fn test_edge(
    adj: &[Vec<AdjEntry>],
    u: usize,
    v: usize,
    a: f64,
    terminal: f64,
    s: &mut Scratch,
) -> Option<Vec<(f64, usize)>> {
    collect_candidates(adj, u, v, a, terminal, &mut s.cands);
    if s.cands.is_empty() {
        return None;
    }
    order_candidates(&s.cands, &mut s.by_b);
    if s.by_b[0].0 > a {
        return None;
    }
    witness_segments(adj, &s.cands, &s.by_b, &mut s.apex_row)
}

fn collect_candidates(
    adj: &[Vec<AdjEntry>],
    u: usize,
    v: usize,
    edge_value: f64,
    terminal: f64,
    candidates: &mut Vec<(usize, f64)>,
) {
    candidates.clear();
    let (lu, lv) = (&adj[u], &adj[v]);
    let (mut i, mut j) = (0, 0);
    while i < lu.len() && j < lv.len() {
        let (x, du, _) = lu[i];
        let (y, dv, _) = lv[j];
        match x.cmp(&y) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                if du.is_finite() && dv.is_finite() {
                    let b = edge_value.max(du).max(dv);
                    if b <= terminal {
                        candidates.push((x, b));
                    }
                }
                i += 1;
                j += 1;
            }
        }
    }
}

fn order_candidates(candidates: &[(usize, f64)], by_birth: &mut Vec<(f64, u32)>) {
    by_birth.clear();
    by_birth.extend(
        candidates
            .iter()
            .enumerate()
            .map(|(position, &(_, birth))| (birth, position as u32)),
    );
    by_birth.sort_unstable_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
}

fn witness_segments(
    adj: &[Vec<AdjEntry>],
    candidates: &[(usize, f64)],
    by_birth: &[(f64, u32)],
    apex_row: &mut Vec<f64>,
) -> Option<Vec<(f64, usize)>> {
    let mut segments: Vec<(f64, usize)> = Vec::new();
    let mut run = 0usize;
    let mut have_apex = false;
    while run < candidates.len() {
        let t = by_birth[run].0;
        let mut run_end = run;
        while run_end < candidates.len() && by_birth[run_end].0 == t {
            run_end += 1;
        }
        let kept = have_apex
            && by_birth[run..run_end]
                .iter()
                .all(|&(_, position)| apex_row[position as usize] <= t);
        if !kept {
            let position = first_dominating_candidate(adj, candidates, &by_birth[..run_end], t)?;
            fill_row(adj, candidates, position, apex_row);
            segments.push((t, candidates[position].0));
            have_apex = true;
        }
        run = run_end;
    }
    Some(segments)
}

fn first_dominating_candidate(
    adj: &[Vec<AdjEntry>],
    candidates: &[(usize, f64)],
    members: &[(f64, u32)],
    level: f64,
) -> Option<usize> {
    (0..candidates.len()).find(|&position| {
        candidates[position].1 <= level && dominates(adj, candidates, members, position, level)
    })
}

/// Run `mark` on the schedule index of every live edge of the subgraph
/// induced by S = N[u] intersect N[v] (closed neighborhoods) in the
/// current graph. With `limit` set, the walk bails and returns false
/// before it visits anything when the neighborhood is larger than the
/// limit allows; `s.marks` is unusable after a bail. Without a limit the
/// walk is exact, which conflict blocking in the rounds schedule needs.
pub(super) fn for_each_induced_edge(
    adj: &[Vec<AdjEntry>],
    u: usize,
    v: usize,
    s: &mut Scratch,
    limit: Option<usize>,
    mut mark: impl FnMut(usize),
) -> bool {
    let (lu, lv) = (&adj[u], &adj[v]);
    if limit.is_some_and(|limit| lu.len().min(lv.len()) > 2 * limit) {
        return false;
    }
    collect_closed_common(lu, lv, u, v, &mut s.marks);
    if limit.is_some_and(|limit| s.marks.len() > limit) {
        return false;
    }
    for (position, &vertex) in s.marks.iter().enumerate() {
        mark_induced_edges(adj, &s.marks, position, vertex, &mut mark);
    }
    true
}

fn collect_closed_common(
    left: &[AdjEntry],
    right: &[AdjEntry],
    u: usize,
    v: usize,
    vertices: &mut Vec<usize>,
) {
    vertices.clear();
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        let (x, du, _) = left[i];
        let (y, dv, _) = right[j];
        match x.cmp(&y) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                if du.is_finite() && dv.is_finite() {
                    vertices.push(x);
                }
                i += 1;
                j += 1;
            }
        }
    }
    vertices.push(u);
    vertices.push(v);
    vertices.sort_unstable();
}

fn mark_induced_edges(
    adj: &[Vec<AdjEntry>],
    vertices: &[usize],
    position: usize,
    vertex: usize,
    mark: &mut impl FnMut(usize),
) {
    let list = &adj[vertex];
    if vertices.len() * 16 < list.len() {
        mark_induced_edges_by_probe(list, &vertices[position + 1..], mark);
    } else {
        mark_induced_edges_by_merge(list, vertices, vertex, mark);
    }
}

fn mark_induced_edges_by_probe(
    list: &[AdjEntry],
    vertices: &[usize],
    mark: &mut impl FnMut(usize),
) {
    for &other in vertices {
        if let Ok(position) = list.binary_search_by(|probe| probe.0.cmp(&other)) {
            let (_, distance, index) = list[position];
            if distance.is_finite() {
                mark(index);
            }
        }
    }
}

fn mark_induced_edges_by_merge(
    list: &[AdjEntry],
    vertices: &[usize],
    vertex: usize,
    mark: &mut impl FnMut(usize),
) {
    let (mut i, mut q) = (0, 0);
    while i < list.len() && q < vertices.len() {
        let (neighbor, distance, index) = list[i];
        match neighbor.cmp(&vertices[q]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => q += 1,
            std::cmp::Ordering::Equal => {
                if neighbor > vertex && distance.is_finite() {
                    mark(index);
                }
                i += 1;
                q += 1;
            }
        }
    }
}

/// Mark every live edge whose test could change after `{u, v}` goes away.
/// A verdict depends only on edges inside `{u, v} union C`, and every such
/// vertex lies in both closed neighborhoods of the removed pair, so marking
/// all live edges with both endpoints in S = N[u] intersect N[v] is a
/// conservative cover. Runs before the tombstone so S still sees the edge.
/// Returns false without marking when S exceeds [`MARK_LIMIT`]; the caller
/// then retests every live edge next pass, which is sound (a superset of
/// the dirty set) and cheaper on dense neighborhoods.
pub(super) fn mark_dirty(
    adj: &[Vec<AdjEntry>],
    dirty: &mut [bool],
    u: usize,
    v: usize,
    s: &mut Scratch,
) -> bool {
    for_each_induced_edge(adj, u, v, s, Some(MARK_LIMIT), |idx| dirty[idx] = true)
}
