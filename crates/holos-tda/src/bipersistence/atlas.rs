//! Class-extension fibers and connected atlas regions.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::Result;

use super::linear::{affine_solution, coordinate_vector, public_terms};
use super::{Bigrade, BipersistenceModule, BipersistenceTerm};

/// Classification of the affine extension fiber at one grid node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClassExtensionKind {
    /// The base class has one extension.
    Unique,
    /// The base class has an affine family of extensions.
    Ambiguous,
    /// The base class has no extension.
    NoExtension,
}

/// Exact extension fiber of one base class at one grid node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassExtension {
    /// Target grid node.
    pub grade: Bigrade,
    /// Classification of the affine fiber.
    pub kind: ClassExtensionKind,
    /// Unique extension or one canonical affine base point.
    pub class: Vec<BipersistenceTerm>,
    /// Canonical basis for the affine direction.
    pub ambiguity: Vec<Vec<BipersistenceTerm>>,
}

/// Connected region with one extension classification and ambiguity rank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassExtensionRegion {
    /// Stable zero-based region position.
    pub region_index: usize,
    /// Shared extension classification.
    pub kind: ClassExtensionKind,
    /// Dimension of the affine direction.
    pub ambiguity_rank: usize,
    /// Region grades in lexicographic order.
    pub grades: Vec<Bigrade>,
}

/// Exact class-extension atlas over an upper parameter cone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologyClassAtlas {
    /// Grade at which the selected class is defined.
    pub base_grade: Bigrade,
    /// Nonzero class in the base node's canonical basis.
    pub base_class: Vec<BipersistenceTerm>,
    /// Extension fiber at every grade above the base.
    pub extensions: Vec<ClassExtension>,
    /// Four-neighbor connected regions of equal fiber type and dimension.
    pub regions: Vec<ClassExtensionRegion>,
}

impl BipersistenceModule {
    /// Compute all extensions of one nonzero class over its upper parameter cone.
    pub fn class_atlas(
        &self,
        base_grade: Bigrade,
        base_class: &[BipersistenceTerm],
    ) -> Result<CohomologyClassAtlas> {
        self.validate_grade(base_grade)?;
        let base = coordinate_vector(
            base_class,
            self.node(base_grade)?.rank,
            self.modulus(),
            "bipersistence base class coordinates are not canonical",
        )?;
        if base.is_zero() {
            return Err(crate::Error::InvalidInput(
                "a bipersistence class atlas needs a nonzero base class".into(),
            ));
        }
        let mut extensions = Vec::new();
        for scale in base_grade.scale()..self.scale_count() {
            for density in base_grade.density()..self.density_count() {
                let grade = Bigrade::new(scale, density);
                extensions.push(self.extension_at_grade(base_grade, grade, &base)?);
            }
        }
        let regions = extension_regions(
            base_grade,
            self.scale_count(),
            self.density_count(),
            &extensions,
        );
        Ok(CohomologyClassAtlas {
            base_grade,
            base_class: public_terms(&base),
            extensions,
            regions,
        })
    }

    fn extension_at_grade(
        &self,
        base_grade: Bigrade,
        grade: Bigrade,
        base: &super::linear::SparseVector,
    ) -> Result<ClassExtension> {
        let map = self.map(base_grade, grade)?;
        let linear = self.linear_from_public(&map)?;
        let (kind, class, ambiguity) = match affine_solution(&linear.columns, base, self.modulus())
        {
            None => (ClassExtensionKind::NoExtension, Vec::new(), Vec::new()),
            Some((particular, kernel)) if kernel.is_empty() => (
                ClassExtensionKind::Unique,
                public_terms(&particular),
                Vec::new(),
            ),
            Some((particular, kernel)) => (
                ClassExtensionKind::Ambiguous,
                public_terms(&particular),
                kernel.iter().map(public_terms).collect(),
            ),
        };
        Ok(ClassExtension {
            grade,
            kind,
            class,
            ambiguity,
        })
    }
}

fn extension_regions(
    base: Bigrade,
    scale_count: usize,
    density_count: usize,
    extensions: &[ClassExtension],
) -> Vec<ClassExtensionRegion> {
    let by_grade = extensions
        .iter()
        .map(|extension| (extension.grade, extension))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    let mut regions = Vec::new();
    for extension in extensions {
        if !visited.insert(extension.grade) {
            continue;
        }
        let label = (extension.kind, extension.ambiguity.len());
        let mut queue = VecDeque::from([extension.grade]);
        let mut grades = Vec::new();
        while let Some(grade) = queue.pop_front() {
            grades.push(grade);
            for neighbor in grade_neighbors(grade, base, scale_count, density_count) {
                let candidate = by_grade[&neighbor];
                if (candidate.kind, candidate.ambiguity.len()) == label && visited.insert(neighbor)
                {
                    queue.push_back(neighbor);
                }
            }
        }
        grades.sort_unstable();
        regions.push(ClassExtensionRegion {
            region_index: regions.len(),
            kind: label.0,
            ambiguity_rank: label.1,
            grades,
        });
    }
    regions
}

fn grade_neighbors(
    grade: Bigrade,
    base: Bigrade,
    scale_count: usize,
    density_count: usize,
) -> Vec<Bigrade> {
    let mut neighbors = Vec::with_capacity(4);
    if grade.scale() > base.scale() {
        neighbors.push(Bigrade::new(grade.scale() - 1, grade.density()));
    }
    if grade.scale() + 1 < scale_count {
        neighbors.push(Bigrade::new(grade.scale() + 1, grade.density()));
    }
    if grade.density() > base.density() {
        neighbors.push(Bigrade::new(grade.scale(), grade.density() - 1));
    }
    if grade.density() + 1 < density_count {
        neighbors.push(Bigrade::new(grade.scale(), grade.density() + 1));
    }
    neighbors
}
