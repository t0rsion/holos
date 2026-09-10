//! Bounded parser for the prepared temporal trajectory.

use std::fs;
use std::path::Path;

use holos_tda::SparseDistanceMatrix;

const MAGIC: &str = "HOLOSTEM1";
const MAX_FILE_BYTES: u64 = 1 << 29;
const MAX_VERTICES: usize = 1_000_000;
const MAX_EDGES: usize = 20_000_000;
const MAX_SNAPSHOTS: usize = 1_000_000;
const MAX_CELLS: usize = 20_000_000;

pub(crate) struct Metadata {
    pub(crate) dataset: String,
    pub(crate) source_sha256: String,
    pub(crate) vertices: usize,
    pub(crate) edges: usize,
    pub(crate) snapshots: usize,
    pub(crate) bin_seconds: u64,
    pub(crate) warmup_bins: usize,
    pub(crate) decay_numerator: u64,
    pub(crate) decay_denominator: u64,
    pub(crate) score_scale: u64,
    pub(crate) weight_offset: u64,
    pub(crate) maximum_score: u64,
}

pub(crate) struct Trajectory {
    pub(crate) metadata: Metadata,
    pub(crate) snapshot_times: Vec<u64>,
    pub(crate) graphs: Vec<SparseDistanceMatrix>,
}

pub(crate) fn read(path: &Path) -> Result<Trajectory, String> {
    let contents = read_contents(path)?;
    let mut tokens = contents.split_whitespace();
    let metadata = read_metadata(&mut tokens)?;
    let snapshot_times = read_snapshot_times(&mut tokens, metadata.snapshots)?;
    let graphs = read_graphs(
        &mut tokens,
        metadata.vertices,
        metadata.edges,
        metadata.snapshots,
    )?;
    Ok(Trajectory {
        metadata,
        snapshot_times,
        graphs,
    })
}

fn read_contents(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| format!("cannot open trajectory: {error}"))?;
    let length = file
        .metadata()
        .map_err(|error| format!("cannot stat trajectory: {error}"))?
        .len();
    if length > MAX_FILE_BYTES {
        return Err("trajectory exceeds the file-size limit".into());
    }
    drop(file);
    fs::read_to_string(path).map_err(|error| format!("cannot read trajectory as UTF-8: {error}"))
}

fn read_metadata<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Result<Metadata, String> {
    let (dataset, source_sha256) = read_identity(tokens)?;
    let (vertices, edges, snapshots) = read_counts(tokens)?;
    let constants = read_constants(tokens)?;
    Ok(Metadata {
        dataset,
        source_sha256,
        vertices,
        edges,
        snapshots,
        bin_seconds: constants.bin_seconds,
        warmup_bins: constants.warmup_bins,
        decay_numerator: constants.decay_numerator,
        decay_denominator: constants.decay_denominator,
        score_scale: constants.score_scale,
        weight_offset: constants.weight_offset,
        maximum_score: constants.maximum_score,
    })
}

fn read_identity<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
) -> Result<(String, String), String> {
    expect_value(tokens, MAGIC)?;
    expect_value(tokens, "dataset")?;
    let dataset = take_string(tokens, "dataset")?;
    expect_value(tokens, "source_sha256")?;
    let source_sha256 = take_string(tokens, "source SHA-256")?;
    if source_sha256.len() != 64 || !source_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("source SHA-256 is not 64 hexadecimal digits".into());
    }
    Ok((dataset, source_sha256))
}

fn read_counts<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
) -> Result<(usize, usize, usize), String> {
    let vertices: usize = labeled(tokens, "vertices")?;
    let edges: usize = labeled(tokens, "edges")?;
    let snapshots: usize = labeled(tokens, "snapshots")?;
    validate_counts(vertices, edges, snapshots)?;
    Ok((vertices, edges, snapshots))
}

struct Constants {
    bin_seconds: u64,
    warmup_bins: usize,
    decay_numerator: u64,
    decay_denominator: u64,
    score_scale: u64,
    weight_offset: u64,
    maximum_score: u64,
}

fn read_constants<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Result<Constants, String> {
    let bin_seconds: u64 = labeled(tokens, "bin_seconds")?;
    let warmup_bins: usize = labeled(tokens, "warmup_bins")?;
    let decay_numerator: u64 = labeled(tokens, "decay_numerator")?;
    let decay_denominator: u64 = labeled(tokens, "decay_denominator")?;
    let score_scale: u64 = labeled(tokens, "score_scale")?;
    let weight_offset: u64 = labeled(tokens, "weight_offset")?;
    let maximum_score: u64 = labeled(tokens, "maximum_score")?;
    validate_constants(
        bin_seconds,
        warmup_bins,
        decay_numerator,
        decay_denominator,
        score_scale,
        maximum_score,
        weight_offset,
    )?;
    Ok(Constants {
        bin_seconds,
        warmup_bins,
        decay_numerator,
        decay_denominator,
        score_scale,
        weight_offset,
        maximum_score,
    })
}

fn validate_counts(vertices: usize, edges: usize, snapshots: usize) -> Result<(), String> {
    if vertices == 0 || vertices > MAX_VERTICES {
        return Err("trajectory vertex count is outside the limit".into());
    }
    if edges == 0 || edges > MAX_EDGES {
        return Err("trajectory edge count is outside the limit".into());
    }
    if !(2..=MAX_SNAPSHOTS).contains(&snapshots) {
        return Err("trajectory snapshot count is outside the limit".into());
    }
    let cells = edges
        .checked_mul(snapshots)
        .ok_or_else(|| "trajectory cell count overflowed".to_string())?;
    if cells > MAX_CELLS {
        return Err("trajectory contains too many edge weights".into());
    }
    Ok(())
}

fn validate_constants(
    bin_seconds: u64,
    warmup_bins: usize,
    decay_numerator: u64,
    decay_denominator: u64,
    score_scale: u64,
    maximum_score: u64,
    weight_offset: u64,
) -> Result<(), String> {
    if bin_seconds == 0
        || warmup_bins == 0
        || decay_denominator == 0
        || decay_numerator >= decay_denominator
        || score_scale == 0
        || maximum_score >= weight_offset
    {
        return Err("trajectory preprocessing constants are invalid".into());
    }
    Ok(())
}

fn read_snapshot_times<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    snapshots: usize,
) -> Result<Vec<u64>, String> {
    expect_value(tokens, "snapshot_times")?;
    let snapshot_times = (0..snapshots)
        .map(|_| take_number(tokens, "snapshot time"))
        .collect::<Result<Vec<u64>, _>>()?;
    if !snapshot_times.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err("snapshot times are not strictly increasing".into());
    }
    Ok(snapshot_times)
}

fn read_graphs<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    vertices: usize,
    edges: usize,
    snapshots: usize,
) -> Result<Vec<SparseDistanceMatrix>, String> {
    expect_value(tokens, "edge_weights")?;
    let mut snapshots_triplets = vec![Vec::with_capacity(edges); snapshots];
    let mut previous_edge = None;
    for _ in 0..edges {
        let (u, v) = read_edge_endpoints(tokens, vertices, previous_edge)?;
        previous_edge = Some((u, v));
        read_edge_weights(tokens, &mut snapshots_triplets, u, v)?;
    }
    if tokens.next().is_some() {
        return Err("trajectory has trailing fields".into());
    }
    snapshots_triplets
        .into_iter()
        .map(|triplets| {
            SparseDistanceMatrix::from_triplets(vertices, &triplets)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn read_edge_endpoints<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    vertices: usize,
    previous_edge: Option<(usize, usize)>,
) -> Result<(usize, usize), String> {
    let u: usize = take_number(tokens, "edge endpoint")?;
    let v: usize = take_number(tokens, "edge endpoint")?;
    if u >= v || v >= vertices || previous_edge.is_some_and(|edge| edge >= (u, v)) {
        return Err("trajectory edges are not canonical".into());
    }
    Ok((u, v))
}

fn read_edge_weights<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    snapshots_triplets: &mut [Vec<(usize, usize, f64)>],
    u: usize,
    v: usize,
) -> Result<(), String> {
    for triplets in snapshots_triplets {
        let weight: u64 = take_number(tokens, "edge weight")?;
        if weight >= 1 << 53 {
            return Err("trajectory edge weight is not an exact f64 integer".into());
        }
        triplets.push((u, v, weight as f64));
    }
    Ok(())
}

fn labeled<'a, T: std::str::FromStr>(
    tokens: &mut impl Iterator<Item = &'a str>,
    label: &str,
) -> Result<T, String> {
    expect_value(tokens, label)?;
    take_number(tokens, label)
}

fn expect_value<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    expected: &str,
) -> Result<(), String> {
    match tokens.next() {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(format!("expected {expected}, found {actual}")),
        None => Err(format!("expected {expected}, found end of file")),
    }
}

fn take_string<'a>(
    tokens: &mut impl Iterator<Item = &'a str>,
    label: &str,
) -> Result<String, String> {
    tokens
        .next()
        .map(str::to_owned)
        .ok_or_else(|| format!("missing {label}"))
}

fn take_number<'a, T: std::str::FromStr>(
    tokens: &mut impl Iterator<Item = &'a str>,
    label: &str,
) -> Result<T, String> {
    let value = tokens.next().ok_or_else(|| format!("missing {label}"))?;
    value
        .parse()
        .map_err(|_| format!("invalid {label}: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_a_canonical_trajectory() {
        let path = temporary_path();
        fs::write(
            &path,
            "HOLOSTEM1\n\
             dataset tiny\n\
             source_sha256 0000000000000000000000000000000000000000000000000000000000000000\n\
             vertices 3\n\
             edges 3\n\
             snapshots 2\n\
             bin_seconds 7\n\
             warmup_bins 1\n\
             decay_numerator 1\n\
             decay_denominator 2\n\
             score_scale 4\n\
             weight_offset 100\n\
             maximum_score 8\n\
             snapshot_times 7 14\n\
             edge_weights\n\
             0 1 10 11\n\
             0 2 20 21\n\
             1 2 30 31\n",
        )
        .unwrap();
        let trajectory = read(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(trajectory.metadata.dataset, "tiny");
        assert_eq!(trajectory.graphs.len(), 2);
        assert_eq!(trajectory.graphs[1].get(1, 2), 31.0);
    }

    #[test]
    fn rejects_invalid_header_limits() {
        assert_eq!(
            validate_counts(0, 1, 2).unwrap_err(),
            "trajectory vertex count is outside the limit"
        );
        assert_eq!(
            validate_constants(1, 1, 1, 1, 1, 2, 2).unwrap_err(),
            "trajectory preprocessing constants are invalid"
        );
    }

    fn temporary_path() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "holos-public-{}-{stamp}.holostem",
            std::process::id()
        ))
    }
}
