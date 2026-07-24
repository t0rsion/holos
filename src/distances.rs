use crate::{Error, Result};

/// Symmetric dissimilarity matrix in condensed lower-triangle form.
/// No metric assumptions: entries need not satisfy the triangle inequality.
/// Entries must be non-negative and not NaN; +inf is legal and equivalent
/// to an absent edge.
#[derive(Debug, Clone)]
pub struct DistanceMatrix {
    n: usize,
    data: Vec<f64>,
}

impl DistanceMatrix {
    /// Euclidean distances of a point cloud. Coordinates must be finite.
    pub fn from_points(points: &[Vec<f64>]) -> Result<Self> {
        let n = points.len();
        if n > 0 {
            let d = points[0].len();
            if let Some(p) = points.iter().find(|p| p.len() != d) {
                return Err(Error::InvalidInput(format!(
                    "inconsistent point dimensions: {} vs {}",
                    d,
                    p.len()
                )));
            }
            if points.iter().flatten().any(|x| !x.is_finite()) {
                return Err(Error::InvalidInput("non-finite coordinate".into()));
            }
        }
        let mut data = Vec::with_capacity(n.saturating_sub(1) * n / 2);
        for i in 1..n {
            for j in 0..i {
                data.push(euclidean(&points[i], &points[j]));
            }
        }
        Ok(Self { n, data })
    }

    /// Condensed lower triangle, row by row: d(1,0), d(2,0), d(2,1), d(3,0), ...
    /// An empty vector means one point (n = 1); an empty *space* (n = 0) is
    /// only representable through [`DistanceMatrix::from_points`].
    pub fn from_condensed(mut data: Vec<f64>) -> Result<Self> {
        let m = data.len();
        let n = ((1.0 + 8.0 * m as f64).sqrt() as usize).div_ceil(2);
        if n * (n - 1) / 2 != m {
            return Err(Error::InvalidInput(format!(
                "condensed length {m} is not n(n-1)/2 for any n"
            )));
        }
        for (i, d) in data.iter_mut().enumerate() {
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
        Ok(Self { n, data })
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
        match i.cmp(&j) {
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => self.data[i * (i - 1) / 2 + j],
            std::cmp::Ordering::Less => self.data[j * (j - 1) / 2 + i],
        }
    }

    /// min over i of max over j of d(i,j): the radius past which the complex
    /// is a cone and acquires no further homology. Used as the default
    /// threshold; exactness-preserving for full persistence.
    pub fn enclosing_radius(&self) -> f64 {
        if self.n < 2 {
            return 0.0;
        }
        (0..self.n)
            .map(|i| {
                (0..self.n)
                    .filter(|&j| j != i)
                    .map(|j| self.get(i, j))
                    .fold(0.0f64, f64::max)
            })
            .fold(f64::INFINITY, f64::min)
    }
}

/// Scaled two-norm: exact where the naive sum of squares would overflow or
/// underflow. Finite coordinates whose difference still overflows f64 yield
/// +inf, which the complex treats as an absent edge.
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

    #[test]
    fn scaled_norm_survives_extreme_magnitudes() {
        let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e200]]).unwrap();
        assert_eq!(d.get(0, 1), 1e200);
        let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e-200]]).unwrap();
        assert_eq!(d.get(0, 1), 1e-200);
        let d = DistanceMatrix::from_points(&[vec![3e200, 0.0], vec![0.0, 4e200]]).unwrap();
        assert!((d.get(0, 1) / 5e200 - 1.0).abs() < 1e-15);
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
