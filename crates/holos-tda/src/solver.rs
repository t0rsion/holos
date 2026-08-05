//! Entry point for a persistence computation: validate parameters, choose the
//! coefficient field, and run the reduction engine. The algorithm lives in
//! [`crate::reduce`], and the field arithmetic lives in [`crate::field`].

use crate::distances::Distances;
use crate::field::{is_prime, Fp, MODULUS_LIMIT, Z2};
use crate::reduce::Engine;
use crate::{Diagram, Error, Result, RipsParams};

pub(crate) fn compute<D: Distances + Sync>(dist: &D, params: &RipsParams) -> Result<Diagram> {
    if let Some(t) = params.threshold {
        if t.is_nan() || t < 0.0 {
            return Err(Error::InvalidInput(format!(
                "threshold must be non-negative, got {t}"
            )));
        }
    }
    let p = params.modulus as u64;
    if !is_prime(p) || p >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {p}"
        )));
    }
    if p == 2 {
        compute_impl(dist, Z2, params)
    } else {
        compute_impl(dist, Fp::new(p), params)
    }
}

fn compute_impl<C, D>(dist: &D, ops: C, params: &RipsParams) -> Result<Diagram>
where
    C: crate::field::Coeffs + Sync,
    D: Distances + Sync,
{
    let mut diagram = Diagram::default();
    if dist.len() == 0 {
        return Ok(diagram);
    }
    let engine = Engine::new(dist, params, ops)?;
    engine.run(&mut diagram);
    diagram.canonicalize();
    Ok(diagram)
}

#[cfg(test)]
mod tests {
    use crate::{rips_persistence, DistanceMatrix, RipsParams};

    #[test]
    fn triangle_unit_distances() {
        let dist = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap();
        let d = rips_persistence(&dist, &RipsParams::new(1)).unwrap();
        let h0: Vec<_> = d.in_dim(0).collect();
        assert_eq!(h0.len(), 3);
        assert_eq!(h0.iter().filter(|b| b.is_essential()).count(), 1);
        assert_eq!(h0.iter().filter(|b| b.death == 1.0).count(), 2);
        // The loop forms and fills at the same scale: zero persistence.
        assert_eq!(d.in_dim(1).count(), 0);
    }

    #[test]
    fn square_has_one_h1_bar() {
        let s = 2.0f64.sqrt();
        // Vertices of a unit square: sides 1, diagonals sqrt(2).
        let dist = DistanceMatrix::from_condensed(vec![1.0, s, 1.0, 1.0, s, 1.0]).unwrap();
        let d = rips_persistence(&dist, &RipsParams::new(1).with_threshold(2.0)).unwrap();
        assert_eq!(d.in_dim(0).filter(|b| b.is_essential()).count(), 1);
        assert_eq!(d.in_dim(0).count(), 4);
        let h1: Vec<_> = d.in_dim(1).collect();
        assert_eq!(h1.len(), 1);
        assert_eq!(h1[0].birth, 1.0);
        assert_eq!(h1[0].death, s);
    }

    #[test]
    fn two_components() {
        let inf = f64::INFINITY;
        let dist = DistanceMatrix::from_condensed(vec![1.0, inf, inf, inf, inf, 1.0]).unwrap();
        let d = rips_persistence(&dist, &RipsParams::new(1).with_threshold(10.0)).unwrap();
        assert_eq!(d.in_dim(0).filter(|b| b.is_essential()).count(), 2);
    }

    #[test]
    fn square_diagram_is_field_independent() {
        // A torsion-free space: identical diagrams over every field.
        let s = 2.0f64.sqrt();
        let dist = DistanceMatrix::from_condensed(vec![1.0, s, 1.0, 1.0, s, 1.0]).unwrap();
        let base = rips_persistence(&dist, &RipsParams::new(1)).unwrap();
        for p in [3u32, 5, 7, 13] {
            let mut params = RipsParams::new(1);
            params.modulus = p;
            let d = rips_persistence(&dist, &params).unwrap();
            assert_eq!(d.bars, base.bars, "diagram differs at p = {p}");
        }
    }

    #[test]
    fn invalid_modulus_is_rejected() {
        let dist = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
        for bad in [0u32, 1, 4, 6, 9, 32768, 32770] {
            let mut params = RipsParams::new(1);
            params.modulus = bad;
            assert!(
                rips_persistence(&dist, &params).is_err(),
                "modulus {bad} must be rejected"
            );
        }
    }
}
