use std::time::Instant;

use holos_tda::{
    CertificateLimits, ExplicitReductionCertificate, FilteredSimplex, FilteredSimplicialComplex,
    ScalarGrade,
};
use holos_tda_check::{ProofLimits, verify_explicit_persistence};

use crate::{Measurement, median};

pub(crate) fn measure(repetitions: usize) -> Result<Measurement, String> {
    let complex = cycle()?;
    let limits = CertificateLimits::default();
    let mut producer_times = Vec::with_capacity(repetitions);
    let mut checker_times = Vec::with_capacity(repetitions);
    let mut final_bytes = Vec::new();
    let mut bars = 0;
    for _ in 0..repetitions {
        let started = Instant::now();
        let certificate = ExplicitReductionCertificate::build(&complex, 1, 3, limits)
            .map_err(|error| error.to_string())?;
        let bytes = certificate
            .encode(limits)
            .map_err(|error| error.to_string())?;
        producer_times.push(started.elapsed());
        let started = Instant::now();
        let checked = verify_explicit_persistence(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        checker_times.push(started.elapsed());
        bars = checked.bars.len();
        final_bytes = bytes;
    }
    Ok(Measurement {
        family: "explicit",
        case: "nonflag-cycle",
        producer: median(producer_times),
        checker: median(checker_times),
        artifact_bytes: final_bytes.len(),
        work: format!("simplices=8 bars={bars} checker=independent"),
    })
}

fn cycle() -> Result<FilteredSimplicialComplex<ScalarGrade>, String> {
    let zero = ScalarGrade::new(0.0).map_err(|error| error.to_string())?;
    let one = ScalarGrade::new(1.0).map_err(|error| error.to_string())?;
    let vertices = (0..4)
        .map(|vertex| FilteredSimplex::new(vec![vertex], zero))
        .collect();
    let edges = [[0, 1], [1, 2], [2, 3], [0, 3]]
        .into_iter()
        .map(|edge| FilteredSimplex::new(edge.to_vec(), one))
        .collect();
    FilteredSimplicialComplex::new((0..4).collect(), vec![vertices, edges, Vec::new()])
        .map_err(|error| error.to_string())
}
