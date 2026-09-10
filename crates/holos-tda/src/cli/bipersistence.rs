use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::Parser;

use super::{InputFormat, read_proof_input, version_string, write_via_temporary};
use crate::{
    BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceModule,
    BipersistenceRectangleClaim, BipersistenceRegionClaim, BipersistenceTerm,
    CircularCoordinateFamily, CircularCoordinateParams, ClassExtensionKind, CohomologyClassAtlas,
    CohomologyLimits, Result,
};

mod input;

#[derive(Parser)]
#[command(
    name = "holos bipersistence",
    version = version_string(),
    about = "Build and certify finite degree-Rips H1 bipersistence",
    after_help = "Grid coordinates are zero-based indices. A class cocycle contains `u v coefficient` rows. Check OUTPUT with: holos-check OUTPUT"
)]
pub(super) struct BipersistenceCli {
    /// Input point cloud, lower-distance matrix, or sparse weighted graph
    input: PathBuf,

    /// Output `HOLOSBP` artifact
    output: PathBuf,

    /// Input format. Inferred when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Largest degree-Rips scale. The input default is used when omitted
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Scale on a declared finite grid. Repeat in strict ascending order
    #[arg(long = "scale", value_name = "T")]
    scales: Vec<f64>,

    /// Minimum degree on a declared finite grid. Repeat in strict descending order
    #[arg(long = "minimum-degree", value_name = "K")]
    minimum_degrees: Vec<usize>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 47)]
    modulus: u32,

    /// Rectangle corners as `LOW_SCALE LOW_DENSITY HIGH_SCALE HIGH_DENSITY`
    #[arg(long = "rectangle", value_names = ["S0", "D0", "S1", "D1"], num_args = 4)]
    rectangles: Vec<usize>,

    /// File with `SCALE DENSITY` rows for one connected region. Repeat as needed
    #[arg(long = "region", value_name = "FILE")]
    regions: Vec<PathBuf>,

    /// Canonical basis class as `SCALE DENSITY BASIS`. Repeat as needed
    #[arg(long = "class", value_names = ["S", "D", "B"], num_args = 3)]
    classes: Vec<usize>,

    /// Cocycle class as `SCALE DENSITY FILE`. Repeat as needed
    #[arg(long = "class-cocycle", value_names = ["S", "D", "FILE"], num_args = 3)]
    class_cocycles: Vec<String>,

    /// Add a checked circular coordinate at every unique class extension
    #[arg(long)]
    circular: bool,

    /// Largest accepted relative normal-equation residual
    #[arg(long, value_name = "R", default_value_t = 1e-10)]
    tolerance: f64,

    /// Largest harmonic solver iteration count
    #[arg(long, value_name = "N", default_value_t = 10_000)]
    max_iterations: usize,

    /// Write a JSON report with axes, ranks, fibers, and optional phases
    #[arg(long, value_name = "FILE")]
    report: Option<PathBuf>,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Largest accepted output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

pub(super) fn run(cli: BipersistenceCli) -> Result<()> {
    let input_threshold = input::input_threshold(&cli);
    let graph = read_proof_input(&cli.input, cli.format, cli.threads, input_threshold)?;
    let degree_rips = input::build_degree_rips(&graph, &cli, input_threshold)?;
    let limits = input::artifact_limits(&cli);
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, cli.modulus, limits)?;
    input::record_rectangles(&mut artifact, &module, &cli.rectangles, limits)?;
    input::record_regions(
        &mut artifact,
        &module,
        &cli.regions,
        cli.max_artifact_bytes,
        limits,
    )?;
    let selections = input::selected_classes(&module, &cli)?;
    let circular_params = CircularCoordinateParams {
        tolerance: cli.tolerance,
        max_iterations: cli.max_iterations,
        cohomology: CohomologyLimits::default(),
    };
    let (atlases, families) = input::record_class_claims(
        &mut artifact,
        &module,
        selections,
        cli.circular,
        circular_params,
        limits,
    )?;

    if let Some(path) = &cli.report {
        write_report(
            path,
            &module,
            artifact.rectangles(),
            artifact.regions(),
            &atlases,
            &families,
        )?;
    }
    write_artifact(&cli.output, &artifact, limits)
}

fn write_artifact(
    output: &Path,
    artifact: &BipersistenceArtifact,
    limits: BipersistenceArtifactLimits,
) -> Result<()> {
    let bytes = artifact.encode(limits)?;
    write_via_temporary(output, &bytes)?;
    let summary = artifact.summary();
    println!(
        "wrote degree-Rips H1 module with {} nodes, {} cover maps, {} rectangle claims, {} connected-region claims, {} class atlases, and {} circular families to {}",
        summary.nodes,
        summary.cover_maps,
        summary.rectangles,
        summary.regions,
        summary.class_atlases,
        summary.circular_families,
        output.display(),
    );
    Ok(())
}

fn write_report(
    path: &Path,
    module: &BipersistenceModule,
    rectangles: &[BipersistenceRectangleClaim],
    regions: &[BipersistenceRegionClaim],
    atlases: &[CohomologyClassAtlas],
    families: &[CircularCoordinateFamily],
) -> Result<()> {
    let mut json = String::new();
    write_report_header(&mut json, module);
    write_report_nodes(&mut json, module);
    write_report_rectangles(&mut json, rectangles);
    write_report_regions(&mut json, regions);
    write_report_atlases(&mut json, atlases);
    write_report_families(&mut json, families);
    write_via_temporary(path, json.as_bytes())
}

fn write_report_header(json: &mut String, module: &BipersistenceModule) {
    json.push_str("{\n  \"format\": \"holos-degree-rips-report-v1\",\n  \"modulus\": ");
    write!(json, "{},\n  \"scales\": [", module.modulus())
        .expect("writing to a string cannot fail");
    for (position, scale) in module.scales().iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write!(json, "{scale}").expect("writing to a string cannot fail");
    }
    json.push_str("],\n  \"minimum_degrees\": [");
    for (position, degree) in module.minimum_degrees().iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write!(json, "{degree}").expect("writing to a string cannot fail");
    }
    json.push_str("],\n  \"nodes\": [");
}

fn write_report_nodes(json: &mut String, module: &BipersistenceModule) {
    for (position, node) in module.nodes().iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write!(
            json,
            "\n    {{\"scale_index\":{},\"density_index\":{},\"rank\":{}}}",
            node.grade.scale(),
            node.grade.density(),
            node.rank,
        )
        .expect("writing to a string cannot fail");
    }
    json.push_str("\n  ],\n  \"rectangles\": [");
}

fn write_report_rectangles(json: &mut String, rectangles: &[BipersistenceRectangleClaim]) {
    for (position, claim) in rectangles.iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        let low = claim.rectangle.lower;
        let high = claim.rectangle.upper;
        write!(
            json,
            "\n    {{\"low\":[{},{}],\"high\":[{},{}],\"rank\":{}}}",
            low.scale(),
            low.density(),
            high.scale(),
            high.density(),
            claim.rank,
        )
        .expect("writing to a string cannot fail");
    }
    json.push_str("\n  ],\n  \"regions\": [");
}

fn write_report_regions(json: &mut String, regions: &[BipersistenceRegionClaim]) {
    for (position, claim) in regions.iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        json.push_str("\n    {\"grades\":[");
        for (grade_position, grade) in claim.region.grades().iter().enumerate() {
            if grade_position != 0 {
                json.push(',');
            }
            write!(json, "[{},{}]", grade.scale(), grade.density())
                .expect("writing to a string cannot fail");
        }
        write!(json, "],\"rank\":{}}}", claim.rank).expect("writing to a string cannot fail");
    }
    json.push_str("\n  ],\n  \"class_atlases\": [");
}

fn write_report_atlases(json: &mut String, atlases: &[CohomologyClassAtlas]) {
    for (position, atlas) in atlases.iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write_atlas_json(json, atlas);
    }
    json.push_str("\n  ],\n  \"circular_families\": [");
}

fn write_report_families(json: &mut String, families: &[CircularCoordinateFamily]) {
    for (position, family) in families.iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write_family_json(json, family);
    }
    json.push_str("\n  ]\n}\n");
}

fn write_atlas_json(json: &mut String, atlas: &CohomologyClassAtlas) {
    write!(
        json,
        "\n    {{\"base\":[{},{}],\"class\":[",
        atlas.base_grade.scale(),
        atlas.base_grade.density(),
    )
    .expect("writing to a string cannot fail");
    write_terms_json(json, &atlas.base_class);
    json.push_str("],\"extensions\":[");
    for (position, extension) in atlas.extensions.iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write!(
            json,
            "{{\"grade\":[{},{}],\"kind\":\"{}\",\"ambiguity_rank\":{}}}",
            extension.grade.scale(),
            extension.grade.density(),
            extension_kind(extension.kind),
            extension.ambiguity.len(),
        )
        .expect("writing to a string cannot fail");
    }
    write!(json, "],\"region_count\":{}}}", atlas.regions.len())
        .expect("writing to a string cannot fail");
}

fn write_family_json(json: &mut String, family: &CircularCoordinateFamily) {
    write!(
        json,
        "\n    {{\"base\":[{},{}],\"entries\":[",
        family.base_grade.scale(),
        family.base_grade.density(),
    )
    .expect("writing to a string cannot fail");
    for (position, entry) in family.entries.iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write!(
            json,
            "{{\"grade\":[{},{}],\"kind\":\"{}\",\"phase\":",
            entry.grade.scale(),
            entry.grade.density(),
            extension_kind(entry.extension),
        )
        .expect("writing to a string cannot fail");
        match &entry.coordinate {
            None => json.push_str("null"),
            Some(coordinate) => {
                json.push('[');
                for (phase_position, phase) in coordinate.phase.iter().enumerate() {
                    if phase_position != 0 {
                        json.push(',');
                    }
                    write!(json, "{phase}").expect("writing to a string cannot fail");
                }
                json.push(']');
            }
        }
        json.push('}');
    }
    json.push_str("]}");
}

fn write_terms_json(json: &mut String, terms: &[BipersistenceTerm]) {
    for (position, term) in terms.iter().enumerate() {
        if position != 0 {
            json.push(',');
        }
        write!(json, "[{},{}]", term.basis_index, term.coefficient)
            .expect("writing to a string cannot fail");
    }
}

fn extension_kind(kind: ClassExtensionKind) -> &'static str {
    match kind {
        ClassExtensionKind::Unique => "unique",
        ClassExtensionKind::Ambiguous => "ambiguous",
        ClassExtensionKind::NoExtension => "no_extension",
    }
}
