use crate::{Error, Result};

#[allow(unused_imports)]
use super::matrix::DistanceMatrix;

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
    pub(super) n: usize,
    /// Where each vertex's neighbor list starts, plus the total at the end.
    /// Length `n + 1`.
    pub(super) offsets: Vec<usize>,
    /// Neighbor vertices, per vertex ascending. `from_triplets` rejects an
    /// `n` above `u32::MAX`, so a vertex fits in a `u32`.
    pub(super) indices: Vec<u32>,
    /// The distance to the neighbor at the same position in `indices`.
    pub(super) values: Vec<f64>,
    /// The largest stored distance, or 0 when no pair is stored.
    pub(super) max_distance: f64,
}

impl SparseDistanceMatrix {
    pub(super) fn from_lower_rows(n: usize, rows: &[Vec<(usize, f64)>]) -> Result<Self> {
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
    pub(super) fn span(&self, v: usize) -> (usize, usize) {
        (self.offsets[v], self.offsets[v + 1])
    }

    /// How many neighbors vertex `v` has.
    #[cfg(test)]
    #[inline]
    pub(super) fn degree(&self, v: usize) -> usize {
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
