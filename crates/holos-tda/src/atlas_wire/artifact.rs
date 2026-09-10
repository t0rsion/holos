use crate::{
    CertificateLimits, Diagram, ExplainedDiagram, PersistenceAtlas, PersistentClassSpace,
    ReductionCertificate, RipsParams, SparseDistanceMatrix,
};

use super::codec::{
    Reader, critical_pair_record_order, critical_pair_records_bits_equal, diagram_bits_equal,
};
use super::model::{AtlasArtifact, AtlasArtifactError, AtlasArtifactRepair, AtlasDecodeLimits};
use super::validation::{
    check_critical_values, check_diagram_structure, check_input_binding, check_reduction_binding,
    check_space_order, check_spaces_structure, checked_threshold, full_graph_digest,
};
use super::wire::{
    decode_atlas_header, decode_atlas_prefix, decode_bars, decode_nested_certificate,
    decode_spaces, encode_atlas_header, encode_bars, encode_spaces, finish_atlas_decode,
    validate_atlas_size, validate_minimum_records,
};

impl AtlasArtifact {
    /// Produce an atlas artifact for an exact H0 and H1 run.
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, AtlasArtifactError> {
        Self::compile(input, params, certificate_limits).map(|(artifact, _)| artifact)
    }

    /// Produce an artifact and retain its ready-to-evaluate atlas.
    pub fn compile(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<(Self, PersistenceAtlas), AtlasArtifactError> {
        let atlas = PersistenceAtlas::build(input, params)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let reduction = ReductionCertificate::build(input, params, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let artifact = Self {
            vertex_count: input.len(),
            threshold: params.threshold,
            modulus: params.modulus,
            input_digest: *atlas.input_digest(),
            explained: atlas.explained().clone(),
            reduction,
        };
        artifact.check_structure(Some(input))?;
        Ok((artifact, atlas))
    }

    /// Adapt this artifact to changed weights on the same listed graph.
    ///
    /// The reduction reuses its longest valid simplex prefixes. Canonical
    /// class spaces are recomputed and checked against the repaired
    /// reduction.
    pub fn repair(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<AtlasArtifactRepair, AtlasArtifactError> {
        self.check_structure(Some(current))?;
        let repair = self
            .reduction
            .repair(current, updated, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let mut params = RipsParams::new(1).with_modulus(self.modulus);
        params.threshold = self.threshold;
        let explained = crate::rips_persistence_with_classes_sparse(updated, &params)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        if !diagram_bits_equal(repair.certificate().diagram(), &explained.diagram) {
            return Err(AtlasArtifactError::new(
                "repaired reduction differs from the canonical class reduction",
            ));
        }
        let mode = repair.mode();
        let work = repair.work();
        let artifact = Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            input_digest: full_graph_digest(updated, self.threshold),
            explained,
            reduction: repair.into_certificate(),
        };
        artifact.check_structure(Some(updated))?;
        Ok(AtlasArtifactRepair {
            artifact,
            mode,
            work,
        })
    }

    pub(crate) fn rebind(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        explained: ExplainedDiagram,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, AtlasArtifactError> {
        self.check_structure(Some(current))?;
        let reduction = self
            .reduction
            .reindex(current, updated, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        if !diagram_bits_equal(reduction.diagram(), &explained.diagram) {
            return Err(AtlasArtifactError::new(
                "reindexed reduction differs from the evaluated class result",
            ));
        }
        let artifact = Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            input_digest: full_graph_digest(updated, self.threshold),
            explained,
            reduction,
        };
        artifact.verify(updated, certificate_limits)?;
        Ok(artifact)
    }

    /// Diagram carried by the atlas.
    pub fn diagram(&self) -> &Diagram {
        &self.explained.diagram
    }

    /// Diagram, class spaces, cocycles, and critical pairs carried by the
    /// atlas.
    pub fn explained(&self) -> &ExplainedDiagram {
        &self.explained
    }

    /// Number of vertices bound to the atlas.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Canonical H1 class spaces carried by the atlas.
    pub fn spaces(&self) -> &[PersistentClassSpace] {
        &self.explained.spaces
    }

    /// Nested algebraic reduction certificate.
    pub fn reduction_certificate(&self) -> &ReductionCertificate {
        &self.reduction
    }

    /// Encode the canonical `HOLOSATL` version 2 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, AtlasArtifactError> {
        self.check_structure(None)?;
        let reduction = self
            .reduction
            .encode()
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let mut out = Vec::new();
        encode_atlas_header(&mut out, self, reduction.len())?;
        encode_bars(&mut out, &self.explained.diagram)?;
        encode_spaces(&mut out, &self.explained.spaces)?;
        out.extend_from_slice(&reduction);
        Ok(out)
    }

    /// Decode and structurally validate a bounded atlas envelope.
    pub fn decode(
        bytes: &[u8],
        limits: AtlasDecodeLimits,
        mut certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, AtlasArtifactError> {
        validate_atlas_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_atlas_prefix(&mut reader)?;
        let header = decode_atlas_header(&mut reader, limits)?;
        validate_minimum_records(&reader, &header)?;
        let bars = decode_bars(&mut reader, header.bars)?;
        let spaces = decode_spaces(&mut reader, &header, limits)?;
        let reduction = decode_nested_certificate(&mut reader, &header, &mut certificate_limits)?;
        finish_atlas_decode(&reader)?;
        let artifact = Self {
            vertex_count: header.vertex_count,
            threshold: header.threshold,
            modulus: header.modulus,
            input_digest: header.input_digest,
            explained: ExplainedDiagram {
                diagram: Diagram { bars },
                spaces,
            },
            reduction,
        };
        artifact.check_structure(None)?;
        Ok(artifact)
    }

    /// Verify the nested reduction and reconstruct the reusable atlas.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<PersistenceAtlas, AtlasArtifactError> {
        self.check_structure(Some(input))?;
        let (diagram, checked_pairs) = self
            .reduction
            .verify_with_h1_critical_pairs(input, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        if !diagram_bits_equal(&diagram, &self.explained.diagram) {
            return Err(AtlasArtifactError::new(
                "checked reduction differs from the atlas diagram",
            ));
        }
        let mut declared_pairs = Vec::new();
        for space in &self.explained.spaces {
            for pair in &space.critical_pairs {
                declared_pairs.push((space.interval, pair.clone()));
            }
        }
        declared_pairs.sort_by(critical_pair_record_order);
        if !critical_pair_records_bits_equal(&checked_pairs, &declared_pairs) {
            return Err(AtlasArtifactError::new(
                "checked reduction differs from the declared critical pairs",
            ));
        }
        PersistenceAtlas::from_checked_parts(
            input,
            self.modulus,
            self.threshold,
            self.explained.clone(),
        )
        .map_err(|error| AtlasArtifactError::new(error.to_string()))
    }

    fn check_structure(
        &self,
        input: Option<&SparseDistanceMatrix>,
    ) -> std::result::Result<(), AtlasArtifactError> {
        let threshold = checked_threshold(self.threshold)?;
        check_input_binding(self, input, threshold)?;
        check_reduction_binding(self)?;
        check_diagram_structure(&self.explained.diagram)?;
        check_spaces_structure(self, input)?;
        check_space_order(&self.explained.spaces)?;
        check_critical_values(input, &self.explained.spaces)
    }
}
