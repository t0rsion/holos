use std::path::PathBuf;

use crate::{CoverageGeometry, CoverageGeometryLimits, CoverageSpecification, PlanarPoint, io};

pub(super) fn read_coverage_geometry(
    paths: &[PathBuf],
    specification: &CoverageSpecification,
    threads: usize,
) -> crate::Result<Option<CoverageGeometry>> {
    if paths.is_empty() {
        return Ok(None);
    }
    if paths.len() != specification.states().len() {
        return Err(crate::Error::InvalidInput(format!(
            "--coordinates needs one file per state, got {} files for {} states",
            paths.len(),
            specification.states().len()
        )));
    }
    let mut states = Vec::with_capacity(paths.len());
    for path in paths {
        let values = io::read_point_cloud(path, threads)?;
        let points = values
            .into_iter()
            .map(|coordinates| {
                let [x, y]: [f64; 2] =
                    coordinates.try_into().map_err(|coordinates: Vec<f64>| {
                        crate::Error::InvalidInput(format!(
                            "{} has a point with {} coordinates instead of 2",
                            path.display(),
                            coordinates.len()
                        ))
                    })?;
                PlanarPoint::new(x, y)
            })
            .collect::<crate::Result<Vec<_>>>()?;
        states.push(points);
    }
    CoverageGeometry::new(specification, states, CoverageGeometryLimits::default()).map(Some)
}
