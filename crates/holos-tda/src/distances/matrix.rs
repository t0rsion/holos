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
    pub(super) n: usize,
    /// Row-major n by n in the full form, the condensed lower triangle in
    /// the compact one. Row `i` holds its entries below the diagonal first
    /// in both, which is what [`DistanceMatrix::lower_row`] returns.
    pub(super) data: Vec<f64>,
    pub(super) square: bool,
}

impl DistanceMatrix {
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
    pub(super) fn lower_row(&self, i: usize) -> &[f64] {
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
    /// Default threshold: the minimum over i of the maximum over j of d(i, j).
    /// Past that radius the complex is a cone and acquires no further homology.
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
