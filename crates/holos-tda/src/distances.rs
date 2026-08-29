use std::ops::ControlFlow;

use rayon::prelude::*;

use crate::combinadic::{BinomialTable, CofacetIter};
use crate::simplex::Simplex;
use crate::{Error, Result};

/// Symmetric dissimilarity matrix.
/// No metric assumptions: entries need not satisfy the triangle inequality.
/// Entries must be non-negative and not NaN; +inf is legal and equivalent
/// to an absent edge.
///
/// The matrix has two storage forms. The compact form holds the condensed
/// lower triangle, `n(n-1)/2` entries, and is the form every constructor
/// builds. The full form holds both triangles row-major, `n * n` entries,
/// so that a cofacet diameter fold reads one contiguous row per simplex
/// vertex instead of one strided column.
#[derive(Debug, Clone)]
pub struct DistanceMatrix {
    n: usize,
    /// Row-major n by n in the full form, the condensed lower triangle in
    /// the compact one. Row `i` holds its entries below the diagonal first
    /// in both, which is what [`DistanceMatrix::lower_row`] returns.
    data: Vec<f64>,
    square: bool,
}

impl DistanceMatrix {
    /// Euclidean distances of a point cloud. Coordinates must be finite.
    pub fn from_points(points: &[Vec<f64>]) -> Result<Self> {
        let n = points.len();
        validate_points(points)?;
        let mut data = Vec::with_capacity(n.saturating_sub(1) * n / 2);
        for i in 1..n {
            for j in 0..i {
                data.push(euclidean(&points[i], &points[j]));
            }
        }
        Ok(Self {
            n,
            data,
            square: false,
        })
    }

    /// Build from the condensed lower triangle, row by row: d(1,0), d(2,0),
    /// d(2,1), d(3,0), and so on. An empty vector means one point (n = 1).
    /// Only [`DistanceMatrix::from_points`] can build an empty *space*
    /// (n = 0).
    pub fn from_condensed(mut condensed: Vec<f64>) -> Result<Self> {
        let m = condensed.len();
        let n = ((1.0 + 8.0 * m as f64).sqrt() as usize).div_ceil(2);
        if n * (n - 1) / 2 != m {
            return Err(Error::InvalidInput(format!(
                "condensed length {m} is not n(n-1)/2 for any n"
            )));
        }
        for (i, d) in condensed.iter_mut().enumerate() {
            if d.is_nan() {
                return Err(Error::InvalidDistance(format!(
                    "NaN at condensed index {i}"
                )));
            }
            if *d < 0.0 {
                return Err(Error::InvalidDistance(format!(
                    "negative entry {d} at condensed index {i}"
                )));
            }
            if *d == 0.0 {
                *d = 0.0;
            }
        }
        Ok(Self {
            n,
            data: condensed,
            square: false,
        })
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.n
    }

    /// True when there are no points.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Distance between points `i` and `j` (0 on the diagonal).
    #[inline]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        debug_assert!(i < self.n && j < self.n);
        if self.square {
            return self.data[i * self.n + j];
        }
        match i.cmp(&j) {
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => self.data[i * (i - 1) / 2 + j],
            std::cmp::Ordering::Less => self.data[j * (j - 1) / 2 + i],
        }
    }

    /// Row `i` up to the diagonal: the distances from `i` to every point
    /// below it, in index order. Both forms store that run contiguously.
    #[inline]
    fn lower_row(&self, i: usize) -> &[f64] {
        let start = if self.square {
            i * self.n
        } else {
            i * (i - 1) / 2
        };
        &self.data[start..start + i]
    }

    /// Count the pairs that enter the complex at `threshold`: finite and at
    /// or below it. One pass over the condensed triangle, no allocation.
    pub(crate) fn count_edges_at(&self, threshold: f64) -> usize {
        (1..self.n)
            .map(|i| {
                self.lower_row(i)
                    .iter()
                    .filter(|d| d.is_finite() && **d <= threshold)
                    .count()
            })
            .sum()
    }

    /// The thresholded graph: the pairs [`DistanceMatrix::count_edges_at`]
    /// counts, over the same vertex set. A vertex with no edge keeps its
    /// place and its essential H0 bar.
    ///
    /// One pass over the lower triangle counts the degrees, and a second
    /// pass files each kept pair under both of its endpoints. Row `i`
    /// reaches vertex `v` before any later row does, and it lists the
    /// neighbors below `v` in ascending order, so every list comes out
    /// sorted.
    pub(crate) fn to_sparse_at(&self, threshold: f64) -> Result<SparseDistanceMatrix> {
        let n = self.n;
        if n > u32::MAX as usize {
            return Err(Error::InvalidInput(format!(
                "sparse matrix holds at most {} points, got {n}",
                u32::MAX
            )));
        }
        let keep = |d: f64| d.is_finite() && d <= threshold;
        let mut degree = vec![0usize; n];
        for i in 1..n {
            for (j, &d) in self.lower_row(i).iter().enumerate() {
                if keep(d) {
                    degree[i] += 1;
                    degree[j] += 1;
                }
            }
        }
        let mut offsets = vec![0usize; n + 1];
        let mut total = 0usize;
        for (v, &deg) in degree.iter().enumerate() {
            offsets[v] = total;
            total += deg;
        }
        offsets[n] = total;

        let mut indices = vec![0u32; total];
        let mut values = vec![0.0f64; total];
        let mut cursor = offsets[..n].to_vec();
        let mut max_distance = 0.0f64;
        for i in 1..n {
            for (j, &d) in self.lower_row(i).iter().enumerate() {
                if !keep(d) {
                    continue;
                }
                max_distance = max_distance.max(d);
                indices[cursor[i]] = j as u32;
                values[cursor[i]] = d;
                cursor[i] += 1;
                indices[cursor[j]] = i as u32;
                values[cursor[j]] = d;
                cursor[j] += 1;
            }
        }
        Ok(SparseDistanceMatrix {
            n,
            offsets,
            indices,
            values,
            max_distance,
        })
    }

    /// Minimum over i of the maximum over j of d(i, j). Past that radius
    /// the complex is a cone and acquires no further homology. This is the
    /// default threshold.
    pub fn enclosing_radius(&self) -> f64 {
        if self.n < 2 {
            return 0.0;
        }
        // Each distance folds into both endpoints' running maxima, so one
        // pass over the lower triangle is enough. Row `i` holds its own
        // maximum in a local until the row ends: no earlier row writes
        // `row_max[i]`, because every column index it touches is below it.
        let mut row_max = vec![0.0f64; self.n];
        for i in 1..self.n {
            let mut max_i = 0.0f64;
            for (m, &d) in row_max[..i].iter_mut().zip(self.lower_row(i)) {
                max_i = max_i.max(d);
                *m = m.max(d);
            }
            row_max[i] = max_i;
        }
        row_max.into_iter().fold(f64::INFINITY, f64::min)
    }
}

/// Kernel used to build an exact threshold graph from points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PointCloudStrategy {
    /// Use the k-d tree for at most 12 coordinates and exhaustive blocks
    /// otherwise. An infinite threshold always uses exhaustive blocks.
    #[default]
    Auto,
    /// Use an exact k-d-tree radius join.
    KdTree,
    /// Test every unordered pair without storing a dense matrix.
    Exhaustive,
}

/// Parameters for exact threshold-graph construction from points.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PointCloudParams {
    /// Largest retained Euclidean distance. Must be non-negative and not
    /// NaN. Positive infinity retains every pair with a finite distance.
    pub threshold: f64,
    /// Worker threads. Zero and one both run serially.
    pub threads: usize,
    /// Construction kernel. Default [`PointCloudStrategy::Auto`].
    pub strategy: PointCloudStrategy,
}

impl PointCloudParams {
    /// Build parameters for `threshold` with one worker and automatic
    /// routing.
    pub fn new(threshold: f64) -> Self {
        Self {
            threshold,
            threads: 1,
            strategy: PointCloudStrategy::Auto,
        }
    }

    /// Set the worker count. Zero and one both run serially.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    /// Force a construction kernel.
    pub fn with_strategy(mut self, strategy: PointCloudStrategy) -> Self {
        self.strategy = strategy;
        self
    }
}

/// Counters from exact threshold-graph construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointCloudStats {
    /// Points in the input.
    pub points: usize,
    /// Coordinates per point.
    pub dimensions: usize,
    /// Unordered pairs whose Euclidean distance was evaluated.
    pub distance_evaluations: u64,
    /// Edges retained at the threshold.
    pub edges: usize,
    /// Kernel that ran after automatic routing.
    pub strategy: PointCloudStrategy,
}

/// Exact threshold graph from a point cloud, with construction counters.
#[derive(Debug, Clone)]
pub struct PointCloudGraph {
    matrix: SparseDistanceMatrix,
    stats: PointCloudStats,
}

impl PointCloudGraph {
    /// Build the exact Euclidean threshold graph of `points`.
    ///
    /// This function does not allocate a dense distance matrix. Both kernels
    /// use the same scaled Euclidean calculation as
    /// [`DistanceMatrix::from_points`]. The result is bit-identical at every
    /// worker count and under both forced strategies.
    pub fn build(points: &[Vec<f64>], params: PointCloudParams) -> Result<Self> {
        let dimensions = validate_points(points)?;
        validate_point_cloud_params(points.len(), params.threshold)?;
        let strategy = resolve_point_cloud_strategy(params, dimensions);
        let rows = build_threshold_rows(points, dimensions, params, strategy)?;
        let distance_evaluations = rows.iter().fold(0u64, |total, row| {
            total.saturating_add(row.evaluations as u64)
        });
        let lower: Vec<Vec<(usize, f64)>> = rows.into_iter().map(|row| row.edges).collect();
        let matrix = SparseDistanceMatrix::from_lower_rows(points.len(), &lower)?;
        let stats = PointCloudStats {
            points: points.len(),
            dimensions,
            distance_evaluations,
            edges: matrix.num_edges(),
            strategy,
        };
        Ok(Self { matrix, stats })
    }

    /// The exact sparse distance matrix.
    pub fn matrix(&self) -> &SparseDistanceMatrix {
        &self.matrix
    }

    /// Consume the result and return its sparse matrix.
    pub fn into_matrix(self) -> SparseDistanceMatrix {
        self.matrix
    }

    /// Construction counters.
    pub fn stats(&self) -> PointCloudStats {
        self.stats
    }
}

fn validate_point_cloud_params(points: usize, threshold: f64) -> Result<()> {
    if threshold.is_nan() || threshold < 0.0 {
        return Err(Error::InvalidInput(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    if points > u32::MAX as usize {
        return Err(Error::InvalidInput(format!(
            "sparse matrix holds at most {} points, got {points}",
            u32::MAX
        )));
    }
    Ok(())
}

fn resolve_point_cloud_strategy(params: PointCloudParams, dimensions: usize) -> PointCloudStrategy {
    match params.strategy {
        PointCloudStrategy::Auto if dimensions <= 12 && params.threshold.is_finite() => {
            PointCloudStrategy::KdTree
        }
        PointCloudStrategy::Auto => PointCloudStrategy::Exhaustive,
        strategy => strategy,
    }
}

fn build_threshold_rows(
    points: &[Vec<f64>],
    dimensions: usize,
    params: PointCloudParams,
    strategy: PointCloudStrategy,
) -> Result<Vec<ThresholdRow>> {
    match strategy {
        PointCloudStrategy::KdTree => {
            threshold_rows_kd(points, dimensions, params.threshold, params.threads)
        }
        PointCloudStrategy::Exhaustive => {
            threshold_rows_exhaustive(points, params.threshold, params.threads)
        }
        PointCloudStrategy::Auto => unreachable!("automatic strategy was resolved"),
    }
}

fn validate_points(points: &[Vec<f64>]) -> Result<usize> {
    let dimensions = points.first().map_or(0, Vec::len);
    if let Some(point) = points.iter().find(|point| point.len() != dimensions) {
        return Err(Error::InvalidInput(format!(
            "inconsistent point dimensions: {} vs {}",
            dimensions,
            point.len()
        )));
    }
    if points
        .iter()
        .flatten()
        .any(|coordinate| !coordinate.is_finite())
    {
        return Err(Error::InvalidInput("non-finite coordinate".into()));
    }
    Ok(dimensions)
}

#[derive(Default)]
struct ThresholdRow {
    edges: Vec<(usize, f64)>,
    evaluations: usize,
}

fn collect_threshold_rows(
    n: usize,
    threads: usize,
    row: impl Fn(usize) -> ThresholdRow + Sync + Send,
) -> Result<Vec<ThresholdRow>> {
    if threads <= 1 || n < 2 {
        return Ok((0..n).map(row).collect());
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| Error::Io(format!("thread pool: {error}")))?;
    Ok(pool.install(|| (0..n).into_par_iter().map(row).collect()))
}

fn threshold_rows_exhaustive(
    points: &[Vec<f64>],
    threshold: f64,
    threads: usize,
) -> Result<Vec<ThresholdRow>> {
    collect_threshold_rows(points.len(), threads, |i| {
        let mut row = ThresholdRow {
            edges: Vec::new(),
            evaluations: i,
        };
        for j in 0..i {
            let distance = euclidean(&points[i], &points[j]);
            if distance.is_finite() && distance <= threshold {
                row.edges.push((j, distance));
            }
        }
        row
    })
}

struct KdNode {
    point: usize,
    axis: usize,
    left: Option<usize>,
    right: Option<usize>,
}

struct KdTree {
    nodes: Vec<KdNode>,
    root: Option<usize>,
}

impl KdTree {
    fn build(points: &[Vec<f64>], dimensions: usize) -> Self {
        let mut order: Vec<usize> = (0..points.len()).collect();
        let mut nodes = Vec::with_capacity(points.len());
        let root = Self::build_node(points, dimensions, &mut order, &mut nodes);
        Self { nodes, root }
    }

    fn build_node(
        points: &[Vec<f64>],
        dimensions: usize,
        order: &mut [usize],
        nodes: &mut Vec<KdNode>,
    ) -> Option<usize> {
        if order.is_empty() {
            return None;
        }
        let axis = (0..dimensions)
            .max_by(|&a, &b| {
                coordinate_spread(points, order, a)
                    .total_cmp(&coordinate_spread(points, order, b))
                    .then_with(|| b.cmp(&a))
            })
            .unwrap_or(0);
        let middle = order.len() / 2;
        order.select_nth_unstable_by(middle, |&a, &b| {
            points[a][axis].total_cmp(&points[b][axis]).then(a.cmp(&b))
        });
        let (lower, rest) = order.split_at_mut(middle);
        let (pivot, upper) = rest.split_first_mut().expect("non-empty k-d-tree slice");
        let point = *pivot;
        let left = Self::build_node(points, dimensions, lower, nodes);
        let right = Self::build_node(points, dimensions, upper, nodes);
        let node = nodes.len();
        nodes.push(KdNode {
            point,
            axis,
            left,
            right,
        });
        Some(node)
    }

    fn query_lower(
        &self,
        node: Option<usize>,
        points: &[Vec<f64>],
        query: usize,
        threshold: f64,
        row: &mut ThresholdRow,
    ) {
        let Some(node) = node else {
            return;
        };
        let node = &self.nodes[node];
        let query_coordinate = points[query][node.axis];
        let pivot_coordinate = points[node.point][node.axis];
        let (near, far) = if query_coordinate.total_cmp(&pivot_coordinate).is_lt() {
            (node.left, node.right)
        } else {
            (node.right, node.left)
        };
        self.query_lower(near, points, query, threshold, row);
        if node.point < query {
            row.evaluations += 1;
            let distance = euclidean(&points[query], &points[node.point]);
            if distance.is_finite() && distance <= threshold {
                row.edges.push((node.point, distance));
            }
        }
        if (query_coordinate - pivot_coordinate).abs() <= threshold {
            self.query_lower(far, points, query, threshold, row);
        }
    }
}

fn coordinate_spread(points: &[Vec<f64>], order: &[usize], axis: usize) -> f64 {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for &point in order {
        low = low.min(points[point][axis]);
        high = high.max(points[point][axis]);
    }
    high - low
}

fn threshold_rows_kd(
    points: &[Vec<f64>],
    dimensions: usize,
    threshold: f64,
    threads: usize,
) -> Result<Vec<ThresholdRow>> {
    if dimensions == 0 {
        return threshold_rows_exhaustive(points, threshold, threads);
    }
    let tree = KdTree::build(points, dimensions);
    collect_threshold_rows(points.len(), threads, |i| {
        let mut row = ThresholdRow::default();
        tree.query_lower(tree.root, points, i, threshold, &mut row);
        row.edges.sort_unstable_by_key(|&(j, _)| j);
        row
    })
}

#[cfg(test)]
thread_local! {
    /// Conversions to the full form on this thread. Each test runs on its
    /// own thread, so the count belongs to one test and no other test can
    /// disturb it.
    pub(crate) static SQUARE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl DistanceMatrix {
    /// True when this matrix holds both triangles.
    #[cfg(test)]
    pub(crate) fn is_square(&self) -> bool {
        self.square
    }

    /// The same distances in the full row-major form. Every constructor
    /// builds the compact form, so this is the only way a run reaches the
    /// full one.
    pub(crate) fn to_square(&self) -> Self {
        #[cfg(test)]
        SQUARE_BUILDS.with(|c| c.set(c.get() + 1));
        let n = self.n;
        let mut data = vec![0.0f64; n * n];
        for i in 1..n {
            let row = self.lower_row(i);
            data[i * n..i * n + i].copy_from_slice(row);
            for (j, &d) in row.iter().enumerate() {
                data[j * n + i] = d;
            }
        }
        Self {
            n,
            data,
            square: true,
        }
    }
}

/// Sparse dissimilarities: only listed pairs have finite distance. An
/// unlisted pair is an absent edge (+inf). No metric assumptions, same
/// entry rules as [`DistanceMatrix`].
///
/// The neighbor lists live in one compressed block: an offset for each
/// vertex, then the neighbor vertices as `u32` and their distances in two
/// arrays of the same length. A list is sorted by neighbor vertex. The
/// cofacet merge then walks four bytes an entry and reads a distance only
/// where two lists meet.
#[derive(Debug, Clone)]
pub struct SparseDistanceMatrix {
    n: usize,
    /// Where each vertex's neighbor list starts, plus the total at the end.
    /// Length `n + 1`.
    offsets: Vec<usize>,
    /// Neighbor vertices, per vertex ascending. `from_triplets` rejects an
    /// `n` above `u32::MAX`, so a vertex fits in a `u32`.
    indices: Vec<u32>,
    /// The distance to the neighbor at the same position in `indices`.
    values: Vec<f64>,
    /// The largest stored distance, or 0 when no pair is stored.
    max_distance: f64,
}

impl SparseDistanceMatrix {
    fn from_lower_rows(n: usize, rows: &[Vec<(usize, f64)>]) -> Result<Self> {
        debug_assert_eq!(rows.len(), n);
        let mut degree = vec![0usize; n];
        let mut max_distance = 0.0f64;
        for (i, row) in rows.iter().enumerate() {
            debug_assert!(row.is_sorted_by_key(|&(j, _)| j));
            for &(j, distance) in row {
                debug_assert!(j < i);
                degree[i] += 1;
                degree[j] += 1;
                max_distance = max_distance.max(distance);
            }
        }
        let mut offsets = vec![0usize; n + 1];
        let mut total = 0usize;
        for (vertex, &count) in degree.iter().enumerate() {
            offsets[vertex] = total;
            total = total
                .checked_add(count)
                .ok_or_else(|| Error::InvalidInput("sparse edge storage overflows usize".into()))?;
        }
        offsets[n] = total;
        let mut indices = vec![0u32; total];
        let mut values = vec![0.0; total];
        let mut cursor = offsets[..n].to_vec();
        for (i, row) in rows.iter().enumerate() {
            for &(j, distance) in row {
                indices[cursor[i]] = j as u32;
                values[cursor[i]] = distance;
                cursor[i] += 1;
                indices[cursor[j]] = i as u32;
                values[cursor[j]] = distance;
                cursor[j] += 1;
            }
        }
        // Lower neighbors arrive first in ascending order. Higher neighbors
        // arrive later as their rows are visited, also in ascending order.
        debug_assert!((0..n).all(|v| {
            let start = offsets[v];
            let end = offsets[v + 1];
            indices[start..end].is_sorted()
        }));
        Ok(Self {
            n,
            offsets,
            indices,
            values,
            max_distance,
        })
    }

    /// Build from `(i, j, d)` triplets over `n` points. A repeated unordered
    /// pair must carry an identical distance. Entries must be finite and
    /// non-negative. Omit a pair to make it absent. `n` must be at or below
    /// `u32::MAX`.
    pub fn from_triplets(n: usize, triplets: &[(usize, usize, f64)]) -> Result<Self> {
        let degree = validate_triplets(n, triplets)?;
        let mut offsets = offsets_from_degrees(&degree);
        let (mut indices, mut values) = fill_neighbor_storage(triplets, &offsets);
        let write = compact_neighbor_storage(n, &degree, &mut offsets, &mut indices, &mut values)?;
        offsets[n] = write;
        indices.truncate(write);
        values.truncate(write);

        let max_distance = triplets.iter().fold(0.0f64, |m, &(_, _, d)| m.max(d));
        Ok(Self {
            n,
            offsets,
            indices,
            values,
            max_distance,
        })
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.n
    }

    /// True when there are no points.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Number of stored edges.
    pub fn num_edges(&self) -> usize {
        self.indices.len() / 2
    }

    /// Where vertex `v`'s neighbor list sits in `indices` and `values`.
    #[inline]
    fn span(&self, v: usize) -> (usize, usize) {
        (self.offsets[v], self.offsets[v + 1])
    }

    /// How many neighbors vertex `v` has.
    #[cfg(test)]
    #[inline]
    fn degree(&self, v: usize) -> usize {
        self.offsets[v + 1] - self.offsets[v]
    }

    /// Distance between `i` and `j`; +inf when the pair is not listed.
    #[inline]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        debug_assert!(i < self.n && j < self.n);
        if i == j {
            return 0.0;
        }
        let (start, end) = self.span(i);
        match self.indices[start..end].binary_search(&(j as u32)) {
            Ok(pos) => self.values[start + pos],
            Err(_) => f64::INFINITY,
        }
    }

    /// Visit every stored edge once, as `(u, v, value)` with `u < v`, in
    /// ascending `u` then `v` order.
    pub fn edges(&self) -> impl Iterator<Item = (usize, usize, f64)> + '_ {
        (0..self.n).flat_map(move |u| {
            let (start, end) = self.span(u);
            self.indices[start..end]
                .iter()
                .zip(&self.values[start..end])
                .filter(move |&(&v, _)| u < v as usize)
                .map(move |(&v, &d)| (u, v as usize, d))
        })
    }
}

fn validate_triplets(n: usize, triplets: &[(usize, usize, f64)]) -> Result<Vec<usize>> {
    if n > u32::MAX as usize {
        return Err(Error::InvalidInput(format!(
            "sparse matrix holds at most {} points, got {n}",
            u32::MAX
        )));
    }
    let mut degree = vec![0usize; n];
    for (index, &(i, j, distance)) in triplets.iter().enumerate() {
        validate_triplet(index, i, j, distance, n)?;
        degree[i] += 1;
        degree[j] += 1;
    }
    Ok(degree)
}

fn validate_triplet(index: usize, i: usize, j: usize, distance: f64, n: usize) -> Result<()> {
    if i >= n || j >= n {
        return Err(Error::InvalidInput(format!(
            "triplet {index}: vertex out of range ({i}, {j}) for n = {n}"
        )));
    }
    if i == j {
        return Err(Error::InvalidInput(format!(
            "triplet {index}: self-distance for vertex {i}"
        )));
    }
    if !distance.is_finite() || distance < 0.0 {
        return Err(Error::InvalidDistance(format!(
            "triplet {index}: distance must be finite and non-negative, got {distance}"
        )));
    }
    Ok(())
}

fn offsets_from_degrees(degree: &[usize]) -> Vec<usize> {
    let mut offsets = vec![0usize; degree.len() + 1];
    let mut total = 0usize;
    for (vertex, &value) in degree.iter().enumerate() {
        offsets[vertex] = total;
        total += value;
    }
    offsets[degree.len()] = total;
    offsets
}

fn fill_neighbor_storage(
    triplets: &[(usize, usize, f64)],
    offsets: &[usize],
) -> (Vec<u32>, Vec<f64>) {
    let total = offsets.last().copied().unwrap_or(0);
    let mut indices = vec![0u32; total];
    let mut values = vec![0.0f64; total];
    let mut cursor = offsets[..offsets.len() - 1].to_vec();
    for &(i, j, distance) in triplets {
        let distance = if distance == 0.0 { 0.0 } else { distance };
        indices[cursor[i]] = j as u32;
        values[cursor[i]] = distance;
        cursor[i] += 1;
        indices[cursor[j]] = i as u32;
        values[cursor[j]] = distance;
        cursor[j] += 1;
    }
    (indices, values)
}

fn compact_neighbor_storage(
    n: usize,
    degree: &[usize],
    offsets: &mut [usize],
    indices: &mut [u32],
    values: &mut [f64],
) -> Result<usize> {
    let widest = degree.iter().copied().max().unwrap_or(0);
    let mut list = Vec::<(u32, f64)>::with_capacity(widest);
    let mut write = 0usize;
    for vertex in 0..n {
        let (start, end) = (offsets[vertex], offsets[vertex + 1]);
        offsets[vertex] = write;
        if indices[start..end].is_sorted_by(|a, b| a < b) {
            copy_sorted_neighbors(start, end, write, indices, values);
            write += end - start;
        } else {
            write = sort_and_copy_neighbors(vertex, start, end, write, indices, values, &mut list)?;
        }
    }
    Ok(write)
}

fn copy_sorted_neighbors(
    start: usize,
    end: usize,
    write: usize,
    indices: &mut [u32],
    values: &mut [f64],
) {
    if start != write {
        indices.copy_within(start..end, write);
        values.copy_within(start..end, write);
    }
}

fn sort_and_copy_neighbors(
    vertex: usize,
    start: usize,
    end: usize,
    mut write: usize,
    indices: &mut [u32],
    values: &mut [f64],
    list: &mut Vec<(u32, f64)>,
) -> Result<usize> {
    list.clear();
    list.extend(
        indices[start..end]
            .iter()
            .zip(&values[start..end])
            .map(|(&neighbor, &distance)| (neighbor, distance)),
    );
    list.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.total_cmp(&right.1)));
    reject_conflicting_neighbors(vertex, list)?;
    list.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    for &(neighbor, distance) in list.iter() {
        indices[write] = neighbor;
        values[write] = distance;
        write += 1;
    }
    Ok(write)
}

fn reject_conflicting_neighbors(vertex: usize, neighbors: &[(u32, f64)]) -> Result<()> {
    for pair in neighbors.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].1 != pair[1].1 {
            return Err(Error::InvalidInput(format!(
                "conflicting distances for pair ({vertex}, {}): {} vs {}",
                pair[0].0, pair[0].1, pair[1].1
            )));
        }
    }
    Ok(())
}

/// A cofacet produced during enumeration: its combinadic index, the position
/// `k` of the added vertex in the cofacet (the coboundary sign exponent), the
/// added vertex itself, and the cofacet's filtration diameter. The vertex
/// lets a caller build the cofacet's vertex set from the simplex's own set
/// instead of unranking the cofacet.
///
/// Under `upper_only` every enumerator reports `k` as 0, not as `dim + 1`.
/// No caller reads the position there.
pub(crate) struct Cofacet {
    pub(crate) index: u64,
    pub(crate) k: usize,
    pub(crate) vertex: usize,
    pub(crate) diameter: f64,
}

/// Cursor slots a sparse cofacet enumeration keeps on the stack. A simplex
/// wider than this allocates its cursors once, on entry.
const INLINE_VERTS: usize = 16;

/// Untimed event counters for the sparse cofacet enumerator.
///
/// Only a test build has them. Elsewhere the `note_*` functions are empty.
/// The counts are thread-local because the test binary runs tests on
/// several threads at once.
#[cfg(test)]
mod counters {
    use std::cell::Cell;

    thread_local! {
        static CANDIDATES: Cell<u64> = const { Cell::new(0) };
        static CALLBACKS: Cell<u64> = const { Cell::new(0) };
        static BREAKS: Cell<u64> = const { Cell::new(0) };
        static SPILLS: Cell<u64> = const { Cell::new(0) };
        static VACUOUS: Cell<u64> = const { Cell::new(0) };
    }

    /// What one thread's enumerations did since the last [`reset`].
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(super) struct Events {
        /// Neighbor-list entries the enumerator read.
        pub(super) candidates: u64,
        /// Calls the enumerator made to the caller's closure.
        pub(super) callbacks: u64,
        /// Breaks the enumerator honored.
        pub(super) breaks: u64,
        /// Enumerations that allocated their cursors instead of keeping
        /// them on the stack.
        pub(super) spills: u64,
        /// Bounded walks whose bound rose to infinity, because no stored
        /// distance reached it.
        pub(super) vacuous: u64,
    }

    #[inline]
    pub(super) fn note_candidate() {
        CANDIDATES.with(|c| c.set(c.get() + 1));
    }

    #[inline]
    pub(super) fn note_callback() {
        CALLBACKS.with(|c| c.set(c.get() + 1));
    }

    #[inline]
    pub(super) fn note_break() {
        BREAKS.with(|c| c.set(c.get() + 1));
    }

    #[inline]
    pub(super) fn note_spill() {
        SPILLS.with(|c| c.set(c.get() + 1));
    }

    #[inline]
    pub(super) fn note_vacuous_bound() {
        VACUOUS.with(|c| c.set(c.get() + 1));
    }

    /// Zero this thread's counters.
    pub(super) fn reset() {
        CANDIDATES.with(|c| c.set(0));
        CALLBACKS.with(|c| c.set(0));
        BREAKS.with(|c| c.set(0));
        SPILLS.with(|c| c.set(0));
        VACUOUS.with(|c| c.set(0));
    }

    /// Read this thread's counters.
    pub(super) fn read() -> Events {
        Events {
            candidates: CANDIDATES.with(Cell::get),
            callbacks: CALLBACKS.with(Cell::get),
            breaks: BREAKS.with(Cell::get),
            spills: SPILLS.with(Cell::get),
            vacuous: VACUOUS.with(Cell::get),
        }
    }
}

#[cfg(not(test))]
mod counters {
    #[inline(always)]
    pub(super) fn note_candidate() {}
    #[inline(always)]
    pub(super) fn note_callback() {}
    #[inline(always)]
    pub(super) fn note_break() {}
    #[inline(always)]
    pub(super) fn note_spill() {}
    #[inline(always)]
    pub(super) fn note_vacuous_bound() {}
}

/// What the solver needs from a distance source. An absent pair reads as +inf.
pub(crate) trait Distances {
    fn len(&self) -> usize;
    fn get(&self, i: usize, j: usize) -> f64;
    /// Threshold to use when the caller gives none.
    fn default_threshold(&self) -> f64;
    /// The largest distance the source can report between distinct points,
    /// or +inf when it knows no such limit. A bound at or above it drops
    /// nothing, so [`Distances::for_each_cofacet_bounded`] raises it to
    /// infinity.
    fn max_distance(&self) -> f64 {
        f64::INFINITY
    }
    /// Visit every pair that could be an edge, as (i, j, d) with j < i.
    fn for_each_edge(&self, f: impl FnMut(usize, usize, f64));

    /// Enumerate cofacets of `simplex` (vertex set `verts`, ascending) in
    /// dimension `dim`, in strictly descending index order. `f` runs on each
    /// cofacet. With `upper_only`, restrict to cofacets whose added vertex
    /// exceeds every simplex vertex. Over all d-simplices, that restriction
    /// generates each (d+1)-simplex exactly once. Diameters may exceed the
    /// threshold or be infinite: the caller filters. `f` may short-circuit
    /// with `Break`.
    ///
    /// The default walks the full combinadic cofacet set. A sparse source
    /// visits only common neighbors.
    #[inline]
    fn for_each_cofacet<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        self.enumerate_cofacets::<false, T, _>(
            bt,
            simplex,
            verts,
            dim,
            upper_only,
            f64::INFINITY,
            f,
        )
    }

    /// [`Distances::for_each_cofacet`] restricted to the cofacets whose
    /// diameter is at or below `bound`. Those reach `f` in the same order and
    /// with the same bits as the unrestricted walk. `bound` must be at or
    /// above `simplex.diameter`.
    ///
    /// A cofacet diameter is the largest of the simplex diameter and the
    /// distances from the added vertex to the simplex vertices, so the fold
    /// can stop at the first distance above the bound.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn for_each_cofacet_bounded<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
        f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        debug_assert!(bound >= simplex.diameter);
        // A bound no distance reaches drops nothing, and infinity is such a
        // bound. Raising it there turns the test in the walk into one the
        // branch predictor always gets right, and it keeps one instance of
        // the walk at the call site.
        let mut bound = bound;
        if bound >= self.max_distance() {
            counters::note_vacuous_bound();
            bound = f64::INFINITY;
        }
        self.enumerate_cofacets::<true, T, _>(bt, simplex, verts, dim, upper_only, bound, f)
    }

    /// The one cofacet walk behind [`Distances::for_each_cofacet`] and
    /// [`Distances::for_each_cofacet_bounded`]. `BOUNDED` selects the bounded
    /// form, and only that form reads `bound`. A distance source overrides
    /// this method alone, so the two entry points cannot drift apart, and the
    /// unbounded one carries no bound test.
    #[allow(clippy::too_many_arguments)]
    fn enumerate_cofacets<const BOUNDED: bool, T, F>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
        mut f: F,
    ) -> Option<T>
    where
        F: FnMut(Cofacet) -> ControlFlow<T>,
    {
        let cofacet_diameter = |added: usize| {
            let mut d = simplex.diameter;
            for &v in verts {
                let x = self.get(added, v);
                if BOUNDED && x > bound {
                    return None;
                }
                d = d.max(x);
            }
            Some(d)
        };
        let mut iter = CofacetIter::new(bt, simplex.index, dim, self.len());
        if upper_only {
            while let Some((index, vertex)) = iter.next_upper() {
                let Some(diameter) = cofacet_diameter(vertex) else {
                    continue;
                };
                let cofacet = Cofacet {
                    index,
                    k: 0,
                    vertex,
                    diameter,
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        } else {
            while let Some((index, vertex, k)) = iter.next_all() {
                let Some(diameter) = cofacet_diameter(vertex) else {
                    continue;
                };
                let cofacet = Cofacet {
                    index,
                    k,
                    vertex,
                    diameter,
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        }
        None
    }
}

impl Distances for DistanceMatrix {
    fn len(&self) -> usize {
        self.n
    }
    fn get(&self, i: usize, j: usize) -> f64 {
        DistanceMatrix::get(self, i, j)
    }
    fn default_threshold(&self) -> f64 {
        self.enclosing_radius()
    }
    fn for_each_edge(&self, mut f: impl FnMut(usize, usize, f64)) {
        for i in 1..self.n {
            for j in 0..i {
                f(i, j, DistanceMatrix::get(self, i, j));
            }
        }
    }

    /// The dense walk. The diameter fold reads the storage form directly.
    ///
    /// Both forms take the vertices in ascending position, as the default
    /// does, so the diameters and the stopping point match bit for bit.
    #[allow(clippy::too_many_arguments)]
    fn enumerate_cofacets<const BOUNDED: bool, T, F>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
        f: F,
    ) -> Option<T>
    where
        F: FnMut(Cofacet) -> ControlFlow<T>,
    {
        let data = &self.data;
        let n = self.n;
        let square = self.square;
        self.walk_cofacets(bt, simplex, dim, upper_only, f, |added| {
            let mut d = simplex.diameter;
            // The square form reads each simplex vertex's own row, which the
            // descending candidates walk backward, one contiguous run at a
            // time. The condensed form holds no row for the candidates above
            // a vertex, so it reads what the trait default reads. One walk
            // never mixes the two, so the test costs a predicted branch.
            let row = added * added.saturating_sub(1) / 2;
            for &v in verts {
                let x = if square {
                    data[v * n + added]
                } else if v < added {
                    data[row + v]
                } else {
                    data[v * (v - 1) / 2 + added]
                };
                if BOUNDED && x > bound {
                    return None;
                }
                d = d.max(x);
            }
            Some(d)
        })
    }
}

impl DistanceMatrix {
    /// The cofacet walk both storage forms share. `cofacet_diameter` folds
    /// one candidate's diameter and returns `None` for a candidate the
    /// caller's bound drops.
    #[inline]
    fn walk_cofacets<T, F, D>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        dim: usize,
        upper_only: bool,
        mut f: F,
        cofacet_diameter: D,
    ) -> Option<T>
    where
        F: FnMut(Cofacet) -> ControlFlow<T>,
        D: Fn(usize) -> Option<f64>,
    {
        let mut iter = CofacetIter::new(bt, simplex.index, dim, self.n);
        if upper_only {
            while let Some((index, vertex)) = iter.next_upper() {
                let Some(diameter) = cofacet_diameter(vertex) else {
                    continue;
                };
                let cofacet = Cofacet {
                    index,
                    k: 0,
                    vertex,
                    diameter,
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        } else {
            while let Some((index, vertex, k)) = iter.next_all() {
                let Some(diameter) = cofacet_diameter(vertex) else {
                    continue;
                };
                let cofacet = Cofacet {
                    index,
                    k,
                    vertex,
                    diameter,
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        }
        None
    }
}

impl Distances for SparseDistanceMatrix {
    fn len(&self) -> usize {
        self.n
    }
    fn get(&self, i: usize, j: usize) -> f64 {
        SparseDistanceMatrix::get(self, i, j)
    }
    /// Sparse input has no enclosing radius: absent edges are absent at
    /// every scale. The default therefore includes all listed edges.
    fn default_threshold(&self) -> f64 {
        f64::INFINITY
    }
    fn max_distance(&self) -> f64 {
        self.max_distance
    }
    fn for_each_edge(&self, mut f: impl FnMut(usize, usize, f64)) {
        for i in 0..self.n {
            let (start, end) = self.span(i);
            for (&j, &d) in self.indices[start..end]
                .iter()
                .zip(&self.values[start..end])
            {
                let j = j as usize;
                if j < i {
                    f(i, j, d);
                }
            }
        }
    }

    /// Enumerate cofacets from the neighbor lists, as ripser's sparse
    /// coboundary does. An in-complex cofacet adds a vertex adjacent to
    /// every simplex vertex, so the enumeration merges the vertices'
    /// neighbor lists from their high ends instead of scanning all `n`
    /// candidates. It streams: each cofacet reaches `f` as soon as the
    /// merge finds it, so a `Break` stops the merge as well as the
    /// callbacks. A simplex vertex never appears in its own neighbor list,
    /// so the merge excludes the simplex vertices without a separate test.
    /// The descent reads `indices` and takes a distance from `values` only
    /// where every list holds the same vertex.
    ///
    /// The index, the position `k`, and the diameter match the dense
    /// default bit for bit. The index recurrence is the one
    /// [`CofacetIter::advance`] runs, and the diameter folds the same
    /// values in the same order. The enumeration omits cofacets whose
    /// diameter is infinite.
    ///
    /// Under `BOUNDED` the merge drops a candidate as soon as one of its
    /// distances exceeds `bound`. The cursors of the lists it did not reach
    /// stay where they stand. Every later candidate is smaller, so those
    /// cursors pass the same entries then and read nothing twice.
    #[allow(clippy::too_many_arguments)]
    fn enumerate_cofacets<const BOUNDED: bool, T, F>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
        mut f: F,
    ) -> Option<T>
    where
        F: FnMut(Cofacet) -> ControlFlow<T>,
    {
        assert!(
            !verts.is_empty(),
            "cofacet enumeration needs a non-empty simplex"
        );
        let width = verts.len();
        let indices = &self.indices[..];
        let values = &self.values[..];
        let mut inline = [(0usize, 0usize); INLINE_VERTS];
        let mut spill: Vec<(usize, usize)>;
        let cursor: &mut [(usize, usize)] = if width <= INLINE_VERTS {
            &mut inline[..width]
        } else {
            counters::note_spill();
            spill = vec![(0, 0); width];
            &mut spill
        };
        for (slot, &v) in cursor.iter_mut().zip(verts) {
            *slot = self.span(v);
        }
        let floor = cofacet_floor(cursor[0], indices, verts, upper_only);
        let mut idx_below = simplex.index;
        let mut idx_above = 0u64;
        let mut k = dim + 1;
        while let Some((w, first_distance)) =
            next_driver_candidate::<BOUNDED>(cursor, floor, indices, values, bound)
        {
            let diameter = match match_sparse_candidate::<BOUNDED>(
                &mut cursor[1..],
                w,
                indices,
                values,
                bound,
                simplex.diameter.max(first_distance),
            ) {
                CandidateMatch::Exhausted => return None,
                CandidateMatch::Rejected => continue,
                CandidateMatch::Matched(diameter) => diameter,
            };
            let w = w as usize;
            advance_cofacet_index(bt, verts, w, &mut k, &mut idx_below, &mut idx_above);
            debug_assert!(!upper_only || k == dim + 1);
            let cofacet = Cofacet {
                index: idx_above + bt.get(w, k + 1) + idx_below,
                k: if upper_only { 0 } else { k },
                vertex: w,
                diameter,
            };
            counters::note_callback();
            if let ControlFlow::Break(t) = f(cofacet) {
                counters::note_break();
                return Some(t);
            }
        }
        None
    }
}

fn cofacet_floor(
    driver: (usize, usize),
    indices: &[u32],
    vertices: &[usize],
    upper_only: bool,
) -> usize {
    if !upper_only {
        return driver.0;
    }
    let (start, end) = driver;
    let highest = vertices[vertices.len() - 1];
    start + indices[start..end].partition_point(|&vertex| vertex as usize <= highest)
}

fn next_driver_candidate<const BOUNDED: bool>(
    cursor: &mut [(usize, usize)],
    floor: usize,
    indices: &[u32],
    values: &[f64],
    bound: f64,
) -> Option<(u32, f64)> {
    while cursor[0].1 != floor {
        cursor[0].1 -= 1;
        counters::note_candidate();
        let at = cursor[0].1;
        if !BOUNDED || values[at] <= bound {
            return Some((indices[at], values[at]));
        }
    }
    None
}

enum CandidateMatch {
    Exhausted,
    Rejected,
    Matched(f64),
}

fn match_sparse_candidate<const BOUNDED: bool>(
    cursors: &mut [(usize, usize)],
    candidate: u32,
    indices: &[u32],
    values: &[f64],
    bound: f64,
    mut diameter: f64,
) -> CandidateMatch {
    for cursor in cursors {
        loop {
            if cursor.1 == cursor.0 {
                return CandidateMatch::Exhausted;
            }
            counters::note_candidate();
            let at = cursor.1 - 1;
            match indices[at].cmp(&candidate) {
                std::cmp::Ordering::Greater => cursor.1 = at,
                std::cmp::Ordering::Less => return CandidateMatch::Rejected,
                std::cmp::Ordering::Equal => {
                    cursor.1 = at;
                    if BOUNDED && values[at] > bound {
                        return CandidateMatch::Rejected;
                    }
                    diameter = diameter.max(values[at]);
                    break;
                }
            }
        }
    }
    CandidateMatch::Matched(diameter)
}

fn advance_cofacet_index(
    table: &BinomialTable,
    vertices: &[usize],
    candidate: usize,
    position: &mut usize,
    below: &mut u64,
    above: &mut u64,
) {
    while *position >= 1 && vertices[*position - 1] > candidate {
        *below -= table.get(vertices[*position - 1], *position);
        *above += table.get(vertices[*position - 1], *position + 1);
        *position -= 1;
    }
}

/// The sparse cofacet algorithm from 0.5.0, retained as a test reference.
/// It builds the complete candidate set before it emits callbacks.
#[cfg(test)]
impl SparseDistanceMatrix {
    pub(crate) fn for_each_cofacet_reference<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        mut f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        let candidates = self.reference_candidates(simplex, verts);
        emit_reference_candidates(bt, simplex, verts, dim, upper_only, &candidates, &mut f)
    }

    fn reference_candidates(&self, simplex: Simplex, verts: &[usize]) -> Vec<(usize, f64)> {
        let pivot = *verts
            .iter()
            .min_by_key(|&&v| self.degree(v))
            .expect("cofacet enumeration needs a non-empty simplex");
        let (start, end) = self.span(pivot);
        let mut candidates: Vec<(usize, f64)> = Vec::new();
        'w: for &w in &self.indices[start..end] {
            let w = w as usize;
            if verts.binary_search(&w).is_ok() {
                continue;
            }
            let mut diameter = simplex.diameter;
            for &v in verts {
                let d = self.get(w, v);
                if !d.is_finite() {
                    continue 'w;
                }
                diameter = diameter.max(d);
            }
            candidates.push((w, diameter));
        }
        candidates
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn emit_reference_candidates<T>(
    table: &BinomialTable,
    simplex: Simplex,
    vertices: &[usize],
    dimension: usize,
    upper_only: bool,
    candidates: &[(usize, f64)],
    callback: &mut impl FnMut(Cofacet) -> ControlFlow<T>,
) -> Option<T> {
    let mut below = simplex.index;
    let mut above = 0u64;
    let mut position = dimension + 1;
    for &(vertex, diameter) in candidates.iter().rev() {
        advance_cofacet_index(
            table,
            vertices,
            vertex,
            &mut position,
            &mut below,
            &mut above,
        );
        if upper_only && position != dimension + 1 {
            break;
        }
        let cofacet = Cofacet {
            index: above + table.get(vertex, position + 1) + below,
            k: if upper_only { 0 } else { position },
            vertex,
            diameter,
        };
        if let ControlFlow::Break(value) = callback(cofacet) {
            return Some(value);
        }
    }
    None
}

/// Scaled two-norm: exact where the naive sum of squares would overflow or
/// underflow. Finite coordinates whose difference still overflows f64 give
/// +inf.
fn euclidean(a: &[f64], b: &[f64]) -> f64 {
    let m = a
        .iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f64, f64::max);
    if m == 0.0 {
        return 0.0;
    }
    if m.is_infinite() {
        return f64::INFINITY;
    }
    let s: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let r = (x - y) / m;
            r * r
        })
        .sum();
    m * s.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed | 1)
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
    }

    // One cofacet as the bits the frozen rules name: index, sign position,
    // and the diameter compared bit for bit rather than by f64 equality.
    type Bits = (u64, usize, u64);

    fn bits(cf: &Cofacet) -> Bits {
        (cf.index, cf.k, cf.diameter.to_bits())
    }

    // Collect the full sequence a distance source yields for a base simplex.
    fn cofacets<D: Distances>(
        d: &D,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
    ) -> Vec<Bits> {
        let mut out = Vec::new();
        d.for_each_cofacet(bt, simplex, verts, dim, upper_only, |cf| {
            out.push(bits(&cf));
            ControlFlow::<()>::Continue(())
        });
        out
    }

    // The same sequence from the bounded entry point.
    fn bounded_cofacets<D: Distances>(
        d: &D,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
    ) -> Vec<Bits> {
        let mut out = Vec::new();
        d.for_each_cofacet_bounded(bt, simplex, verts, dim, upper_only, bound, |cf| {
            out.push(bits(&cf));
            ControlFlow::<()>::Continue(())
        });
        out
    }

    fn reference_cofacets(
        sparse: &SparseDistanceMatrix,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
    ) -> Vec<Bits> {
        let mut out = Vec::new();
        sparse.for_each_cofacet_reference(bt, simplex, verts, dim, upper_only, |cf| {
            out.push(bits(&cf));
            ControlFlow::<()>::Continue(())
        });
        out
    }

    fn rank(bt: &BinomialTable, verts: &[usize]) -> u64 {
        verts
            .iter()
            .enumerate()
            .map(|(i, &v)| bt.get(v, i + 1))
            .sum()
    }

    // The dense matrix that matches a sparse graph: +inf at every absent pair.
    fn densify(sparse: &SparseDistanceMatrix) -> DistanceMatrix {
        let n = sparse.len();
        let mut condensed = Vec::new();
        for i in 1..n {
            for j in 0..i {
                condensed.push(SparseDistanceMatrix::get(sparse, i, j));
            }
        }
        DistanceMatrix::from_condensed(condensed).unwrap()
    }

    fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
        fn go(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
            if cur.len() == k {
                out.push(cur.clone());
                return;
            }
            for v in start..n {
                cur.push(v);
                go(v + 1, n, k, cur, out);
                cur.pop();
            }
        }
        let mut out = Vec::new();
        go(0, n, k, &mut Vec::new(), &mut out);
        out
    }

    // Every simplex of the graph up to `max_dim`, as (simplex, vertices,
    // dimension). A vertex set is a simplex when every pair is present.
    fn simplices(
        dense: &DistanceMatrix,
        bt: &BinomialTable,
        max_dim: usize,
    ) -> Vec<(Simplex, Vec<usize>, usize)> {
        let mut out = Vec::new();
        for dim in 0..=max_dim {
            for verts in combinations(dense.len(), dim + 1) {
                let mut diameter = 0.0f64;
                let mut real = true;
                for a in 0..verts.len() {
                    for b in 0..a {
                        let d = dense.get(verts[a], verts[b]);
                        if !d.is_finite() {
                            real = false;
                        }
                        diameter = diameter.max(d);
                    }
                }
                if !real {
                    continue;
                }
                let simplex = Simplex {
                    diameter,
                    index: rank(bt, &verts),
                };
                out.push((simplex, verts, dim));
            }
        }
        out
    }

    // The three-way gate on one base simplex. The shipped enumerator, the
    // frozen reference, and the dense default with its infinite diameters
    // removed must give the same bits in the same order, and the order must
    // strictly descend.
    fn check_bits(
        label: &str,
        sparse: &SparseDistanceMatrix,
        dense: &DistanceMatrix,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
    ) {
        for upper_only in [false, true] {
            let expected: Vec<Bits> = cofacets(dense, bt, simplex, verts, dim, upper_only)
                .into_iter()
                .filter(|&(_, _, diameter)| f64::from_bits(diameter).is_finite())
                .collect();
            let reference = reference_cofacets(sparse, bt, simplex, verts, dim, upper_only);
            assert_eq!(
                reference, expected,
                "{label}: reference against dense, verts {verts:?}, upper_only {upper_only}"
            );
            for w in expected.windows(2) {
                assert!(
                    w[0].0 > w[1].0,
                    "{label}: cofacet indices must strictly descend, verts {verts:?}"
                );
            }
            // The shipped path, reached through the trait the engine calls.
            let shipped = cofacets(sparse, bt, simplex, verts, dim, upper_only);
            assert_eq!(
                shipped, expected,
                "{label}: shipped against dense, verts {verts:?}, upper_only {upper_only}"
            );
        }
    }

    // Every bound worth testing on one base simplex: the simplex diameter,
    // which is the bound the engine passes, each cofacet diameter and a
    // value just below it, and the two bounds that keep everything.
    fn bounds(simplex: Simplex, full: &[Bits]) -> Vec<f64> {
        let mut out = vec![simplex.diameter, f64::MAX, f64::INFINITY];
        for &(_, _, diameter) in full {
            let d = f64::from_bits(diameter);
            if d.is_finite() && d > simplex.diameter {
                out.push(d);
                out.push(d.next_down());
            }
        }
        out.sort_unstable_by(f64::total_cmp);
        out.dedup();
        out
    }

    // The bounded entry point on one base simplex. It must give the
    // unbounded sequence filtered to the bound, bits and order alike, on
    // the dense source and on the sparse one.
    fn check_bounded(
        label: &str,
        sparse: &SparseDistanceMatrix,
        dense: &DistanceMatrix,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
    ) {
        for upper_only in [false, true] {
            let dense_full = cofacets(dense, bt, simplex, verts, dim, upper_only);
            let sparse_full = cofacets(sparse, bt, simplex, verts, dim, upper_only);
            for bound in bounds(simplex, &dense_full) {
                let under = |full: &[Bits]| -> Vec<Bits> {
                    full.iter()
                        .copied()
                        .filter(|&(_, _, diameter)| f64::from_bits(diameter) <= bound)
                        .collect()
                };
                let got = bounded_cofacets(dense, bt, simplex, verts, dim, upper_only, bound);
                assert_eq!(
                    got,
                    under(&dense_full),
                    "{label}: bounded dense at {bound}, verts {verts:?}, upper_only {upper_only}"
                );
                let got = bounded_cofacets(sparse, bt, simplex, verts, dim, upper_only, bound);
                assert_eq!(
                    got,
                    under(&sparse_full),
                    "{label}: bounded sparse at {bound}, verts {verts:?}, upper_only {upper_only}"
                );
            }
        }
    }

    // The Break gate on one base simplex: the value passes through, the
    // callbacks stop at the break, and the enumerator reads no neighbor
    // list entry after it. `bound` picks the bounded entry point.
    fn check_breaks(
        label: &str,
        sparse: &SparseDistanceMatrix,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        bound: Option<f64>,
    ) {
        let enumerate =
            |f: &mut dyn FnMut(Cofacet) -> ControlFlow<u64>, upper_only: bool| match bound {
                Some(bound) => {
                    sparse.for_each_cofacet_bounded(bt, simplex, verts, dim, upper_only, bound, f)
                }
                None => sparse.for_each_cofacet(bt, simplex, verts, dim, upper_only, f),
            };
        for upper_only in [false, true] {
            let full = match bound {
                Some(bound) => bounded_cofacets(sparse, bt, simplex, verts, dim, upper_only, bound),
                None => cofacets(sparse, bt, simplex, verts, dim, upper_only),
            };

            // A run that never breaks: no Break value, and one mark of the
            // candidate counter per callback.
            counters::reset();
            let mut marks = Vec::new();
            let out = enumerate(
                &mut |_| {
                    marks.push(counters::read().candidates);
                    ControlFlow::<u64>::Continue(())
                },
                upper_only,
            );
            let events = counters::read();
            assert_eq!(out, None, "{label}: a run without a Break");
            assert_eq!(events.callbacks as usize, full.len(), "{label}: callbacks");
            assert_eq!(events.breaks, 0, "{label}: breaks without a Break");

            for m in 0..full.len() {
                let sentinel = 0xbeef_0000_u64 + m as u64;
                counters::reset();
                let mut seen = Vec::new();
                let mut done = false;
                let out = enumerate(
                    &mut |cf| {
                        assert!(!done, "{label}: called back after a Break at {m}");
                        seen.push(bits(&cf));
                        if seen.len() == m + 1 {
                            done = true;
                            ControlFlow::Break(sentinel)
                        } else {
                            ControlFlow::Continue(())
                        }
                    },
                    upper_only,
                );
                let events = counters::read();
                assert_eq!(out, Some(sentinel), "{label}: Break value at {m}");
                assert_eq!(seen, full[..=m], "{label}: callback prefix at {m}");
                assert_eq!(
                    events.callbacks as usize,
                    m + 1,
                    "{label}: callbacks at {m}"
                );
                assert_eq!(events.breaks, 1, "{label}: breaks at {m}");
                assert_eq!(
                    events.candidates, marks[m],
                    "{label}: neighbor list entries read after the Break at {m}"
                );
            }
        }
    }

    // Both gates on every simplex of a graph up to `max_dim`.
    fn check_graph(label: &str, sparse: &SparseDistanceMatrix, max_dim: usize) {
        let dense = densify(sparse);
        let n = sparse.len();
        let bt = BinomialTable::new(n.max(1), max_dim + 2).unwrap();
        for (simplex, verts, dim) in simplices(&dense, &bt, max_dim) {
            check_bits(label, sparse, &dense, &bt, simplex, &verts, dim);
            check_bounded(label, sparse, &dense, &bt, simplex, &verts, dim);
            check_breaks(label, sparse, &bt, simplex, &verts, dim, None);
            check_breaks(
                label,
                sparse,
                &bt,
                simplex,
                &verts,
                dim,
                Some(simplex.diameter),
            );
        }
    }

    fn graph(n: usize, triplets: &[(usize, usize, f64)]) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(n, triplets).unwrap()
    }

    // The adversarial graphs, each one a shape that defeats a plausible
    // enumerator shortcut.
    fn adversarial_fixtures() -> Vec<(&'static str, SparseDistanceMatrix)> {
        let mut fixtures = vec![
            star_fixture(),
            joined_cliques_fixture(),
            bipartite_fixture(),
            all_equal_fixture(),
            duplicate_points_fixture(),
            skewed_fixture(),
            disconnected_fixture(),
        ];
        fixtures.extend(cut_fixtures());
        fixtures.push(complete_fixture());
        fixtures.push(("one point", graph(1, &[])));
        fixtures.push(("two points", graph(2, &[(0, 1, 1.0)])));
        fixtures.push(("edge across the range", graph(5, &[(0, 4, 1.0)])));
        fixtures
    }

    fn star_fixture() -> (&'static str, SparseDistanceMatrix) {
        let edges: Vec<_> = (1..7)
            .map(|vertex| (0, vertex, 1.0 + vertex as f64))
            .collect();
        ("star", graph(7, &edges))
    }

    fn joined_cliques_fixture() -> (&'static str, SparseDistanceMatrix) {
        let mut edges = Vec::new();
        for a in 0..4 {
            for b in 0..a {
                edges.push((a, b, 1.0));
                edges.push((a + 4, b + 4, 2.0));
            }
        }
        edges.push((3, 4, 3.0));
        ("joined cliques", graph(8, &edges))
    }

    fn bipartite_fixture() -> (&'static str, SparseDistanceMatrix) {
        let mut edges = Vec::new();
        for a in 0..3 {
            for b in 3..6 {
                edges.push((a, b, 1.0 + a as f64));
            }
        }
        ("bipartite", graph(6, &edges))
    }

    fn all_equal_fixture() -> (&'static str, SparseDistanceMatrix) {
        let mut edges = Vec::new();
        for a in 0..6 {
            for b in 0..a {
                edges.push((a, b, 2.0));
            }
        }
        ("all equal", graph(6, &edges))
    }

    fn duplicate_points_fixture() -> (&'static str, SparseDistanceMatrix) {
        let mut edges = Vec::new();
        for a in 0..6 {
            for b in 0..a {
                let distance = if a < 3 { 0.0 } else { 1.0 + b as f64 };
                edges.push((a, b, distance));
            }
        }
        ("duplicate points", graph(6, &edges))
    }

    fn skewed_fixture() -> (&'static str, SparseDistanceMatrix) {
        let mut edges = vec![(0, 1, 1.0), (0, 2, 1.0)];
        for a in 1..7 {
            for b in 1..a {
                if (a + b) % 3 != 0 {
                    edges.push((a, b, 1.0 + (a * b) as f64 / 8.0));
                }
            }
        }
        ("least selective pivot", graph(7, &edges))
    }

    fn disconnected_fixture() -> (&'static str, SparseDistanceMatrix) {
        (
            "disconnected",
            graph(
                7,
                &[
                    (0, 1, 1.0),
                    (0, 2, 1.0),
                    (1, 2, 1.0),
                    (3, 4, 2.0),
                    (3, 5, 2.0),
                    (4, 5, 2.0),
                ],
            ),
        )
    }

    fn cut_fixtures() -> Vec<(&'static str, SparseDistanceMatrix)> {
        [
            ("threshold at the smallest edge", 1.0),
            ("threshold at a tie", 3.0),
            ("threshold between edge values", 2.5),
        ]
        .into_iter()
        .map(|(label, threshold)| (label, graph(7, &quantized_edges(threshold))))
        .collect()
    }

    fn quantized_edges(threshold: f64) -> Vec<(usize, usize, f64)> {
        let mut edges = Vec::new();
        for a in 0..7 {
            for b in 0..a {
                let distance = 1.0 + ((b * 7 + a) % 4) as f64;
                if distance <= threshold {
                    edges.push((a, b, distance));
                }
            }
        }
        edges
    }

    fn complete_fixture() -> (&'static str, SparseDistanceMatrix) {
        let mut complete = Vec::new();
        for a in 0..7 {
            for b in 0..a {
                complete.push((a, b, 1.0 + ((a * 5 + b) % 4) as f64));
            }
        }
        ("dense as sparse", graph(7, &complete))
    }

    // A random sparse graph plus the dense matrix that uses +inf for every
    // absent pair. The dense default then enumerates the same cofacets. It
    // gives the missing ones an infinite diameter that the sparse side omits.
    fn random_graph(rng: &mut Rng, n: usize) -> (SparseDistanceMatrix, DistanceMatrix) {
        // Duplicates and a zero so the diameter fold is exercised.
        let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0];
        let mut triplets = Vec::new();
        let mut condensed = Vec::new();
        for i in 1..n {
            for j in 0..i {
                if rng.below(3) > 0 {
                    let w = palette[rng.below(palette.len())];
                    triplets.push((i, j, w));
                    condensed.push(w);
                } else {
                    condensed.push(f64::INFINITY);
                }
            }
        }
        (
            SparseDistanceMatrix::from_triplets(n, &triplets).unwrap(),
            DistanceMatrix::from_condensed(condensed).unwrap(),
        )
    }

    // A random graph with the density and the distance palette the caller
    // asks for. `present` is the chance in a thousand that a pair is an
    // edge, so a caller can reach a near-empty or a complete graph.
    fn random_graph_shaped(
        rng: &mut Rng,
        n: usize,
        present: usize,
        palette: &[f64],
    ) -> SparseDistanceMatrix {
        let mut triplets = Vec::new();
        for i in 1..n {
            for j in 0..i {
                if rng.below(1000) < present {
                    triplets.push((i, j, palette[rng.below(palette.len())]));
                }
            }
        }
        SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
    }

    // The sparse override must yield exactly what the dense default yields
    // once its infinite-diameter (absent-neighbor) cofacets are dropped: the
    // same indices, k, diameters, and descending order. The frozen reference
    // stands between the two, so the shipped enumerator is compared against
    // the body it replaced as well as against the dense one.
    #[test]
    fn sparse_cofacets_match_dense_default() {
        let mut rng = Rng::new(0xc0fa_ce75_0000_0001);
        let mut trials = 0usize;
        for _ in 0..4000 {
            let n = 4 + rng.below(9);
            let (sparse, dense) = random_graph(&mut rng, n);
            let bt = BinomialTable::new(n, 6).unwrap();
            let dim = 1 + rng.below(3); // base simplex dimension 1..=3
            if dim + 1 > n {
                continue;
            }
            // A genuine base simplex: distinct vertices, all pairs present.
            let mut verts: Vec<usize> = Vec::new();
            while verts.len() < dim + 1 {
                let v = rng.below(n);
                if !verts.contains(&v) {
                    verts.push(v);
                }
            }
            verts.sort_unstable();
            let mut diameter = 0.0f64;
            let mut real = true;
            for a in 0..verts.len() {
                for b in 0..a {
                    let d = dense.get(verts[a], verts[b]);
                    if !d.is_finite() {
                        real = false;
                    }
                    diameter = diameter.max(d);
                }
            }
            if !real {
                continue;
            }
            let simplex = Simplex {
                diameter,
                index: rank(&bt, &verts),
            };
            check_bits("random", &sparse, &dense, &bt, simplex, &verts, dim);
            check_bounded("random", &sparse, &dense, &bt, simplex, &verts, dim);
            trials += 1;
        }
        assert!(
            trials > 500,
            "too few genuine simplices exercised: {trials}"
        );
    }

    // Degenerate intersections stay in lockstep with the dense default: an
    // empty pivot neighbor list (isolated vertex), an empty intersection
    // with both endpoints non-empty, and an ordinary non-empty case.
    #[test]
    fn sparse_cofacets_empty_intersections() {
        // Triangle {0,1,2}, a disjoint edge 3-4, and an isolated vertex 5.
        let sparse = SparseDistanceMatrix::from_triplets(
            6,
            &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0), (3, 4, 2.0)],
        )
        .unwrap();
        let inf = f64::INFINITY;
        let dense = DistanceMatrix::from_condensed(vec![
            1.0, // 1-0
            1.0, 1.0, // 2-0, 2-1
            inf, inf, inf, // 3-*
            inf, inf, inf, 2.0, // 4-*, 4-3
            inf, inf, inf, inf, inf, // 5-*
        ])
        .unwrap();
        let bt = BinomialTable::new(6, 6).unwrap();

        // {0,1}: common neighbor 2 (non-empty). {0,3}: 0->{1,2}, 3->{4}, no
        // common vertex (empty intersection, both lists non-empty). {0,5}:
        // vertex 5 is isolated, so the pivot list is empty.
        for verts in [[0usize, 1usize], [0, 3], [0, 5]] {
            let d01 = dense.get(verts[0], verts[1]);
            let simplex = Simplex {
                diameter: d01,
                index: rank(&bt, &verts),
            };
            check_bits("degenerate", &sparse, &dense, &bt, simplex, &verts, 1);
        }
    }

    // The named adversarial graphs, every simplex of each, by bits and by
    // Break position.
    #[test]
    fn adversarial_graphs_match_the_reference() {
        for (label, sparse) in adversarial_fixtures() {
            check_graph(label, &sparse, 3.min(sparse.len().saturating_sub(1)));
        }
    }

    // Randomized shapes the fixed graphs do not reach: a complete graph, a
    // graph so thin that most intersections are empty, an all-equal graph,
    // and one with duplicate points. Every simplex of every draw is checked.
    #[test]
    fn random_shapes_match_the_reference() {
        let mut rng = Rng::new(0x5ea5_0f17_0000_0003);
        let shapes: [(&str, usize, &[f64]); 4] = [
            ("complete", 1000, &[1.0, 2.0, 3.0]),
            ("thin", 120, &[1.0, 2.0]),
            ("all equal", 700, &[2.0]),
            ("duplicate points", 700, &[0.0, 0.0, 1.0]),
        ];
        for (label, present, palette) in shapes {
            for _ in 0..12 {
                let n = 5 + rng.below(4);
                let sparse = random_graph_shaped(&mut rng, n, present, palette);
                check_graph(label, &sparse, 3.min(n - 1));
            }
        }
    }

    // A simplex wider than the inline cursor array must enumerate from the
    // heap and stay bit-exact. Nothing narrower may allocate.
    #[test]
    fn wide_simplices_spill_to_the_heap() {
        let n = 20;
        let mut triplets = Vec::new();
        for a in 0..n {
            for b in 0..a {
                triplets.push((a, b, 1.0 + ((a * 3 + b) % 5) as f64));
            }
        }
        let sparse = graph(n, &triplets);
        let dense = densify(&sparse);
        let bt = BinomialTable::new(n, INLINE_VERTS + 4).unwrap();
        // Widths on both sides of the inline bound, including the first
        // width that spills.
        for width in [
            INLINE_VERTS - 1,
            INLINE_VERTS,
            INLINE_VERTS + 1,
            INLINE_VERTS + 2,
        ] {
            let verts: Vec<usize> = (0..width).collect();
            let dim = width - 1;
            let mut diameter = 0.0f64;
            for a in 0..width {
                for b in 0..a {
                    diameter = diameter.max(dense.get(verts[a], verts[b]));
                }
            }
            let simplex = Simplex {
                diameter,
                index: rank(&bt, &verts),
            };
            check_bits("wide", &sparse, &dense, &bt, simplex, &verts, dim);
            check_bounded("wide", &sparse, &dense, &bt, simplex, &verts, dim);
            check_breaks("wide", &sparse, &bt, simplex, &verts, dim, None);

            counters::reset();
            let got = cofacets(&sparse, &bt, simplex, &verts, dim, false);
            let spills = counters::read().spills;
            assert!(!got.is_empty(), "width {width}: nothing to enumerate");
            if width > INLINE_VERTS {
                assert_eq!(spills, 1, "width {width}: the cursors must spill once");
            } else {
                assert_eq!(spills, 0, "width {width}: the cursors must not allocate");
            }
        }
    }

    // A dense source that counts the distance reads its enumerator makes.
    struct Counting<'a> {
        inner: &'a DistanceMatrix,
        reads: std::cell::Cell<usize>,
    }

    impl Distances for Counting<'_> {
        fn len(&self) -> usize {
            self.inner.len()
        }
        fn get(&self, i: usize, j: usize) -> f64 {
            self.reads.set(self.reads.get() + 1);
            self.inner.get(i, j)
        }
        fn default_threshold(&self) -> f64 {
            self.inner.enclosing_radius()
        }
        fn for_each_edge(&self, f: impl FnMut(usize, usize, f64)) {
            Distances::for_each_edge(self.inner, f)
        }
    }

    // The bounded fold reads distances until one exceeds the bound, and it
    // stops there. The edge {0,1} of this four-point matrix has the cofacets
    // 3 and then 2. Vertex 3 is far from vertex 0, so it costs one read and
    // no callback; vertex 2 is near both, so it costs two reads and reaches
    // the callback. The unbounded walk reads all four distances and reports
    // both cofacets.
    #[test]
    fn the_bounded_fold_stops_at_the_first_distance_above_the_bound() {
        // Condensed order: (1,0), (2,0), (2,1), (3,0), (3,1), (3,2).
        let dense = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0, 5.0, 1.0, 1.0]).unwrap();
        let counting = Counting {
            inner: &dense,
            reads: std::cell::Cell::new(0),
        };
        let bt = BinomialTable::new(4, 3).unwrap();
        let verts = [0usize, 1usize];
        let simplex = Simplex {
            diameter: 1.0,
            index: rank(&bt, &verts),
        };

        let full = cofacets(&counting, &bt, simplex, &verts, 1, false);
        assert_eq!(counting.reads.replace(0), 4, "reads of the unbounded walk");
        assert_eq!(full.len(), 2, "cofacets of the unbounded walk");

        let got = bounded_cofacets(&counting, &bt, simplex, &verts, 1, false, simplex.diameter);
        assert_eq!(counting.reads.replace(0), 3, "reads of the bounded walk");
        let expected: Vec<Bits> = full
            .into_iter()
            .filter(|&(_, _, diameter)| f64::from_bits(diameter) <= simplex.diameter)
            .collect();
        assert_eq!(got, expected, "cofacets of the bounded walk");
    }

    // Under `upper_only` the merge stops at the first candidate that is not
    // above every simplex vertex. The fixture puts two qualifying candidates
    // above the edge {2, 3} and two failing ones below it, so a walk that
    // read past the boundary would confirm 1 and 0 against the second list
    // and count more entries.
    #[test]
    fn the_upper_only_merge_stops_at_the_top_simplex_vertex() {
        let far = 5.0;
        let near = 1.0;
        let mut triplets = vec![(2usize, 3usize, near)];
        for v in [0usize, 1] {
            triplets.push((2, v, far));
            triplets.push((3, v, far));
        }
        for v in [4usize, 5] {
            triplets.push((2, v, near));
            triplets.push((3, v, near));
        }
        let sparse = SparseDistanceMatrix::from_triplets(6, &triplets).unwrap();
        let bt = BinomialTable::new(6, 3).unwrap();
        let verts = [2usize, 3usize];
        let simplex = Simplex {
            diameter: near,
            index: rank(&bt, &verts),
        };

        counters::reset();
        let got = bounded_cofacets(&sparse, &bt, simplex, &verts, 1, true, simplex.diameter);
        let events = counters::read();
        assert_eq!(got.len(), 2, "cofacets above the edge");
        // The two entries of vertex 2's list above the edge, and one entry of
        // vertex 3's list per confirmed candidate. Nothing below the edge is
        // read at all.
        assert_eq!(events.candidates, 4, "neighbor list entries read");
    }

    // A bound no stored distance reaches drops nothing, so the walk reads
    // exactly what the unbounded walk reads and reports the same cofacets.
    #[test]
    fn a_vacuous_bound_drops_nothing() {
        let mut rng = Rng::new(0x51ed_2701);
        let (sparse, _) = random_graph(&mut rng, 7);
        let bt = BinomialTable::new(7, 3).unwrap();
        let (u, v) = sparse
            .edges()
            .map(|(u, v, _)| (u, v))
            .find(|&(u, v)| u > 0 && v < 6)
            .expect("the fixture needs an edge with room on both sides");
        let verts = [u, v];
        let simplex = Simplex {
            diameter: sparse.get(u, v),
            index: rank(&bt, &verts),
        };
        for upper_only in [false, true] {
            counters::reset();
            let plain = cofacets(&sparse, &bt, simplex, &verts, 1, upper_only);
            let unbounded = counters::read().candidates;
            for bound in [sparse.max_distance(), f64::MAX, f64::INFINITY] {
                counters::reset();
                let got = bounded_cofacets(&sparse, &bt, simplex, &verts, 1, upper_only, bound);
                let events = counters::read();
                assert_eq!(got, plain, "bits at the vacuous bound {bound}");
                assert_eq!(events.vacuous, 1, "raised bounds at {bound}");
                assert_eq!(
                    events.candidates, unbounded,
                    "entries read at the vacuous bound {bound}"
                );
            }
            // A bound under the largest stored distance keeps the test in
            // the merge.
            counters::reset();
            bounded_cofacets(
                &sparse,
                &bt,
                simplex,
                &verts,
                1,
                upper_only,
                sparse.max_distance().next_down(),
            );
            assert_eq!(
                counters::read().vacuous,
                0,
                "a bound that a distance reaches"
            );
        }
    }

    // The bound the engine passes is the simplex diameter, and a simplex of
    // identical points has diameter zero. Both zeros are legal bounds there,
    // and neither drops a cofacet at distance zero.
    #[test]
    fn a_zero_bound_keeps_the_cofacets_at_zero() {
        let triplets: Vec<(usize, usize, f64)> = (1..5)
            .flat_map(|i| (0..i).map(move |j| (i, j, 0.0)))
            .collect();
        let sparse = SparseDistanceMatrix::from_triplets(5, &triplets).unwrap();
        let dense = densify(&sparse);
        let bt = BinomialTable::new(5, 3).unwrap();
        let verts = [1usize, 2usize];
        let simplex = Simplex {
            diameter: 0.0,
            index: rank(&bt, &verts),
        };
        for upper_only in [false, true] {
            let expected = cofacets(&dense, &bt, simplex, &verts, 1, upper_only);
            for bound in [0.0f64, -0.0f64] {
                let got = bounded_cofacets(&dense, &bt, simplex, &verts, 1, upper_only, bound);
                assert_eq!(got, expected, "dense at bound {bound}");
                let got = bounded_cofacets(&sparse, &bt, simplex, &verts, 1, upper_only, bound);
                assert_eq!(got, expected, "sparse at bound {bound}");
            }
        }
    }

    // The dense source overrides the walk; `Counting` does not, so it runs
    // the default body on the same distances. The two must agree in bits and
    // in order, bounded and unbounded alike.
    #[test]
    fn the_dense_walk_matches_the_default_fold() {
        let mut rng = Rng::new(0x9e37_79b9);
        for n in [3usize, 5, 7] {
            for round in 0..4 {
                let (_, dense) = random_graph(&mut rng, n);
                let max_dim = if round % 2 == 0 { 1 } else { 2 };
                let bt = BinomialTable::new(n, max_dim + 2).unwrap();
                let plain = Counting {
                    inner: &dense,
                    reads: std::cell::Cell::new(0),
                };
                for (simplex, verts, dim) in simplices(&dense, &bt, max_dim) {
                    for upper_only in [false, true] {
                        let expected = cofacets(&plain, &bt, simplex, &verts, dim, upper_only);
                        for square in [false, true] {
                            let form = if square {
                                dense.to_square()
                            } else {
                                dense.clone()
                            };
                            let got = cofacets(&form, &bt, simplex, &verts, dim, upper_only);
                            assert_eq!(
                                got, expected,
                                "n {n}, verts {verts:?}, upper {upper_only}, square {square}"
                            );
                            for bound in bounds(simplex, &expected) {
                                let want = bounded_cofacets(
                                    &plain, &bt, simplex, &verts, dim, upper_only, bound,
                                );
                                let got = bounded_cofacets(
                                    &form, &bt, simplex, &verts, dim, upper_only, bound,
                                );
                                assert_eq!(
                                    got, want,
                                    "n {n}, verts {verts:?}, upper {upper_only},                                      bound {bound}, square {square}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Both storage forms answer every query with the same bits.
    #[test]
    fn the_two_storage_forms_agree() {
        let mut rng = Rng::new(0x2f19_a7c3);
        let (_, condensed) = random_graph(&mut rng, 9);
        let square = condensed.to_square();
        assert!(
            !condensed.is_square(),
            "every constructor builds the compact form"
        );
        assert!(square.is_square());
        for i in 0..condensed.len() {
            for j in 0..condensed.len() {
                assert_eq!(
                    square.get(i, j).to_bits(),
                    condensed.get(i, j).to_bits(),
                    "get({i}, {j})"
                );
            }
        }
        assert_eq!(
            square.enclosing_radius().to_bits(),
            condensed.enclosing_radius().to_bits(),
            "enclosing radius"
        );
        for threshold in [0.0, 1.0, 2.0, f64::INFINITY] {
            assert_eq!(
                square.count_edges_at(threshold),
                condensed.count_edges_at(threshold),
                "edges at {threshold}"
            );
            let a = square.to_sparse_at(threshold).unwrap();
            let b = condensed.to_sparse_at(threshold).unwrap();
            let edges = |m: &SparseDistanceMatrix| -> Vec<(usize, usize, u64)> {
                m.edges().map(|(u, v, d)| (u, v, d.to_bits())).collect()
            };
            assert_eq!(edges(&a), edges(&b), "graph at {threshold}");
        }
    }

    #[test]
    fn scaled_norm_survives_extreme_magnitudes() {
        let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e200]]).unwrap();
        assert_eq!(d.get(0, 1), 1e200);
        let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e-200]]).unwrap();
        assert_eq!(d.get(0, 1), 1e-200);
        let d = DistanceMatrix::from_points(&[vec![3e200, 0.0], vec![0.0, 4e200]]).unwrap();
        assert!((d.get(0, 1) / 5e200 - 1.0).abs() < 1e-15);
    }

    fn edge_bits(matrix: &SparseDistanceMatrix) -> Vec<(usize, usize, u64)> {
        matrix
            .edges()
            .map(|(u, v, distance)| (u, v, distance.to_bits()))
            .collect()
    }

    #[test]
    fn threshold_point_kernels_match_the_dense_constructor() {
        let mut rng = Rng::new(0x8d47_2016_7f31);
        for dimensions in [0usize, 1, 2, 3, 7, 13] {
            let mut points = Vec::new();
            for i in 0..47 {
                let point = (0..dimensions)
                    .map(|axis| {
                        let raw = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                        if i % 11 == 0 {
                            axis as f64 * 0.125
                        } else {
                            raw.mul_add(4.0, -2.0)
                        }
                    })
                    .collect();
                points.push(point);
            }
            let dense = DistanceMatrix::from_points(&points).unwrap();
            for threshold in [0.0, 0.25, 1.0, 4.0, f64::INFINITY] {
                let expected = edge_bits(&dense.to_sparse_at(threshold).unwrap());
                for strategy in [PointCloudStrategy::KdTree, PointCloudStrategy::Exhaustive] {
                    let serial = PointCloudGraph::build(
                        &points,
                        PointCloudParams::new(threshold).with_strategy(strategy),
                    )
                    .unwrap();
                    let parallel = PointCloudGraph::build(
                        &points,
                        PointCloudParams::new(threshold)
                            .with_strategy(strategy)
                            .with_threads(3),
                    )
                    .unwrap();
                    assert_eq!(
                        edge_bits(serial.matrix()),
                        expected,
                        "dimensions {dimensions}, threshold {threshold}, strategy {strategy:?}"
                    );
                    assert_eq!(edge_bits(parallel.matrix()), expected);
                    assert_eq!(serial.stats(), parallel.stats());
                }
            }
        }
    }

    #[test]
    fn threshold_point_constructor_preserves_extreme_distance_bits() {
        let points = vec![
            vec![0.0, 0.0],
            vec![1e-200, 0.0],
            vec![0.0, 1e200],
            vec![1e308, 0.0],
            vec![-1e308, 0.0],
        ];
        let dense = DistanceMatrix::from_points(&points).unwrap();
        for threshold in [0.0, 1e-200, 1e200, f64::MAX, f64::INFINITY] {
            let expected = edge_bits(&dense.to_sparse_at(threshold).unwrap());
            let graph = PointCloudGraph::build(&points, PointCloudParams::new(threshold)).unwrap();
            assert_eq!(edge_bits(graph.matrix()), expected, "threshold {threshold}");
        }
    }

    #[test]
    fn automatic_point_route_is_frozen() {
        let low = vec![vec![0.0; 12], vec![1.0; 12]];
        let high = vec![vec![0.0; 13], vec![1.0; 13]];
        assert_eq!(
            PointCloudGraph::build(&low, PointCloudParams::new(1.0))
                .unwrap()
                .stats()
                .strategy,
            PointCloudStrategy::KdTree
        );
        assert_eq!(
            PointCloudGraph::build(&high, PointCloudParams::new(1.0))
                .unwrap()
                .stats()
                .strategy,
            PointCloudStrategy::Exhaustive
        );
        assert_eq!(
            PointCloudGraph::build(&low, PointCloudParams::new(f64::INFINITY))
                .unwrap()
                .stats()
                .strategy,
            PointCloudStrategy::Exhaustive
        );
    }

    #[test]
    fn overflowing_difference_is_an_absent_edge() {
        let d = DistanceMatrix::from_points(&[vec![1e308], vec![-1e308]]).unwrap();
        assert_eq!(d.get(0, 1), f64::INFINITY);
    }

    #[test]
    fn non_finite_coordinates_are_rejected() {
        assert!(DistanceMatrix::from_points(&[vec![f64::INFINITY], vec![0.0]]).is_err());
        assert!(DistanceMatrix::from_points(&[vec![f64::NAN], vec![0.0]]).is_err());
    }

    #[test]
    fn negative_zero_entries_are_normalized() {
        let d = DistanceMatrix::from_condensed(vec![-0.0]).unwrap();
        assert!(d.get(0, 1).is_sign_positive());
    }

    #[test]
    fn validation_errors_carry_the_condensed_index() {
        let err = DistanceMatrix::from_condensed(vec![1.0, f64::NAN, 1.0]).unwrap_err();
        assert!(err.to_string().contains("index 1"), "{err}");
        let err = DistanceMatrix::from_condensed(vec![1.0, 1.0, -2.0]).unwrap_err();
        assert!(err.to_string().contains("index 2"), "{err}");
    }

    #[test]
    fn empty_condensed_means_one_point() {
        assert_eq!(DistanceMatrix::from_condensed(vec![]).unwrap().len(), 1);
    }
}
