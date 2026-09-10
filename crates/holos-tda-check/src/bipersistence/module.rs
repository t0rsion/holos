use std::collections::BTreeMap;

use crate::ProofError;
use crate::circular::verify_single_binding;
use crate::{cohomology::MapTerm, inverse_mod};

use super::claims::*;
use super::grid::degree_table;
use super::linear::{LinearMap, coordinate_vector};
use super::node::{CheckedNode, build_nodes, validate_grid};

pub(crate) struct CheckedModule {
    pub(crate) modulus: u32,
    pub(crate) scales: Vec<f64>,
    pub(crate) minimum_degrees: Vec<usize>,
    pub(crate) nodes: Vec<CheckedNode>,
    pub(crate) maps: Vec<MapClaim>,
    pub(crate) map_positions: BTreeMap<(Grade, Grade), usize>,
}

impl CheckedModule {
    pub(crate) fn build(
        claim: &Claim,
        limits: BipersistenceProofLimits,
    ) -> Result<Self, ProofError> {
        let (scales, minimum_degrees) = validate_grid(claim, limits)?;
        let degrees = degree_table(claim.vertex_count, &claim.edges, &scales);
        let nodes = build_nodes(claim, limits, &scales, &minimum_degrees, &degrees)?;
        let mut module = Self {
            modulus: claim.modulus,
            scales,
            minimum_degrees,
            nodes,
            maps: Vec::new(),
            map_positions: BTreeMap::new(),
        };
        module.add_cover_maps()?;
        module.verify_cover_maps(&claim.cover_maps)?;
        module.check_squares()?;
        Ok(module)
    }

    pub(crate) fn verify_claims(
        &self,
        claim: &Claim,
        limits: BipersistenceProofLimits,
    ) -> Result<(), ProofError> {
        self.verify_rectangles(&claim.rectangles, limits)?;
        self.verify_regions(&claim.rank_regions, limits)?;
        self.verify_atlases(&claim.class_atlases)?;
        self.verify_circular_families(claim, limits)?;
        Ok(())
    }

    fn verify_rectangles(
        &self,
        rectangles: &[RectangleClaim],
        limits: BipersistenceProofLimits,
    ) -> Result<(), ProofError> {
        for rectangle in rectangles {
            self.validate_comparable(rectangle.lower, rectangle.upper)?;
            if self.rectangle_rank(rectangle.lower, rectangle.upper, limits)? != rectangle.rank {
                return Err(ProofError::new(
                    "a bipersistence rectangle rank differs from exact replay",
                ));
            }
        }
        Ok(())
    }

    fn verify_regions(
        &self,
        regions: &[RankRegionClaim],
        limits: BipersistenceProofLimits,
    ) -> Result<(), ProofError> {
        for region in regions {
            if self.region_rank(&region.grades, limits)? != region.rank {
                return Err(ProofError::new(
                    "a bipersistence region rank differs from exact replay",
                ));
            }
        }
        Ok(())
    }

    fn verify_atlases(&self, atlases: &[AtlasClaim]) -> Result<(), ProofError> {
        for atlas in atlases {
            if self.class_atlas(atlas.base_grade, &atlas.base_class)? != *atlas {
                return Err(ProofError::new(
                    "a bipersistence class atlas differs from exact replay",
                ));
            }
        }
        Ok(())
    }

    fn verify_circular_families(
        &self,
        claim: &Claim,
        limits: BipersistenceProofLimits,
    ) -> Result<(), ProofError> {
        for family in &claim.circular_families {
            self.verify_circular_family(family, &claim.class_atlases, claim.vertex_count, limits)?;
        }
        Ok(())
    }

    fn verify_circular_family(
        &self,
        family: &CircularFamilyClaim,
        atlases: &[AtlasClaim],
        vertex_count: usize,
        limits: BipersistenceProofLimits,
    ) -> Result<(), ProofError> {
        let atlas = atlases
            .iter()
            .find(|atlas| {
                atlas.base_grade == family.base_grade && atlas.base_class == family.base_class
            })
            .ok_or_else(|| {
                ProofError::new("a bipersistence circular family has no matching class atlas")
            })?;
        if family.entries.len() != atlas.extensions.len() {
            return Err(ProofError::new(
                "a bipersistence circular family has the wrong entry count",
            ));
        }
        for (entry, extension) in family.entries.iter().zip(&atlas.extensions) {
            self.verify_circular_entry(entry, extension, family, vertex_count, limits)?;
        }
        Ok(())
    }

    fn verify_circular_entry(
        &self,
        entry: &CircularEntryClaim,
        extension: &Extension,
        family: &CircularFamilyClaim,
        vertex_count: usize,
        limits: BipersistenceProofLimits,
    ) -> Result<(), ProofError> {
        let has_coordinate = check_circular_entry_shape(entry, extension)?;
        if !has_coordinate {
            return Ok(());
        }
        let Some(bytes) = &entry.coordinate else {
            return Ok(());
        };
        self.verify_circular_binding(bytes, extension, family, vertex_count, entry.grade, limits)
    }

    fn verify_circular_binding(
        &self,
        bytes: &[u8],
        extension: &Extension,
        family: &CircularFamilyClaim,
        vertex_count: usize,
        grade: Grade,
        limits: BipersistenceProofLimits,
    ) -> Result<(), ProofError> {
        let binding = verify_single_binding(bytes, limits.circular)?;
        let node = self.node(grade)?;
        let expected_class = extension
            .class
            .iter()
            .map(|term| MapTerm {
                target: term.basis,
                coefficient: term.coefficient,
            })
            .collect::<Vec<_>>();
        if binding.vertex_count != vertex_count
            || binding.edges != node.edges
            || binding.modulus != self.modulus
            || binding.scale.to_bits() != 0.0f64.to_bits()
            || binding.tolerance.to_bits() != family.tolerance_bits
            || binding.space != node.space_id
            || !projectively_equal(&binding.class, &expected_class, self.modulus)
        {
            return Err(ProofError::new(
                "a nested circular coordinate differs from its module class",
            ));
        }
        Ok(())
    }

    pub(crate) fn scale_count(&self) -> usize {
        self.scales.len()
    }

    pub(crate) fn density_count(&self) -> usize {
        self.minimum_degrees.len()
    }

    pub(crate) fn node_index(&self, grade: Grade) -> usize {
        grade.scale * self.density_count() + grade.density
    }

    pub(crate) fn node(&self, grade: Grade) -> Result<&CheckedNode, ProofError> {
        self.validate_grade(grade)?;
        Ok(&self.nodes[self.node_index(grade)])
    }

    pub(crate) fn validate_grade(&self, grade: Grade) -> Result<(), ProofError> {
        if grade.scale >= self.scale_count() || grade.density >= self.density_count() {
            return Err(ProofError::new(
                "bipersistence grade is outside the finite grid",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_comparable(&self, lower: Grade, upper: Grade) -> Result<(), ProofError> {
        self.validate_grade(lower)?;
        self.validate_grade(upper)?;
        if !lower.precedes(upper) {
            return Err(ProofError::new("bipersistence grades are not comparable"));
        }
        Ok(())
    }

    fn add_cover_maps(&mut self) -> Result<(), ProofError> {
        for scale in 0..self.scale_count() {
            for density in 0..self.density_count() {
                self.add_scale_map(scale, density)?;
                self.add_density_map(scale, density)?;
            }
        }
        Ok(())
    }

    fn add_scale_map(&mut self, scale: usize, density: usize) -> Result<(), ProofError> {
        if scale + 1 < self.scale_count() {
            self.push_map(
                Grade { scale, density },
                Grade {
                    scale: scale + 1,
                    density,
                },
            )?;
        }
        Ok(())
    }

    fn add_density_map(&mut self, scale: usize, density: usize) -> Result<(), ProofError> {
        if density + 1 < self.density_count() {
            self.push_map(
                Grade { scale, density },
                Grade {
                    scale,
                    density: density + 1,
                },
            )?;
        }
        Ok(())
    }

    fn verify_cover_maps(&self, maps: &[MapClaim]) -> Result<(), ProofError> {
        if self.maps != maps {
            return Err(ProofError::new(
                "bipersistence cover maps differ from exact restriction replay",
            ));
        }
        Ok(())
    }

    pub(crate) fn push_map(&mut self, lower: Grade, upper: Grade) -> Result<(), ProofError> {
        let lower_node = self.node(lower)?;
        let upper_node = self.node(upper)?;
        if lower_node
            .edges
            .iter()
            .any(|edge| upper_node.edges.binary_search(edge).is_err())
        {
            return Err(ProofError::new(
                "a degree-Rips cover is not a simplicial inclusion",
            ));
        }
        let (columns, rank) = upper_node
            .space
            .restriction_to(&lower_node.space, self.modulus)?;
        let map = MapClaim {
            lower,
            upper,
            source_space: upper_node.space_id,
            target_space: lower_node.space_id,
            rank,
            columns: columns
                .into_iter()
                .enumerate()
                .map(|(source, image)| Column {
                    source,
                    image: image
                        .into_iter()
                        .map(|term| Term {
                            basis: term.target,
                            coefficient: term.coefficient,
                        })
                        .collect(),
                })
                .collect(),
        };
        self.map_positions.insert((lower, upper), self.maps.len());
        self.maps.push(map);
        Ok(())
    }

    pub(crate) fn cover(&self, lower: Grade, upper: Grade) -> Result<&MapClaim, ProofError> {
        self.map_positions
            .get(&(lower, upper))
            .map(|position| &self.maps[*position])
            .ok_or_else(|| ProofError::new("bipersistence grades do not form a cover"))
    }

    pub(crate) fn linear(&self, map: &MapClaim) -> Result<LinearMap, ProofError> {
        let source_rank = self.node(map.upper)?.space.rank();
        let target_rank = self.node(map.lower)?.space.rank();
        if map.columns.len() != source_rank {
            return Err(ProofError::new("bipersistence map source rank is invalid"));
        }
        let columns = map
            .columns
            .iter()
            .enumerate()
            .map(|(source, column)| {
                if column.source != source {
                    return Err(ProofError::new(
                        "bipersistence map columns are not canonical",
                    ));
                }
                coordinate_vector(
                    &column.image,
                    target_rank,
                    self.modulus,
                    "bipersistence map image is not canonical",
                )
            })
            .collect::<Result<Vec<_>, ProofError>>()?;
        Ok(LinearMap {
            source_rank,
            target_rank,
            columns,
        })
    }

    pub(crate) fn map(&self, lower: Grade, upper: Grade) -> Result<LinearMap, ProofError> {
        self.validate_comparable(lower, upper)?;
        let (current, composed) = self.compose_scale_path(lower, upper)?;
        self.compose_density_path(lower, current, composed)
    }

    fn compose_scale_path(
        &self,
        lower: Grade,
        upper: Grade,
    ) -> Result<(Grade, LinearMap), ProofError> {
        let mut composed = LinearMap::identity(self.node(upper)?.space.rank());
        let mut current = upper;
        while current.scale > lower.scale {
            let next = Grade {
                scale: current.scale - 1,
                density: current.density,
            };
            composed = LinearMap::compose(
                &self.linear(self.cover(next, current)?)?,
                &composed,
                self.modulus,
            )?;
            current = next;
        }
        Ok((current, composed))
    }

    fn compose_density_path(
        &self,
        lower: Grade,
        mut current: Grade,
        mut composed: LinearMap,
    ) -> Result<LinearMap, ProofError> {
        while current.density > lower.density {
            let next = Grade {
                scale: current.scale,
                density: current.density - 1,
            };
            composed = LinearMap::compose(
                &self.linear(self.cover(next, current)?)?,
                &composed,
                self.modulus,
            )?;
            current = next;
        }
        Ok(composed)
    }

    pub(crate) fn check_squares(&self) -> Result<(), ProofError> {
        for scale in 0..self.scale_count().saturating_sub(1) {
            for density in 0..self.density_count().saturating_sub(1) {
                self.check_square(scale, density)?;
            }
        }
        Ok(())
    }

    fn check_square(&self, scale: usize, density: usize) -> Result<(), ProofError> {
        let lower = Grade { scale, density };
        let upper = Grade {
            scale: scale + 1,
            density: density + 1,
        };
        let left = self.compose_square_side(
            lower,
            Grade {
                scale,
                density: density + 1,
            },
            upper,
        )?;
        let right = self.compose_square_side(
            lower,
            Grade {
                scale: scale + 1,
                density,
            },
            upper,
        )?;
        if left != right {
            return Err(ProofError::new(
                "a bipersistence module square does not commute",
            ));
        }
        Ok(())
    }

    fn compose_square_side(
        &self,
        lower: Grade,
        middle: Grade,
        upper: Grade,
    ) -> Result<LinearMap, ProofError> {
        LinearMap::compose(
            &self.linear(self.cover(lower, middle)?)?,
            &self.linear(self.cover(middle, upper)?)?,
            self.modulus,
        )
    }
}

fn check_circular_entry_shape(
    entry: &CircularEntryClaim,
    extension: &Extension,
) -> Result<bool, ProofError> {
    if entry.grade != extension.grade || entry.extension != extension.kind {
        return Err(ProofError::new(
            "a circular-family entry differs from its class extension",
        ));
    }
    let needs_coordinate = extension.kind == ExtensionKind::Unique;
    if entry.coordinate.is_some() != needs_coordinate {
        return Err(ProofError::new(
            "a circular-family coordinate differs from its extension kind",
        ));
    }
    Ok(entry.coordinate.is_some())
}

fn projectively_equal(left: &[MapTerm], right: &[MapTerm], modulus: u32) -> bool {
    fn normalized(terms: &[MapTerm], modulus: u32) -> Option<Vec<MapTerm>> {
        let first = terms.first()?;
        let multiplier = inverse_mod(u64::from(first.coefficient), u64::from(modulus)) as u32;
        Some(
            terms
                .iter()
                .map(|term| MapTerm {
                    target: term.target,
                    coefficient: ((u64::from(term.coefficient) * u64::from(multiplier))
                        % u64::from(modulus)) as u32,
                })
                .collect(),
        )
    }
    normalized(left, modulus) == normalized(right, modulus)
}
