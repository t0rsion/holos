use rayon::prelude::*;

use super::construction::{euclidean, validate_points};
#[allow(unused_imports)]
use super::matrix::DistanceMatrix;
use super::sparse::SparseDistanceMatrix;
use crate::{Error, Result};

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
