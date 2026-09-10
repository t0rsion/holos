//! Condensed distance conversion.

/// Reorder SciPy `pdist` data into the core lower-triangle layout.
///
/// `pdist` emits the upper triangle row by row (d01, d02, ..., d12, ...).
/// The constructor stores the lower triangle (d10, d20, d21, ...).
/// The Python contract is the pdist layout.
pub(crate) fn pdist_to_lower(data: Vec<f64>) -> Result<Vec<f64>, holos_tda::Error> {
    let m = data.len();
    let n = ((1.0 + 8.0 * m as f64).sqrt() as usize).div_ceil(2);
    if n * (n - 1) / 2 != m {
        return Err(holos_tda::Error::InvalidInput(format!(
            "condensed length {m} is not n(n-1)/2 for any n"
        )));
    }
    let mut lower = vec![0.0; m];
    let mut pos = 0;
    for i in 0..n {
        for j in i + 1..n {
            lower[j * (j - 1) / 2 + i] = data[pos];
            pos += 1;
        }
    }
    Ok(lower)
}
