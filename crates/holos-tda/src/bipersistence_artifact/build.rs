use crate::{
    BipersistenceModule, BipersistenceRectangle, BipersistenceRegion, CircularCoordinateArtifact,
    CircularCoordinateParams, CohomologyClassAtlas, Error, Result,
};

use super::model::{ArtifactCircularEntry, ArtifactCircularFamily};
use super::{BipersistenceArtifact, BipersistenceArtifactLimits};

impl BipersistenceArtifact {
    /// Build an artifact and the exact finite module it records.
    pub fn build(
        degree_rips: &crate::DegreeRipsBifiltration,
        modulus: u32,
        limits: BipersistenceArtifactLimits,
    ) -> Result<(Self, BipersistenceModule)> {
        let module = BipersistenceModule::from_degree_rips(degree_rips, modulus, limits.module)?;
        let edges = degree_rips
            .source()
            .edges()
            .map(|(u, v, value)| super::model::ArtifactEdge {
                u,
                v,
                value_bits: value.to_bits(),
            })
            .collect::<Vec<_>>();
        if edges.len() > limits.max_source_edges {
            return Err(Error::InvalidInput(format!(
                "bipersistence source edge count exceeds the limit {}",
                limits.max_source_edges
            )));
        }
        let mut artifact = Self {
            vertex_count: degree_rips.source().len(),
            edges,
            threshold_bits: degree_rips.threshold().to_bits(),
            modulus,
            scale_bits: module
                .scales()
                .iter()
                .map(|scale| scale.to_bits())
                .collect(),
            minimum_degrees: module.minimum_degrees().to_vec(),
            nodes: module.nodes().to_vec(),
            cover_maps: module.cover_maps().to_vec(),
            rectangles: Vec::new(),
            regions: Vec::new(),
            class_atlases: Vec::new(),
            circular_families: Vec::new(),
            digest: [0; 32],
        };
        artifact.digest = artifact.compute_digest()?;
        Ok((artifact, module))
    }

    /// Add or replace one exact generalized rectangle-rank claim.
    pub fn record_rectangle(
        &mut self,
        module: &BipersistenceModule,
        rectangle: BipersistenceRectangle,
        limits: BipersistenceArtifactLimits,
    ) -> Result<()> {
        self.require_module(module)?;
        let claim = super::BipersistenceRectangleClaim {
            rectangle,
            rank: module.rectangle_rank(rectangle)?,
        };
        let position = self
            .rectangles
            .binary_search_by_key(&(rectangle.lower, rectangle.upper), |item| {
                (item.rectangle.lower, item.rectangle.upper)
            });
        check_insert_limit(
            self.rectangles.len(),
            limits.max_rectangles,
            position.is_ok(),
            "bipersistence rectangle",
        )?;
        match position {
            Ok(position) => self.rectangles[position] = claim,
            Err(position) => self.rectangles.insert(position, claim),
        }
        self.digest = self.compute_digest()?;
        Ok(())
    }

    /// Add or replace one exact generalized connected-region rank claim.
    pub fn record_region(
        &mut self,
        module: &BipersistenceModule,
        region: BipersistenceRegion,
        limits: BipersistenceArtifactLimits,
    ) -> Result<()> {
        self.require_module(module)?;
        let claim = super::BipersistenceRegionClaim {
            rank: module.region_rank(&region)?,
            region,
        };
        let position = self
            .regions
            .binary_search_by(|item| item.region.cmp(&claim.region));
        check_insert_limit(
            self.regions.len(),
            limits.max_regions,
            position.is_ok(),
            "bipersistence region",
        )?;
        match position {
            Ok(position) => self.regions[position] = claim,
            Err(position) => self.regions.insert(position, claim),
        }
        self.digest = self.compute_digest()?;
        Ok(())
    }

    /// Add or replace one exact class-extension atlas.
    pub fn record_class_atlas(
        &mut self,
        module: &BipersistenceModule,
        atlas: &CohomologyClassAtlas,
        limits: BipersistenceArtifactLimits,
    ) -> Result<()> {
        self.require_module(module)?;
        let rebuilt = module.class_atlas(atlas.base_grade, &atlas.base_class)?;
        if rebuilt != *atlas {
            return Err(Error::InvalidInput(
                "the class atlas differs from exact bipersistence replay".into(),
            ));
        }
        let position = self.class_atlases.binary_search_by(|item| {
            (item.base_grade, &item.base_class).cmp(&(atlas.base_grade, &atlas.base_class))
        });
        check_insert_limit(
            self.class_atlases.len(),
            limits.max_class_atlases,
            position.is_ok(),
            "bipersistence class-atlas",
        )?;
        match position {
            Ok(position) => self.class_atlases[position] = atlas.clone(),
            Err(position) => self.class_atlases.insert(position, atlas.clone()),
        }
        self.digest = self.compute_digest()?;
        Ok(())
    }

    /// Add or replace checked circular coordinates for one stored class atlas.
    ///
    /// A unique extension carries one nested `HOLOSCC` proof. An ambiguous or
    /// absent extension carries no coordinate.
    pub fn record_circular_family(
        &mut self,
        module: &BipersistenceModule,
        atlas: &CohomologyClassAtlas,
        params: CircularCoordinateParams,
        limits: BipersistenceArtifactLimits,
    ) -> Result<()> {
        self.require_module(module)?;
        if !self.class_atlases.iter().any(|stored| stored == atlas) {
            return Err(Error::InvalidInput(
                "record the class atlas before its circular family".into(),
            ));
        }
        let claim = circular_family_claim(module, atlas, params, limits)?;
        let position = self.circular_families.binary_search_by(|item| {
            (item.base_grade, &item.base_class).cmp(&(claim.base_grade, &claim.base_class))
        });
        check_insert_limit(
            self.circular_families.len(),
            limits.max_circular_families,
            position.is_ok(),
            "bipersistence circular-family",
        )?;
        match position {
            Ok(position) => self.circular_families[position] = claim,
            Err(position) => self.circular_families.insert(position, claim),
        }
        self.digest = self.compute_digest()?;
        Ok(())
    }
}

fn check_insert_limit(count: usize, maximum: usize, replacing: bool, name: &str) -> Result<()> {
    if count > maximum || (!replacing && count == maximum) {
        return Err(Error::InvalidInput(format!(
            "{name} count exceeds the limit {maximum}"
        )));
    }
    Ok(())
}

pub(super) fn circular_family_claim(
    module: &BipersistenceModule,
    atlas: &CohomologyClassAtlas,
    params: CircularCoordinateParams,
    limits: BipersistenceArtifactLimits,
) -> Result<ArtifactCircularFamily> {
    let family = module.circular_coordinate_family(atlas, params)?;
    let entries = family
        .entries
        .into_iter()
        .map(|entry| {
            let coordinate = entry
                .coordinate
                .as_ref()
                .map(|coordinate| {
                    CircularCoordinateArtifact::from_coordinate(
                        module.h1_graph(entry.grade)?,
                        coordinate,
                    )?
                    .encode()
                    .map_err(|error| Error::InvalidInput(error.to_string()))
                })
                .transpose()?;
            if coordinate
                .as_ref()
                .is_some_and(|bytes| bytes.len() > limits.max_coordinate_bytes)
            {
                return Err(Error::InvalidInput(format!(
                    "nested circular coordinate exceeds the limit {}",
                    limits.max_coordinate_bytes
                )));
            }
            Ok(ArtifactCircularEntry {
                grade: entry.grade,
                extension: entry.extension,
                coordinate,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ArtifactCircularFamily {
        base_grade: atlas.base_grade,
        base_class: atlas.base_class.clone(),
        tolerance_bits: params.tolerance.to_bits(),
        max_iterations: params.max_iterations,
        entries,
    })
}
