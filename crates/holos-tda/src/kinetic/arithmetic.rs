use num_rational::BigRational;
use num_traits::ToPrimitive;

use crate::{Error, Result};

use super::model::{KineticEvent, KineticEventKind};

pub(super) fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

pub(super) fn midpoint(left: &BigRational, right: &BigRational) -> BigRational {
    (left + right) / BigRational::from_integer(2.into())
}

pub(super) fn public_event(
    time: &BigRational,
    kinds: Vec<KineticEventKind>,
) -> Result<KineticEvent> {
    let approximation = time
        .to_f64()
        .ok_or_else(|| Error::InvalidInput("kinetic event does not fit f64".into()))?;
    let mut lower = approximation;
    while rational(lower) > *time {
        lower = next_down(lower);
    }
    let mut upper = approximation;
    while rational(upper) < *time {
        upper = next_up(upper);
    }
    Ok(KineticEvent {
        time: approximation,
        lower,
        upper,
        kinds,
    })
}

pub(super) fn next_up(value: f64) -> f64 {
    if value == f64::INFINITY {
        return value;
    }
    if value == -0.0 {
        return f64::from_bits(1);
    }
    let bits = value.to_bits();
    f64::from_bits(if value >= 0.0 { bits + 1 } else { bits - 1 })
}

pub(super) fn next_down(value: f64) -> f64 {
    if value == f64::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return -f64::from_bits(1);
    }
    let bits = value.to_bits();
    f64::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}
