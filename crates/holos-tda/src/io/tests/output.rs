use super::super::*;
use crate::{Bar, Diagram};

fn sample_diagram() -> Diagram {
    let mut diagram = Diagram {
        bars: vec![
            Bar {
                dim: 0,
                birth: 0.0,
                death: f64::INFINITY,
            },
            Bar {
                dim: 0,
                birth: 0.0,
                death: 0.25,
            },
            Bar {
                dim: 1,
                birth: 0.5,
                death: 1.0,
            },
        ],
    };
    diagram.canonicalize();
    diagram
}

#[test]
fn ripser_output_format() {
    let mut out = Vec::new();
    write_diagram(&mut out, &sample_diagram(), OutputFormat::Ripser, 1).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "persistence intervals in dim 0:\n [0,0.25)\n [0, )\npersistence intervals in dim 1:\n [0.5,1)\n"
    );
}

#[test]
fn csv_output_format() {
    let mut out = Vec::new();
    write_diagram(&mut out, &sample_diagram(), OutputFormat::Csv, 1).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "dim,birth,death\n0,0,0.25\n0,0,inf\n1,0.5,1\n"
    );
}

#[test]
fn empty_diagram_ripser_output_prints_headers_only() {
    let mut out = Vec::new();
    write_diagram(&mut out, &Diagram::default(), OutputFormat::Ripser, 1).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(
        text,
        "persistence intervals in dim 0:\npersistence intervals in dim 1:\n"
    );
}
