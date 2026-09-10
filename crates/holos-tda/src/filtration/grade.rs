use std::fmt;

/// Failure while constructing an explicit filtered complex or grade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiltrationError {
    message: String,
}

impl FiltrationError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated filtration rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FiltrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "filtered complex: {}", self.message)
    }
}

impl std::error::Error for FiltrationError {}

/// A grade with a decidable filtration partial order.
pub trait FiltrationGrade: Clone + Eq {
    /// Return true when `self` is no later than `other` in the filtration.
    fn precedes(&self, other: &Self) -> bool;
}

/// A filtration grade with one canonical total order.
pub trait LinearFiltrationGrade: FiltrationGrade + Ord {}

/// A finite, non-negative scalar filtration grade with a canonical total order.
///
/// The value is stored as canonical IEEE 754 bits. Negative zero is stored as
/// positive zero, so equality and ordering agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScalarGrade(u64);

impl ScalarGrade {
    /// Construct a checked scalar grade.
    pub fn new(value: f64) -> Result<Self, FiltrationError> {
        if !value.is_finite() || value < 0.0 {
            return Err(FiltrationError::new(
                "a scalar grade must be finite and non-negative",
            ));
        }
        Ok(Self(if value == 0.0 {
            0.0f64.to_bits()
        } else {
            value.to_bits()
        }))
    }

    /// The scalar value.
    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// Canonical IEEE 754 bits used by proof formats.
    pub fn bits(self) -> u64 {
        self.0
    }
}

impl FiltrationGrade for ScalarGrade {
    fn precedes(&self, other: &Self) -> bool {
        self <= other
    }
}

impl LinearFiltrationGrade for ScalarGrade {}

/// A coordinatewise filtration grade with `N` parameters.
///
/// Grades use the product partial order. Incomparable grades have no total
/// order. Scalar persistence requires a [`ScalarProjection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProductGrade<const N: usize> {
    coordinates: [ScalarGrade; N],
}

impl<const N: usize> ProductGrade<N> {
    /// Construct a checked product grade from scalar coordinates.
    pub fn new(coordinates: [f64; N]) -> Result<Self, FiltrationError> {
        if N == 0 {
            return Err(FiltrationError::new(
                "a product grade requires at least one coordinate",
            ));
        }
        let mut checked = [ScalarGrade(0); N];
        for (index, value) in coordinates.into_iter().enumerate() {
            checked[index] = ScalarGrade::new(value)?;
        }
        Ok(Self {
            coordinates: checked,
        })
    }

    /// Scalar coordinates in parameter order.
    pub fn coordinates(&self) -> &[ScalarGrade; N] {
        &self.coordinates
    }
}

impl<const N: usize> FiltrationGrade for ProductGrade<N> {
    fn precedes(&self, other: &Self) -> bool {
        self.coordinates
            .iter()
            .zip(other.coordinates.iter())
            .all(|(left, right)| left.precedes(right))
    }
}

/// A declared monotone map from a grade into a scalar filtration.
pub trait ScalarProjection<G: FiltrationGrade> {
    /// Project one grade into the scalar filtration.
    fn project(&self, grade: &G) -> Result<ScalarGrade, FiltrationError>;
}

/// Projection onto one coordinate of a product grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinateProjection {
    coordinate: usize,
}

impl CoordinateProjection {
    /// Select a zero-based coordinate.
    pub fn new(coordinate: usize) -> Self {
        Self { coordinate }
    }
}

impl<const N: usize> ScalarProjection<ProductGrade<N>> for CoordinateProjection {
    fn project(&self, grade: &ProductGrade<N>) -> Result<ScalarGrade, FiltrationError> {
        grade
            .coordinates
            .get(self.coordinate)
            .copied()
            .ok_or_else(|| {
                FiltrationError::new(format!(
                    "coordinate {} is outside a {N}-parameter grade",
                    self.coordinate
                ))
            })
    }
}
