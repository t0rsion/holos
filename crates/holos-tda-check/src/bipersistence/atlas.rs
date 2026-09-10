use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::ProofError;

use super::claims::*;
use super::linear::{affine_solution, coordinate_vector, public_terms};
use super::module::CheckedModule;

impl CheckedModule {
    pub(crate) fn class_atlas(
        &self,
        base_grade: Grade,
        base_terms: &[Term],
    ) -> Result<AtlasClaim, ProofError> {
        let base = coordinate_vector(
            base_terms,
            self.node(base_grade)?.space.rank(),
            self.modulus,
            "bipersistence base class is not canonical",
        )?;
        if base.0.is_empty() {
            return Err(ProofError::new(
                "bipersistence class atlas needs a nonzero base class",
            ));
        }
        let extensions = self.extensions(base_grade, &base)?;
        let regions = extension_regions(
            base_grade,
            self.scale_count(),
            self.density_count(),
            &extensions,
        );
        Ok(AtlasClaim {
            base_grade,
            base_class: public_terms(&base),
            extensions,
            regions,
        })
    }

    fn extensions(
        &self,
        base_grade: Grade,
        base: &super::linear::Vector,
    ) -> Result<Vec<Extension>, ProofError> {
        let mut extensions = Vec::new();
        for scale in base_grade.scale..self.scale_count() {
            for density in base_grade.density..self.density_count() {
                extensions.push(self.extension(base_grade, Grade { scale, density }, base)?);
            }
        }
        Ok(extensions)
    }

    fn extension(
        &self,
        base_grade: Grade,
        grade: Grade,
        base: &super::linear::Vector,
    ) -> Result<Extension, ProofError> {
        let map = self.map(base_grade, grade)?;
        let (kind, class, ambiguity) = match affine_solution(&map.columns, base, self.modulus) {
            None => (ExtensionKind::NoExtension, Vec::new(), Vec::new()),
            Some((particular, kernel)) if kernel.is_empty() => {
                (ExtensionKind::Unique, public_terms(&particular), Vec::new())
            }
            Some((particular, kernel)) => (
                ExtensionKind::Ambiguous,
                public_terms(&particular),
                kernel.iter().map(public_terms).collect(),
            ),
        };
        Ok(Extension {
            grade,
            kind,
            class,
            ambiguity,
        })
    }
}

fn extension_regions(
    base: Grade,
    scale_count: usize,
    density_count: usize,
    extensions: &[Extension],
) -> Vec<Region> {
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
        regions.push(Region {
            index: regions.len(),
            kind: label.0,
            ambiguity_rank: label.1,
            grades,
        });
    }
    regions
}

fn grade_neighbors(
    grade: Grade,
    base: Grade,
    scale_count: usize,
    density_count: usize,
) -> Vec<Grade> {
    let mut neighbors = Vec::with_capacity(4);
    if grade.scale > base.scale {
        neighbors.push(Grade {
            scale: grade.scale - 1,
            density: grade.density,
        });
    }
    if grade.scale + 1 < scale_count {
        neighbors.push(Grade {
            scale: grade.scale + 1,
            density: grade.density,
        });
    }
    if grade.density > base.density {
        neighbors.push(Grade {
            scale: grade.scale,
            density: grade.density - 1,
        });
    }
    if grade.density + 1 < density_count {
        neighbors.push(Grade {
            scale: grade.scale,
            density: grade.density + 1,
        });
    }
    neighbors
}
