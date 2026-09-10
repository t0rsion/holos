use std::path::{Path, PathBuf};

use crate::{
    Bigrade, BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceModule,
    BipersistenceRectangle, BipersistenceRegion, BipersistenceTerm, CircularCoordinateFamily,
    CircularCoordinateParams, CochainTerm, CohomologyClassAtlas, DegreeRipsBifiltration,
    DegreeRipsParams, Error, Result, cocycle_from_ripser_terms,
};

use super::super::read_circular_cocycle;
use super::BipersistenceCli;

pub(super) fn input_threshold(cli: &BipersistenceCli) -> Option<f64> {
    cli.threshold.or(cli.scales.last().copied())
}

pub(super) fn build_degree_rips(
    graph: &crate::SparseDistanceMatrix,
    cli: &BipersistenceCli,
    input_threshold: Option<f64>,
) -> Result<DegreeRipsBifiltration> {
    let params = DegreeRipsParams {
        max_homology_dimension: 1,
        threshold: input_threshold,
        ..DegreeRipsParams::default()
    };
    match (cli.scales.is_empty(), cli.minimum_degrees.is_empty()) {
        (true, true) => DegreeRipsBifiltration::from_graph(graph, params),
        (false, false) => DegreeRipsBifiltration::from_graph_on_grid(
            graph,
            cli.scales.clone(),
            cli.minimum_degrees.clone(),
            params,
        ),
        _ => Err(Error::InvalidInput(
            "a declared grid needs both --scale and --minimum-degree values".into(),
        )),
    }
}

pub(super) fn artifact_limits(cli: &BipersistenceCli) -> BipersistenceArtifactLimits {
    BipersistenceArtifactLimits {
        max_bytes: cli.max_artifact_bytes,
        max_coordinate_bytes: cli.max_artifact_bytes,
        ..BipersistenceArtifactLimits::default()
    }
}

pub(super) fn record_rectangles(
    artifact: &mut BipersistenceArtifact,
    module: &BipersistenceModule,
    values: &[usize],
    limits: BipersistenceArtifactLimits,
) -> Result<()> {
    for values in values.chunks_exact(4) {
        artifact.record_rectangle(
            module,
            BipersistenceRectangle::new(
                Bigrade::new(values[0], values[1]),
                Bigrade::new(values[2], values[3]),
            )?,
            limits,
        )?;
    }
    Ok(())
}

pub(super) fn record_regions(
    artifact: &mut BipersistenceArtifact,
    module: &BipersistenceModule,
    paths: &[PathBuf],
    maximum_bytes: usize,
    limits: BipersistenceArtifactLimits,
) -> Result<()> {
    for path in paths {
        artifact.record_region(module, read_region(path, maximum_bytes)?, limits)?;
    }
    Ok(())
}

pub(super) fn selected_classes(
    module: &BipersistenceModule,
    cli: &BipersistenceCli,
) -> Result<Vec<(Bigrade, Vec<BipersistenceTerm>)>> {
    let mut selections = basis_selections(&cli.classes);
    selections.extend(cocycle_selections(
        module,
        &cli.class_cocycles,
        cli.modulus,
    )?);
    selections.sort();
    selections.dedup();
    if cli.circular && selections.is_empty() {
        return Err(Error::InvalidInput(
            "--circular requires at least one --class or --class-cocycle".into(),
        ));
    }
    Ok(selections)
}

pub(super) fn record_class_claims(
    artifact: &mut BipersistenceArtifact,
    module: &BipersistenceModule,
    selections: Vec<(Bigrade, Vec<BipersistenceTerm>)>,
    circular: bool,
    circular_params: CircularCoordinateParams,
    limits: BipersistenceArtifactLimits,
) -> Result<(Vec<CohomologyClassAtlas>, Vec<CircularCoordinateFamily>)> {
    let mut atlases = Vec::with_capacity(selections.len());
    let mut families = Vec::new();
    for (grade, class) in selections {
        let atlas = module.class_atlas(grade, &class)?;
        artifact.record_class_atlas(module, &atlas, limits)?;
        if circular {
            families.push(module.circular_coordinate_family(&atlas, circular_params)?);
            artifact.record_circular_family(module, &atlas, circular_params, limits)?;
        }
        atlases.push(atlas);
    }
    Ok((atlases, families))
}

fn read_region(path: &Path, maximum_bytes: usize) -> Result<BipersistenceRegion> {
    let bytes =
        super::super::input::read_bounded_artifact(path, maximum_bytes, "bipersistence region")?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| Error::InvalidInput(format!("invalid region file: {error}")))?;
    let mut grades = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 2 {
            return Err(Error::InvalidInput(format!(
                "region line {} needs scale and density indices",
                line_index + 1
            )));
        }
        grades.push(Bigrade::new(
            parse_grid_index(fields[0], "region scale index")?,
            parse_grid_index(fields[1], "region density index")?,
        ));
    }
    BipersistenceRegion::new(grades)
}

fn basis_selections(values: &[usize]) -> Vec<(Bigrade, Vec<BipersistenceTerm>)> {
    values
        .chunks_exact(3)
        .map(|values| {
            (
                Bigrade::new(values[0], values[1]),
                vec![BipersistenceTerm {
                    basis_index: values[2],
                    coefficient: 1,
                }],
            )
        })
        .collect()
}

fn cocycle_selections(
    module: &BipersistenceModule,
    values: &[String],
    modulus: u32,
) -> Result<Vec<(Bigrade, Vec<BipersistenceTerm>)>> {
    values
        .chunks_exact(3)
        .map(|values| {
            let scale = parse_grid_index(&values[0], "class cocycle scale index")?;
            let density = parse_grid_index(&values[1], "class cocycle density index")?;
            let grade = Bigrade::new(scale, density);
            let rows = read_circular_cocycle(Path::new(&values[2]), modulus)?;
            let cocycle = cocycle_from_ripser_terms(module.h1_graph(grade)?, modulus, 0.0, &rows)?;
            let cochain = cocycle
                .terms
                .iter()
                .map(|term| CochainTerm {
                    simplex: vec![term.u, term.v],
                    coefficient: term.coefficient,
                })
                .collect::<Vec<_>>();
            let coordinates = module
                .cohomology_space(grade)?
                .coordinates_of_cocycle(&cochain)?;
            if coordinates.is_empty() {
                return Err(Error::InvalidInput(
                    "a selected degree-Rips cocycle is cohomologically zero".into(),
                ));
            }
            Ok((
                grade,
                coordinates
                    .into_iter()
                    .map(|(basis_index, coefficient)| BipersistenceTerm {
                        basis_index,
                        coefficient,
                    })
                    .collect(),
            ))
        })
        .collect()
}

fn parse_grid_index(value: &str, name: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|error| Error::InvalidInput(format!("invalid {name} {value:?}: {error}")))
}
