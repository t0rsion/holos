use crate::Diagram;
use crate::certificate::{CertificateError, ChangeColumn};

use super::super::model::{InterfaceCancellation, InterfaceCell, RelativeInterfaceCertificate};
use super::{F64_BITS_CODEC, MAGIC, VERSION};

pub(super) fn encode(
    certificate: &RelativeInterfaceCertificate,
    limits: crate::certificate::CertificateLimits,
) -> Result<Vec<u8>, CertificateError> {
    certificate.verify(limits)?;
    let mut output = Vec::new();
    encode_interface_header(&mut output, certificate)?;
    encode_usizes(&mut output, &certificate.protected_vertices)?;
    encode_cells(&mut output, &certificate.input_cells)?;
    encode_cancellations(&mut output, &certificate.cancellations)?;
    encode_cells(&mut output, &certificate.core_cells)?;
    encode_columns(&mut output, &certificate.columns)?;
    encode_diagram(&mut output, &certificate.diagram)?;
    output.extend_from_slice(&certificate.digest);
    check_encoded_size(output.len(), limits.max_bytes)?;
    Ok(output)
}

fn encode_interface_header(
    output: &mut Vec<u8>,
    certificate: &RelativeInterfaceCertificate,
) -> Result<(), CertificateError> {
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(F64_BITS_CODEC);
    put_usize(output, certificate.max_dim)?;
    output.extend_from_slice(&certificate.modulus.to_be_bytes());
    put_usize(output, certificate.protected_vertices.len())?;
    Ok(())
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<(), CertificateError> {
    for &value in values {
        put_usize(output, value)?;
    }
    Ok(())
}

fn encode_cancellations(
    output: &mut Vec<u8>,
    cancellations: &[InterfaceCancellation],
) -> Result<(), CertificateError> {
    put_usize(output, cancellations.len())?;
    for step in cancellations {
        encode_cancellation(output, step)?;
    }
    Ok(())
}

fn encode_cancellation(
    output: &mut Vec<u8>,
    step: &InterfaceCancellation,
) -> Result<(), CertificateError> {
    encode_key(output, &step.upper)?;
    encode_key(output, &step.lower)?;
    output.extend_from_slice(&step.coefficient.to_be_bytes());
    Ok(())
}

fn encode_columns(
    output: &mut Vec<u8>,
    columns: &[Vec<ChangeColumn>],
) -> Result<(), CertificateError> {
    put_usize(output, columns.len())?;
    for dimension in columns {
        encode_column_dimension(output, dimension)?;
    }
    Ok(())
}

fn encode_column_dimension(
    output: &mut Vec<u8>,
    columns: &[ChangeColumn],
) -> Result<(), CertificateError> {
    put_usize(output, columns.len())?;
    for column in columns {
        encode_change_column(output, column)?;
    }
    Ok(())
}

fn encode_change_column(
    output: &mut Vec<u8>,
    column: &ChangeColumn,
) -> Result<(), CertificateError> {
    put_usize(output, column.terms.len())?;
    for term in &column.terms {
        put_usize(output, term.index)?;
        output.extend_from_slice(&term.coefficient.to_be_bytes());
    }
    Ok(())
}

fn encode_diagram(output: &mut Vec<u8>, diagram: &Diagram) -> Result<(), CertificateError> {
    put_usize(output, diagram.bars.len())?;
    for bar in &diagram.bars {
        put_usize(output, bar.dim)?;
        output.extend_from_slice(&bar.birth.to_bits().to_be_bytes());
        output.extend_from_slice(&bar.death.to_bits().to_be_bytes());
    }
    Ok(())
}

pub(super) fn check_encoded_size(actual: usize, limit: usize) -> Result<(), CertificateError> {
    if actual > limit {
        return Err(CertificateError::new(format!(
            "relative interface has {actual} bytes, above the limit {limit}"
        )));
    }
    Ok(())
}
fn encode_cells(
    output: &mut Vec<u8>,
    cells: &[Vec<InterfaceCell>],
) -> Result<(), CertificateError> {
    put_usize(output, cells.len())?;
    for dimension in cells {
        put_usize(output, dimension.len())?;
        for cell in dimension {
            encode_key(output, &cell.vertices)?;
            output.extend_from_slice(&cell.value.to_bits().to_be_bytes());
            put_usize(output, cell.boundary.len())?;
            for term in &cell.boundary {
                encode_key(output, &term.cell)?;
                output.extend_from_slice(&term.coefficient.to_be_bytes());
            }
        }
    }
    Ok(())
}

fn encode_key(output: &mut Vec<u8>, key: &[usize]) -> Result<(), CertificateError> {
    put_usize(output, key.len())?;
    for vertex in key {
        put_usize(output, *vertex)?;
    }
    Ok(())
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), CertificateError> {
    let value = u64::try_from(value)
        .map_err(|_| CertificateError::new("relative interface integer does not fit u64"))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}
