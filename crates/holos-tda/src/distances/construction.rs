use super::matrix::DistanceMatrix;
use super::sparse::SparseDistanceMatrix;
use crate::{Error, Result};

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
}

impl DistanceMatrix {
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
}

pub(super) fn validate_points(points: &[Vec<f64>]) -> Result<usize> {
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

/// Scaled two-norm: exact where the naive sum of squares would overflow or
/// underflow. Finite coordinates whose difference still overflows f64 give
/// +inf.
pub(super) fn euclidean(a: &[f64], b: &[f64]) -> f64 {
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
