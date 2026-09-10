//! Verification workflows for collapse and proof artifacts.

use crate::collapse::CollapseCompleteness;
use crate::collapse::verify::{verify_dense_artifact, verify_sparse_artifact};
use crate::collapse::wire::{CollapseArtifact, DecodeLimits};
use crate::io;
use crate::{
    AtlasArtifact, AtlasDecodeLimits, CertificateLimits, DistanceMatrix, InterventionArtifact,
    InterventionBudget, InterventionDecodeLimits, PointCloudGraph, PointCloudParams,
    ProgramArtifact, ProgramDecodeLimits, ProgramTraceArtifact, ProgramTraceDecodeLimits,
    TrajectoryArtifact, TrajectoryDecodeLimits,
};

use super::args::{
    InputFormat, InterveneCli, VerifyAtlasCli, VerifyCli, VerifyInterventionCli, VerifyProgramCli,
    VerifyProgramTraceCli, VerifyTrajectoryCli,
};
use super::input::{
    infer_format, invalid_input, read_bounded_artifact, read_proof_input, write_via_temporary,
};

pub(super) fn run_verify(cli: VerifyCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(&cli.artifact, cli.max_artifact_bytes, "collapse artifact")?;
    let limits = DecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        ..DecodeLimits::default()
    };
    let artifact = CollapseArtifact::decode(&bytes, limits).map_err(invalid_input)?;
    verify_collapse_input(&cli, &artifact)?;
    report_verified_collapse(&artifact);
    Ok(())
}

fn verify_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let format = cli.format.unwrap_or_else(|| infer_format(&cli.input));
    match format {
        InputFormat::Sparse => verify_sparse_collapse_input(cli, artifact),
        InputFormat::PointCloud => verify_point_collapse_input(cli, artifact),
        InputFormat::LowerDistance => verify_lower_collapse_input(cli, artifact),
    }
}

fn report_verified_collapse(artifact: &CollapseArtifact) {
    let completeness = match artifact.certificate().completeness() {
        CollapseCompleteness::CompleteFixedPoint => "complete fixed point",
        CollapseCompleteness::BudgetLimited => "budget-limited partial collapse",
    };
    println!(
        "verified collapse artifact: algorithm version {}, {completeness}, {} removals",
        artifact.certificate().algorithm_version(),
        artifact.certificate().steps().len()
    );
}

fn verify_sparse_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let matrix = io::read_sparse_matrix(&cli.input, cli.threads.max(1))?;
    verify_sparse_artifact(&matrix, cli.threshold, artifact).map_err(invalid_input)
}

fn verify_point_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let points = io::read_point_cloud(&cli.input, cli.threads.max(1))?;
    if let Some(threshold) = cli.threshold {
        let graph = PointCloudGraph::build(
            &points,
            PointCloudParams::new(threshold).with_threads(cli.threads),
        )?;
        return verify_sparse_artifact(graph.matrix(), Some(threshold), artifact)
            .map_err(invalid_input);
    }
    let matrix = DistanceMatrix::from_points(&points)?;
    verify_dense_artifact(&matrix, None, artifact).map_err(invalid_input)
}

fn verify_lower_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let matrix = io::read_lower_distance_matrix(&cli.input, cli.threads.max(1))?;
    verify_dense_artifact(&matrix, cli.threshold, artifact).map_err(invalid_input)
}

pub(super) fn run_verify_atlas(cli: VerifyAtlasCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(&cli.artifact, cli.max_artifact_bytes, "persistence atlas")?;
    let atlas_limits = AtlasDecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        max_certificate_bytes: cli.max_artifact_bytes,
        ..AtlasDecodeLimits::default()
    };
    let certificate_limits = CertificateLimits {
        max_bytes: cli.max_artifact_bytes,
        ..CertificateLimits::default()
    };
    let artifact =
        AtlasArtifact::decode(&bytes, atlas_limits, certificate_limits).map_err(invalid_input)?;
    let matrix = read_proof_input(&cli.input, cli.format, cli.threads, artifact.threshold())?;
    artifact
        .verify(&matrix, certificate_limits)
        .map_err(invalid_input)?;
    let classes: usize = artifact
        .spaces()
        .iter()
        .map(|space| space.basis.len())
        .sum();
    println!(
        "verified persistence atlas: Z/{}, {} bars, {} H1 class spaces, {} basis classes",
        artifact.modulus(),
        artifact.diagram().bars.len(),
        artifact.spaces().len(),
        classes
    );
    Ok(())
}

pub(super) fn run_verify_trajectory(cli: VerifyTrajectoryCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(
        &cli.artifact,
        cli.max_artifact_bytes,
        "persistence trajectory",
    )?;
    let trace_limits = TrajectoryDecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        max_atlas_bytes: cli.max_artifact_bytes,
        ..TrajectoryDecodeLimits::default()
    };
    let atlas_limits = AtlasDecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        max_certificate_bytes: cli.max_artifact_bytes,
        ..AtlasDecodeLimits::default()
    };
    let certificate_limits = CertificateLimits {
        max_bytes: cli.max_artifact_bytes,
        ..CertificateLimits::default()
    };
    let artifact =
        TrajectoryArtifact::decode(&bytes, trace_limits, atlas_limits, certificate_limits)
            .map_err(invalid_input)?;
    let verified = artifact.verify(certificate_limits).map_err(invalid_input)?;
    let reused = verified
        .steps
        .iter()
        .filter(|step| step.mode == crate::UpdateMode::Reused)
        .count();
    let events: usize = verified.steps.iter().map(|step| step.events.len()).sum();
    println!(
        "verified persistence trajectory: {} steps, {reused} reused, {events} region events",
        verified.steps.len()
    );
    Ok(())
}

fn program_limits(maximum: usize) -> ProgramDecodeLimits {
    ProgramDecodeLimits {
        max_bytes: maximum,
        max_atlas_bytes: maximum,
        atlas: AtlasDecodeLimits {
            max_bytes: maximum,
            max_certificate_bytes: maximum,
            ..AtlasDecodeLimits::default()
        },
        ..ProgramDecodeLimits::default()
    }
}

fn program_trace_limits(maximum: usize) -> ProgramTraceDecodeLimits {
    ProgramTraceDecodeLimits {
        max_bytes: maximum,
        max_checkpoint_bytes: maximum,
        program: program_limits(maximum),
        ..ProgramTraceDecodeLimits::default()
    }
}

pub(crate) fn certificate_limits(maximum: usize) -> CertificateLimits {
    CertificateLimits {
        max_bytes: maximum,
        ..CertificateLimits::default()
    }
}

pub(super) fn run_verify_program(cli: VerifyProgramCli) -> crate::Result<()> {
    let bytes =
        read_bounded_artifact(&cli.artifact, cli.max_artifact_bytes, "persistence program")?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact = ProgramArtifact::decode(&bytes, program_limits(cli.max_artifact_bytes), limits)
        .map_err(invalid_input)?;
    let matrix = read_proof_input(&cli.input, cli.format, cli.threads, artifact.threshold())?;
    let program = artifact.verify(&matrix, limits).map_err(invalid_input)?;
    let summary = program.summary();
    println!(
        "verified persistence program: Z/{}, {} bars, {} atoms, {} cyclic atoms, {} guards",
        artifact.modulus(),
        artifact.diagram().bars.len(),
        summary.atoms,
        summary.cyclic_atoms,
        summary.guards
    );
    Ok(())
}

pub(super) fn run_verify_program_trace(cli: VerifyProgramTraceCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(
        &cli.artifact,
        cli.max_artifact_bytes,
        "persistence program trace",
    )?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact =
        ProgramTraceArtifact::decode(&bytes, program_trace_limits(cli.max_artifact_bytes), limits)
            .map_err(invalid_input)?;
    let verified = artifact.verify(limits).map_err(invalid_input)?;
    let reused = verified
        .steps
        .iter()
        .filter(|step| step.mode == crate::ProgramUpdateMode::Reused)
        .count();
    let repaired = verified
        .steps
        .iter()
        .filter(|step| step.mode == crate::ProgramUpdateMode::Repaired)
        .count();
    println!(
        "verified persistence program trace: {} steps, {reused} reused, {repaired} repaired",
        verified.steps.len()
    );
    Ok(())
}

pub(super) fn run_intervene(cli: InterveneCli) -> crate::Result<()> {
    validate_intervention_budget(cli.budget)?;
    let program = read_intervention_program(&cli)?;
    let target = intervention_target(&program, cli.space)?;
    let intervention =
        program.kill_h1_before(target, cli.before, InterventionBudget::new(cli.budget))?;
    let proof = intervention.artifact.ok_or_else(|| {
        crate::Error::InvalidInput(
            "the candidate budget ended without a certified intervention".into(),
        )
    })?;
    let encoded = proof.encode().map_err(invalid_input)?;
    write_via_temporary(&cli.output, &encoded)?;
    println!(
        "certified H1 intervention: {:?}, {} edits, lower bound {}, upper bound {}, wrote {} bytes to {}",
        intervention.status,
        intervention.edits.len(),
        intervention.lower_bound,
        intervention
            .upper_bound
            .expect("a feasible intervention has an upper bound"),
        encoded.len(),
        cli.output.display()
    );
    Ok(())
}

fn validate_intervention_budget(budget: usize) -> crate::Result<()> {
    if budget == 0 {
        return Err(crate::Error::InvalidInput(
            "--budget must be at least 1 when an output artifact is requested".into(),
        ));
    }
    Ok(())
}

fn read_intervention_program(cli: &InterveneCli) -> crate::Result<crate::PersistenceProgram> {
    let bytes = read_bounded_artifact(&cli.program, cli.max_artifact_bytes, "persistence program")?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact = ProgramArtifact::decode(&bytes, program_limits(cli.max_artifact_bytes), limits)
        .map_err(invalid_input)?;
    let matrix = read_proof_input(&cli.input, cli.format, cli.threads, artifact.threshold())?;
    artifact.verify(&matrix, limits).map_err(invalid_input)
}

fn intervention_target(
    program: &crate::PersistenceProgram,
    space: usize,
) -> crate::Result<crate::IntervalGroupId> {
    program
        .result()
        .spaces
        .get(space)
        .map(|target| target.id)
        .ok_or_else(|| {
            crate::Error::InvalidInput(format!(
                "H1 class-space index {} is out of range for {} spaces",
                space,
                program.result().spaces.len()
            ))
        })
}

pub(super) fn run_verify_intervention(cli: VerifyInterventionCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(
        &cli.artifact,
        cli.max_artifact_bytes,
        "persistence intervention",
    )?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact = InterventionArtifact::decode(
        &bytes,
        InterventionDecodeLimits {
            max_bytes: cli.max_artifact_bytes,
            max_trace_bytes: cli.max_artifact_bytes,
            trace: program_trace_limits(cli.max_artifact_bytes),
            ..InterventionDecodeLimits::default()
        },
        limits,
    )
    .map_err(invalid_input)?;
    let verified = artifact.verify(limits).map_err(invalid_input)?;
    println!(
        "verified H1 intervention: {:?}, {} edits, lower bound {}, upper bound {}",
        verified.status,
        verified.edits.len(),
        verified.lower_bound,
        verified.upper_bound
    );
    Ok(())
}
