use holos_tda::collapse::{CollapseCertificate, CollapsedRips, RemovalStep};
use holos_tda::{Diagram, SparseDistanceMatrix};

use super::super::model::{Args, Config, Kind, OrderedGate};

impl OrderedGate {
    pub(super) fn observe(
        &mut self,
        args: &Args,
        cfg: &Config,
        collapsed: Option<CollapsedRips>,
    ) -> Result<(), String> {
        match cfg.kind {
            Kind::V1 => self.serial_v1 = collapsed,
            Kind::V1Ordered => self.check_ordered(args, cfg, collapsed)?,
            Kind::V2 | Kind::V1Product | Kind::NoCollapse => {}
        }
        Ok(())
    }

    fn check_ordered(
        &mut self,
        args: &Args,
        cfg: &Config,
        collapsed: Option<CollapsedRips>,
    ) -> Result<(), String> {
        let ordered = collapsed
            .ok_or_else(|| format!("configuration {} produced no collapse result", cfg.name))?;
        let reference = self.serial_v1.as_ref().ok_or_else(|| {
            format!(
                "entry {}: configuration {} has no v1-c1 reference in this run, so its ordered gate cannot run; add v1 to --mode",
                args.entry, cfg.name
            )
        })?;
        ordered_equal(reference, &ordered).map_err(|diff| {
            format!(
                "entry {}: {} does not reproduce v1-c1: {diff}; timings void",
                args.entry, cfg.name
            )
        })?;
        self.lines.push(format!(
            "kind=ordered_gate entry={} config={} reference=v1-c1 checked=yes matrix=match certificate=match counters=match",
            args.entry, cfg.name
        ));
        Ok(())
    }
}

/// The ordered gate. An ordered run executes the version 1 schedule, so it
/// must reproduce the serial version 1 run: the same collapsed matrix, the
/// same certificate, and the counters the schedule fixes. The first
/// difference found is the error.
fn ordered_equal(reference: &CollapsedRips, ordered: &CollapsedRips) -> Result<(), String> {
    matrices_equal(&reference.matrix, &ordered.matrix)?;
    certificates_equal(&reference.certificate, &ordered.certificate)?;
    let (want, got) = (&reference.stats, &ordered.stats);
    let fields = [
        ("input_edges", want.input_edges, got.input_edges),
        ("output_edges", want.output_edges, got.output_edges),
        ("removed_edges", want.removed_edges, got.removed_edges),
        ("passes", want.epochs, got.epochs),
        (
            "witness_segments",
            want.witness_segments,
            got.witness_segments,
        ),
        ("logical_tests", want.edge_tests, got.logical_tests),
    ];
    for (name, want, got) in fields {
        if want != got {
            return Err(format!("{name} is {got}, the serial run has {want}"));
        }
    }
    Ok(())
}

fn matrices_equal(
    reference: &SparseDistanceMatrix,
    ordered: &SparseDistanceMatrix,
) -> Result<(), String> {
    if reference.len() != ordered.len() {
        return Err(format!(
            "the collapsed matrix has {} vertices, the serial run has {}",
            ordered.len(),
            reference.len()
        ));
    }
    if reference.num_edges() != ordered.num_edges() {
        return Err(format!(
            "the collapsed matrix has {} edges, the serial run has {}",
            ordered.num_edges(),
            reference.num_edges()
        ));
    }
    for (index, (want, got)) in reference.edges().zip(ordered.edges()).enumerate() {
        if want.0 != got.0 || want.1 != got.1 || want.2.to_bits() != got.2.to_bits() {
            return Err(format!(
                "collapsed edge {index} is ({}, {}, {:?}), the serial run has ({}, {}, {:?})",
                got.0, got.1, got.2, want.0, want.1, want.2
            ));
        }
    }
    Ok(())
}

fn certificates_equal(
    reference: &CollapseCertificate,
    ordered: &CollapseCertificate,
) -> Result<(), String> {
    certificate_version_equal(reference, ordered)?;
    certificate_header_equal(reference, ordered)?;
    certificate_levels_equal(reference, ordered)?;
    for (index, (want, got)) in reference.steps().iter().zip(ordered.steps()).enumerate() {
        removal_step_equal(index, want, got)?;
    }
    Ok(())
}

fn certificate_version_equal(
    reference: &CollapseCertificate,
    ordered: &CollapseCertificate,
) -> Result<(), String> {
    if reference.algorithm_version() != ordered.algorithm_version() {
        return Err(format!(
            "the certificate is algorithm version {}, the serial run is version {}",
            ordered.algorithm_version(),
            reference.algorithm_version()
        ));
    }
    Ok(())
}

fn certificate_header_equal(
    reference: &CollapseCertificate,
    ordered: &CollapseCertificate,
) -> Result<(), String> {
    let header = [
        (
            "vertex_count",
            reference.vertex_count(),
            ordered.vertex_count(),
        ),
        (
            "input_edge_count",
            reference.input_edge_count(),
            ordered.input_edge_count(),
        ),
        (
            "output_edge_count",
            reference.output_edge_count(),
            ordered.output_edge_count(),
        ),
        ("steps", reference.steps().len(), ordered.steps().len()),
    ];
    for (name, want, got) in header {
        if want != got {
            return Err(format!(
                "the certificate {name} is {got}, the serial run has {want}"
            ));
        }
    }
    Ok(())
}

fn certificate_levels_equal(
    reference: &CollapseCertificate,
    ordered: &CollapseCertificate,
) -> Result<(), String> {
    if optional_bits(reference.requested_threshold())
        != optional_bits(ordered.requested_threshold())
    {
        return Err(format!(
            "the certificate requested threshold is {:?}, the serial run has {:?}",
            ordered.requested_threshold(),
            reference.requested_threshold()
        ));
    }
    if reference.terminal_level().to_bits() != ordered.terminal_level().to_bits() {
        return Err(format!(
            "the certificate terminal level is {:?}, the serial run has {:?}",
            ordered.terminal_level(),
            reference.terminal_level()
        ));
    }
    Ok(())
}

fn removal_step_equal(index: usize, want: &RemovalStep, got: &RemovalStep) -> Result<(), String> {
    if want.edge() != got.edge() {
        return Err(format!(
            "removal {index} is edge {:?}, the serial run removes {:?}",
            got.edge(),
            want.edge()
        ));
    }
    if want.value().to_bits() != got.value().to_bits() {
        return Err(format!(
            "removal {index} has value {:?}, the serial run has {:?}",
            got.value(),
            want.value()
        ));
    }
    if want.position().number() != got.position().number() {
        return Err(format!(
            "removal {index} is in pass {}, the serial run puts it in pass {}",
            got.position().number(),
            want.position().number()
        ));
    }
    witness_steps_equal(index, want, got)
}

fn witness_steps_equal(index: usize, want: &RemovalStep, got: &RemovalStep) -> Result<(), String> {
    if want.witnesses().len() != got.witnesses().len() {
        return Err(format!(
            "removal {index} has {} witness segments, the serial run has {}",
            got.witnesses().len(),
            want.witnesses().len()
        ));
    }
    for (segment, (want, got)) in want.witnesses().iter().zip(got.witnesses()).enumerate() {
        if want.0.to_bits() != got.0.to_bits() || want.1 != got.1 {
            return Err(format!(
                "removal {index} witness segment {segment} is {:?}, the serial run has {:?}",
                got, want
            ));
        }
    }
    Ok(())
}

fn optional_bits(value: Option<f64>) -> Option<u64> {
    value.map(f64::to_bits)
}

pub(super) fn diagrams_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(x, y)| {
            x.dim == y.dim
                && x.birth.to_bits() == y.birth.to_bits()
                && x.death.to_bits() == y.death.to_bits()
        })
}
