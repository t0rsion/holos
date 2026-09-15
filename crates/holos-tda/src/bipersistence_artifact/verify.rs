use crate::{
    BipersistenceModule, CircularCoordinateParams, DegreeRipsBifiltration, DegreeRipsParams, Error,
    Result, SparseDistanceMatrix,
};

use super::BipersistenceArtifactLimits;
use super::build::{circular_family_claim, validate_circular_iteration_limit};
use super::model::{
    ArtifactCircularEntry, ArtifactCircularStatus, ArtifactEdge, BipersistenceArtifact,
};

impl BipersistenceArtifact {
    /// Reconstruct the module and compare every stored claim.
    pub fn verify(&self, limits: BipersistenceArtifactLimits) -> Result<()> {
        self.validate_claims(limits)?;
        let module = self.replay_module(limits)?;
        self.require_module(&module)?;
        self.verify_rectangle_ranks(&module)?;
        self.verify_region_ranks(&module)?;
        self.verify_class_atlases(&module)?;
        self.verify_circular_families(&module, limits)?;
        self.verify_digest()?;
        Ok(())
    }

    fn validate_claims(&self, limits: BipersistenceArtifactLimits) -> Result<()> {
        validate_sorted_claims(&self.rectangles, limits.max_rectangles, |left, right| {
            (left.rectangle.lower, left.rectangle.upper)
                < (right.rectangle.lower, right.rectangle.upper)
        })?;
        validate_sorted_claims(&self.regions, limits.max_regions, |left, right| {
            left.region < right.region
        })?;
        validate_sorted_claims(
            &self.class_atlases,
            limits.max_class_atlases,
            |left, right| {
                (left.base_grade, &left.base_class) < (right.base_grade, &right.base_class)
            },
        )?;
        validate_sorted_claims(
            &self.circular_families,
            limits.max_circular_families,
            |left, right| {
                (left.base_grade, &left.base_class) < (right.base_grade, &right.base_class)
            },
        )?;
        for family in &self.circular_families {
            validate_circular_iteration_limit(family.max_iterations, limits)?;
            for entry in &family.entries {
                validate_circular_entry_shape(entry)?;
            }
        }
        Ok(())
    }

    fn replay_module(&self, limits: BipersistenceArtifactLimits) -> Result<BipersistenceModule> {
        let graph = self.source_graph(limits)?;
        let degree_rips = DegreeRipsBifiltration::from_graph_on_grid(
            &graph,
            self.scale_bits
                .iter()
                .copied()
                .map(f64::from_bits)
                .collect(),
            self.minimum_degrees.clone(),
            DegreeRipsParams {
                max_homology_dimension: 1,
                threshold: Some(f64::from_bits(self.threshold_bits)),
                limits: limits.bifiltration,
            },
        )?;
        BipersistenceModule::from_degree_rips(&degree_rips, self.modulus, limits.module)
    }

    fn verify_rectangle_ranks(&self, module: &BipersistenceModule) -> Result<()> {
        for claim in &self.rectangles {
            if module.rectangle_rank(claim.rectangle)? != claim.rank {
                return Err(Error::InvalidInput(
                    "a bipersistence rectangle rank differs from exact replay".into(),
                ));
            }
        }
        Ok(())
    }

    fn verify_region_ranks(&self, module: &BipersistenceModule) -> Result<()> {
        for claim in &self.regions {
            if module.region_rank(&claim.region)? != claim.rank {
                return Err(Error::InvalidInput(
                    "a bipersistence region rank differs from exact replay".into(),
                ));
            }
        }
        Ok(())
    }

    fn verify_class_atlases(&self, module: &BipersistenceModule) -> Result<()> {
        for atlas in &self.class_atlases {
            if module.class_atlas(atlas.base_grade, &atlas.base_class)? != *atlas {
                return Err(Error::InvalidInput(
                    "a bipersistence class atlas differs from exact replay".into(),
                ));
            }
        }
        Ok(())
    }

    fn verify_circular_families(
        &self,
        module: &BipersistenceModule,
        limits: BipersistenceArtifactLimits,
    ) -> Result<()> {
        for family in &self.circular_families {
            let atlas = self
                .class_atlases
                .iter()
                .find(|atlas| {
                    atlas.base_grade == family.base_grade && atlas.base_class == family.base_class
                })
                .ok_or_else(|| {
                    Error::InvalidInput(
                        "a circular family does not name a stored class atlas".into(),
                    )
                })?;
            let expected = circular_family_claim(
                module,
                atlas,
                CircularCoordinateParams {
                    tolerance: f64::from_bits(family.tolerance_bits),
                    max_iterations: family.max_iterations,
                    cohomology: limits.module.cohomology,
                },
                limits,
            )?;
            if expected != *family {
                return Err(Error::InvalidInput(
                    "a circular family differs from exact replay".into(),
                ));
            }
        }
        Ok(())
    }

    fn verify_digest(&self) -> Result<()> {
        if self.compute_digest()? != self.digest {
            return Err(Error::InvalidInput(
                "bipersistence artifact digest differs from its content".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn source_graph(
        &self,
        limits: BipersistenceArtifactLimits,
    ) -> Result<SparseDistanceMatrix> {
        if self.edges.len() > limits.max_source_edges {
            return Err(Error::InvalidInput(
                "bipersistence source edge count exceeds its limit".into(),
            ));
        }
        SparseDistanceMatrix::from_triplets(
            self.vertex_count,
            &self
                .edges
                .iter()
                .map(|edge| (edge.u, edge.v, f64::from_bits(edge.value_bits)))
                .collect::<Vec<_>>(),
        )
    }

    pub(super) fn require_module(&self, module: &BipersistenceModule) -> Result<()> {
        let source = module.degree_rips().source();
        let source_edges = source
            .edges()
            .map(|(u, v, value)| ArtifactEdge {
                u,
                v,
                value_bits: value.to_bits(),
            })
            .collect::<Vec<_>>();
        let scale_bits = module
            .scales()
            .iter()
            .map(|scale| scale.to_bits())
            .collect::<Vec<_>>();
        if source.len() != self.vertex_count
            || source_edges != self.edges
            || module.degree_rips().threshold().to_bits() != self.threshold_bits
            || module.modulus() != self.modulus
            || scale_bits != self.scale_bits
            || module.minimum_degrees() != self.minimum_degrees
            || module.nodes() != self.nodes
            || module.cover_maps() != self.cover_maps
        {
            return Err(Error::InvalidInput(
                "bipersistence module differs from the artifact claim".into(),
            ));
        }
        Ok(())
    }
}

pub(super) fn validate_circular_entry_shape(entry: &ArtifactCircularEntry) -> Result<()> {
    match (entry.extension, &entry.status) {
        (crate::ClassExtensionKind::Unique, ArtifactCircularStatus::NotAttempted) => Err(
            Error::InvalidInput("a unique circular extension has no computation status".into()),
        ),
        (crate::ClassExtensionKind::Unique, ArtifactCircularStatus::Success(bytes))
            if bytes.is_empty() =>
        {
            Err(Error::InvalidInput(
                "a successful circular status has no coordinate".into(),
            ))
        }
        (
            crate::ClassExtensionKind::Ambiguous | crate::ClassExtensionKind::NoExtension,
            ArtifactCircularStatus::NotAttempted,
        ) => Ok(()),
        (crate::ClassExtensionKind::Ambiguous | crate::ClassExtensionKind::NoExtension, _) => Err(
            Error::InvalidInput("a non-unique circular extension has a computation status".into()),
        ),
        (crate::ClassExtensionKind::Unique, _) => Ok(()),
    }
}

fn validate_sorted_claims<T>(
    claims: &[T],
    maximum: usize,
    mut precedes: impl FnMut(&T, &T) -> bool,
) -> Result<()> {
    if claims.len() > maximum || claims.windows(2).any(|pair| !precedes(&pair[0], &pair[1])) {
        return Err(Error::InvalidInput(
            "bipersistence derived claims are not canonical or exceed their limits".into(),
        ));
    }
    Ok(())
}
