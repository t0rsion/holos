//! Construction and accessors for finite bipersistence modules.

use std::collections::BTreeMap;

use crate::bifiltration::DegreeRipsBifiltration;
use crate::cohomology::{CohomologySpace, cohomology_restriction, cohomology_space};
use crate::{Error, Result, SparseDistanceMatrix};

use super::linear::{LinearMap, linear_from_restriction, public_terms, rank};
use super::{
    Bigrade, BipersistenceLimits, BipersistenceMap, BipersistenceMapColumn, BipersistenceNode,
};

/// Exact finite `H¹` module of a degree-Rips bifiltration.
///
/// Cover maps are cohomology restrictions. Their ranks equal the ranks of the
/// dual homology inclusion maps over the same field.
#[derive(Debug, Clone)]
pub struct BipersistenceModule {
    pub(super) degree_rips: DegreeRipsBifiltration,
    pub(super) modulus: u32,
    pub(super) scales: Vec<f64>,
    pub(super) minimum_degrees: Vec<usize>,
    pub(super) nodes: Vec<BipersistenceNode>,
    pub(super) cover_maps: Vec<BipersistenceMap>,
    pub(super) cover_positions: BTreeMap<(Bigrade, Bigrade), usize>,
    pub(super) graphs: Vec<SparseDistanceMatrix>,
    pub(super) spaces: Vec<CohomologySpace>,
    pub(super) limits: BipersistenceLimits,
}

impl BipersistenceModule {
    /// Construct the complete finite `H¹` module on a degree-Rips grid.
    ///
    /// Construction checks every cover restriction and every commutative
    /// square. The degree-Rips input must include homology dimension one.
    pub fn from_degree_rips(
        degree_rips: &DegreeRipsBifiltration,
        modulus: u32,
        limits: BipersistenceLimits,
    ) -> Result<Self> {
        validate_h1_dimension(degree_rips)?;
        let bifiltration = degree_rips.bifiltration();
        let scales = bifiltration.scales().collect::<Vec<_>>();
        let minimum_degrees = bifiltration.minimum_degrees().to_vec();
        let (node_count, cover_count) = grid_sizes(&scales, &minimum_degrees, limits)?;
        let (nodes, graphs, spaces) = build_nodes(
            bifiltration,
            &scales,
            &minimum_degrees,
            modulus,
            limits,
            node_count,
        )?;

        let mut module = Self {
            degree_rips: degree_rips.clone(),
            modulus,
            scales,
            minimum_degrees,
            nodes,
            cover_maps: Vec::with_capacity(cover_count),
            cover_positions: BTreeMap::new(),
            graphs,
            spaces,
            limits,
        };
        module.populate_covers()?;
        module.validate_map_terms()?;
        module.check_squares()?;
        Ok(module)
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Scale values in strict ascending order.
    pub fn scales(&self) -> &[f64] {
        &self.scales
    }

    /// Minimum degrees in strict descending order.
    pub fn minimum_degrees(&self) -> &[usize] {
        &self.minimum_degrees
    }

    /// Nodes in scale-major, then density-major order.
    pub fn nodes(&self) -> &[BipersistenceNode] {
        &self.nodes
    }

    /// Checked horizontal and vertical cover maps.
    pub fn cover_maps(&self) -> &[BipersistenceMap] {
        &self.cover_maps
    }

    /// Source degree-Rips bifiltration.
    pub fn degree_rips(&self) -> &DegreeRipsBifiltration {
        &self.degree_rips
    }

    /// Return one module node.
    pub fn node(&self, grade: Bigrade) -> Result<&BipersistenceNode> {
        self.validate_grade(grade)?;
        Ok(&self.nodes[self.node_index(grade)])
    }

    /// Return the zero-weight graph used for `H¹` at one grid node.
    ///
    /// The graph retains inactive vertices as isolated labels. It represents
    /// positive-dimensional flag cohomology, not `H⁰` of the degree-Rips
    /// slice.
    pub fn h1_graph(&self, grade: Bigrade) -> Result<&SparseDistanceMatrix> {
        self.validate_grade(grade)?;
        Ok(&self.graphs[self.node_index(grade)])
    }

    /// Return the canonical `H¹` space at one grid node.
    pub fn cohomology_space(&self, grade: Bigrade) -> Result<&CohomologySpace> {
        self.validate_grade(grade)?;
        Ok(&self.spaces[self.node_index(grade)])
    }

    /// Return one checked cover map.
    pub fn cover_map(&self, lower: Bigrade, upper: Bigrade) -> Result<&BipersistenceMap> {
        let position = self.cover_positions.get(&(lower, upper)).ok_or_else(|| {
            Error::InvalidInput("the requested grades do not form a grid cover".into())
        })?;
        Ok(&self.cover_maps[*position])
    }

    pub(super) fn scale_count(&self) -> usize {
        self.scales.len()
    }

    pub(super) fn density_count(&self) -> usize {
        self.minimum_degrees.len()
    }

    pub(super) fn node_index(&self, grade: Bigrade) -> usize {
        grade.scale() * self.density_count() + grade.density()
    }

    pub(super) fn validate_grade(&self, grade: Bigrade) -> Result<()> {
        if grade.scale() >= self.scale_count() || grade.density() >= self.density_count() {
            return Err(Error::InvalidInput(
                "bipersistence grade is outside the finite parameter grid".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn validate_comparable(&self, lower: Bigrade, upper: Bigrade) -> Result<()> {
        self.validate_grade(lower)?;
        self.validate_grade(upper)?;
        if !lower.precedes(upper) {
            return Err(Error::InvalidInput(
                "bipersistence map grades are not comparable".into(),
            ));
        }
        Ok(())
    }

    fn push_cover(&mut self, lower: Bigrade, upper: Bigrade) -> Result<()> {
        let lower_position = self.node_index(lower);
        let upper_position = self.node_index(upper);
        let restriction = cohomology_restriction(
            &self.graphs[upper_position],
            &self.spaces[upper_position],
            &self.graphs[lower_position],
            &self.spaces[lower_position],
        )?;
        let linear = linear_from_restriction(
            &restriction,
            &self.spaces[upper_position],
            &self.spaces[lower_position],
        )?;
        let map = self.public_map(lower, upper, &linear);
        let position = self.cover_maps.len();
        self.cover_positions.insert((lower, upper), position);
        self.cover_maps.push(map);
        Ok(())
    }

    pub(super) fn cover_linear(&self, lower: Bigrade, upper: Bigrade) -> Result<LinearMap> {
        self.linear_from_public(self.cover_map(lower, upper)?)
    }

    pub(super) fn linear_from_public(&self, map: &BipersistenceMap) -> Result<LinearMap> {
        let source_rank = self.node(map.upper_grade)?.rank;
        let target_rank = self.node(map.lower_grade)?.rank;
        if map.columns.len() != source_rank {
            return Err(Error::InvalidInput(
                "bipersistence map has the wrong source rank".into(),
            ));
        }
        let mut columns = Vec::with_capacity(source_rank);
        for (source, column) in map.columns.iter().enumerate() {
            if column.source_basis_index != source {
                return Err(Error::InvalidInput(
                    "bipersistence map columns are not canonical".into(),
                ));
            }
            columns.push(super::linear::coordinate_vector(
                &column.image,
                target_rank,
                self.modulus,
                "bipersistence map image is not canonical",
            )?);
        }
        Ok(LinearMap {
            source_rank,
            target_rank,
            columns,
        })
    }

    pub(super) fn public_map(
        &self,
        lower: Bigrade,
        upper: Bigrade,
        linear: &LinearMap,
    ) -> BipersistenceMap {
        BipersistenceMap {
            lower_grade: lower,
            upper_grade: upper,
            source_space: self.nodes[self.node_index(upper)].space,
            target_space: self.nodes[self.node_index(lower)].space,
            rank: rank(linear.columns.clone(), linear.target_rank, self.modulus),
            columns: linear
                .columns
                .iter()
                .enumerate()
                .map(|(source_basis_index, image)| BipersistenceMapColumn {
                    source_basis_index,
                    image: public_terms(image),
                })
                .collect(),
        }
    }

    fn check_squares(&self) -> Result<()> {
        for scale in 0..self.scale_count().saturating_sub(1) {
            for density in 0..self.density_count().saturating_sub(1) {
                let lower = Bigrade::new(scale, density);
                let upper = Bigrade::new(scale + 1, density + 1);
                let scale_first = LinearMap::compose(
                    &self.cover_linear(lower, Bigrade::new(scale, density + 1))?,
                    &self.cover_linear(Bigrade::new(scale, density + 1), upper)?,
                    self.modulus,
                )?;
                let density_first = LinearMap::compose(
                    &self.cover_linear(lower, Bigrade::new(scale + 1, density))?,
                    &self.cover_linear(Bigrade::new(scale + 1, density), upper)?,
                    self.modulus,
                )?;
                if scale_first != density_first {
                    return Err(Error::InvalidInput(format!(
                        "bipersistence square at ({scale}, {density}) does not commute"
                    )));
                }
            }
        }
        Ok(())
    }
}

fn validate_h1_dimension(degree_rips: &DegreeRipsBifiltration) -> Result<()> {
    if degree_rips.max_homology_dimension() < 1 {
        return Err(Error::InvalidInput(
            "an H1 bipersistence module needs degree-Rips dimension one".into(),
        ));
    }
    Ok(())
}

fn grid_sizes(
    scales: &[f64],
    minimum_degrees: &[usize],
    limits: BipersistenceLimits,
) -> Result<(usize, usize)> {
    let node_count = scales
        .len()
        .checked_mul(minimum_degrees.len())
        .ok_or_else(|| Error::InvalidInput("bipersistence grid size overflows".into()))?;
    if node_count > limits.max_nodes {
        return Err(Error::InvalidInput(format!(
            "bipersistence node count exceeds the limit {}",
            limits.max_nodes
        )));
    }
    let horizontal = scales
        .len()
        .saturating_sub(1)
        .checked_mul(minimum_degrees.len())
        .ok_or_else(|| Error::InvalidInput("bipersistence cover count overflows".into()))?;
    let vertical = minimum_degrees
        .len()
        .saturating_sub(1)
        .checked_mul(scales.len())
        .ok_or_else(|| Error::InvalidInput("bipersistence cover count overflows".into()))?;
    let cover_count = horizontal
        .checked_add(vertical)
        .ok_or_else(|| Error::InvalidInput("bipersistence cover count overflows".into()))?;
    if cover_count > limits.max_cover_maps {
        return Err(Error::InvalidInput(format!(
            "bipersistence cover count exceeds the limit {}",
            limits.max_cover_maps
        )));
    }
    Ok((node_count, cover_count))
}

fn build_nodes(
    bifiltration: &crate::bifiltration::MulticriticalBifiltration,
    scales: &[f64],
    minimum_degrees: &[usize],
    modulus: u32,
    limits: BipersistenceLimits,
    node_count: usize,
) -> Result<(
    Vec<BipersistenceNode>,
    Vec<SparseDistanceMatrix>,
    Vec<CohomologySpace>,
)> {
    let mut nodes = Vec::with_capacity(node_count);
    let mut graphs = Vec::with_capacity(node_count);
    let mut spaces = Vec::with_capacity(node_count);
    let mut total_rank = 0usize;
    for scale in 0..scales.len() {
        for density in 0..minimum_degrees.len() {
            let grade = Bigrade::new(scale, density);
            let slice = bifiltration.slice(grade)?;
            let graph = slice.h1_graph(bifiltration.vertex_count())?;
            let space = cohomology_space(&graph, 1, 0.0, modulus, limits.cohomology)?;
            total_rank = total_rank
                .checked_add(space.rank())
                .ok_or_else(|| Error::InvalidInput("bipersistence total rank overflows".into()))?;
            if total_rank > limits.max_total_rank {
                return Err(Error::InvalidInput(format!(
                    "bipersistence total rank exceeds the limit {}",
                    limits.max_total_rank
                )));
            }
            nodes.push(BipersistenceNode {
                grade,
                space: space.id(),
                rank: space.rank(),
            });
            graphs.push(graph);
            spaces.push(space);
        }
    }
    Ok((nodes, graphs, spaces))
}

impl BipersistenceModule {
    fn populate_covers(&mut self) -> Result<()> {
        for scale in 0..self.scale_count() {
            for density in 0..self.density_count() {
                let lower = Bigrade::new(scale, density);
                if scale + 1 < self.scale_count() {
                    self.push_cover(lower, Bigrade::new(scale + 1, density))?;
                }
                if density + 1 < self.density_count() {
                    self.push_cover(lower, Bigrade::new(scale, density + 1))?;
                }
            }
        }
        Ok(())
    }

    fn validate_map_terms(&self) -> Result<()> {
        let map_terms = self
            .cover_maps
            .iter()
            .flat_map(|map| &map.columns)
            .map(|column| column.image.len())
            .try_fold(0usize, |total, count| total.checked_add(count))
            .ok_or_else(|| Error::InvalidInput("bipersistence map term count overflows".into()))?;
        if map_terms > self.limits.max_map_terms {
            return Err(Error::InvalidInput(format!(
                "bipersistence map term count exceeds the limit {}",
                self.limits.max_map_terms
            )));
        }
        Ok(())
    }
}
