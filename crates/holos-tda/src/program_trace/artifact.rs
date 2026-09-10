use crate::{
    CertificateLimits, ProgramArtifact, ProgramUpdateMode, RipsParams, SparseDistanceMatrix,
};

use super::codec::{Reader, program_artifact_error};
use super::envelope::{
    STEP_MINIMUM_BYTES, check_count_bytes, check_envelope_size, check_no_trailing_bytes,
    decode_program_artifact, decode_trace_header, decode_trace_steps, encode_checkpoints,
    encode_program_artifact, encode_trace_header, encode_trace_step,
};
use super::model::{
    ProgramTraceArtifact, ProgramTraceDecodeLimits, ProgramTraceError, ProgramTraceStep,
    TraceTotals, VerifiedProgramTrace,
};
use super::records::{decode_graph, encode_graph};
use super::replay::{check_replayed_step, check_step_shape, replay_step, verify_program_artifact};

impl ProgramTraceArtifact {
    /// Produce a trace from an initial graph and updated graphs.
    pub fn build(
        initial: &SparseDistanceMatrix,
        updates: &[SparseDistanceMatrix],
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramTraceError> {
        let (initial_program, mut program) =
            ProgramArtifact::compile(initial, params, certificate_limits)
                .map_err(program_artifact_error)?;
        let mut steps = Vec::with_capacity(updates.len());
        for graph in updates {
            let update = program
                .advance(graph)
                .map_err(|error| ProgramTraceError::new(error.to_string()))?;
            let checkpoint = (update.mode != ProgramUpdateMode::Reused)
                .then(|| ProgramArtifact::capture(graph, &program))
                .transpose()
                .map_err(program_artifact_error)?;
            steps.push(ProgramTraceStep {
                graph: graph.clone(),
                mode: update.mode,
                work: update.work,
                events: update.events,
                continuation: update.continuation,
                correspondence: update.correspondence,
                diagram: update.result.diagram,
                checkpoint,
            });
        }
        Ok(Self {
            initial_graph: initial.clone(),
            initial_program,
            steps,
        })
    }

    /// Initial graph embedded in the trace.
    pub fn initial_graph(&self) -> &SparseDistanceMatrix {
        &self.initial_graph
    }

    /// Initial program artifact.
    pub fn initial_program(&self) -> &ProgramArtifact {
        &self.initial_program
    }

    /// Ordered update steps.
    pub fn steps(&self) -> &[ProgramTraceStep] {
        &self.steps
    }

    /// Encode the canonical `HOLOSDLT` version 2 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, ProgramTraceError> {
        let initial_program = encode_program_artifact(&self.initial_program)?;
        let checkpoints = encode_checkpoints(&self.steps)?;
        let mut out = Vec::new();
        encode_trace_header(&mut out, self.steps.len(), initial_program.len())?;
        encode_graph(&mut out, &self.initial_graph)?;
        out.extend_from_slice(&initial_program);
        for (step, checkpoint) in self.steps.iter().zip(checkpoints) {
            encode_trace_step(&mut out, step, checkpoint.as_deref())?;
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded program trace.
    pub fn decode(
        bytes: &[u8],
        limits: ProgramTraceDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramTraceError> {
        check_envelope_size(bytes, limits.max_bytes)?;
        let mut reader = Reader::new(bytes);
        let header = decode_trace_header(&mut reader, limits)?;
        let mut total_edges = 0usize;
        let initial_graph = decode_graph(&mut reader, limits, &mut total_edges)?;
        let initial_program = decode_program_artifact(
            &mut reader,
            header.initial_program_bytes,
            limits,
            certificate_limits,
        )?;
        let mut totals = TraceTotals {
            checkpoint_bytes: header.initial_program_bytes,
            ..TraceTotals::default()
        };
        check_count_bytes(
            &reader,
            header.step_count,
            STEP_MINIMUM_BYTES,
            "trace step records",
        )?;
        let steps = decode_trace_steps(
            &mut reader,
            header.step_count,
            limits,
            certificate_limits,
            initial_program.modulus(),
            &mut totals,
            &mut total_edges,
        )?;
        check_no_trailing_bytes(&reader)?;
        Ok(Self {
            initial_graph,
            initial_program,
            steps,
        })
    }

    /// Verify every reused step and every checkpoint.
    pub fn verify(
        &self,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<VerifiedProgramTrace, ProgramTraceError> {
        let mut program = verify_program_artifact(
            &self.initial_program,
            &self.initial_graph,
            certificate_limits,
        )?;
        let initial_result = program.result().clone();
        let mut verified_steps = Vec::with_capacity(self.steps.len());
        for (index, step) in self.steps.iter().enumerate() {
            check_step_shape(step)?;
            let checked = replay_step(&mut program, step, index, certificate_limits)?;
            check_replayed_step(&checked, step, index)?;
            verified_steps.push(checked.into());
        }
        Ok(VerifiedProgramTrace {
            initial_result,
            steps: verified_steps,
            final_program: program,
        })
    }
}
