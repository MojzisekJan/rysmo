//! CPU-only timings and snapshot sharing for edits with many annotations.
use rysmo_math::{DVec2, DVec3};
use rysmo_model::dimensions::{Dimension, DimensionAnchor, DimensionMeasure};
use rysmo_model::{Command, Definition, DefinitionId, DimensionId, Document, Editor, TextId, Units};
use std::{hint::black_box, sync::Arc, time::Instant};

fn fixture() -> Document {
    let mut document = Document::new();
    for _ in 0..100 {
        let id = DefinitionId(document.reserve_id());
        let mut definition = Definition::default();
        for _ in 0..50 {
            let text = rysmo_model::text::TextDefaults::default()
                .screen("Annotation".repeat(8), DVec2::ZERO, DVec2::new(800., 600.))
                .unwrap();
            definition.texts.insert(TextId(document.reserve_id()), text);
            definition.dimensions.insert(
                DimensionId(document.reserve_id()),
                Dimension {
                    measure: DimensionMeasure::Linear {
                        from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                        to: DimensionAnchor { point: DVec3::X, reference: None },
                        axis: None,
                    },
                    normal: DVec3::Z,
                    offset: DVec3::Y,
                    text: String::new(),
                    alignment: Default::default(),
                    font: Default::default(),
                    text_position: Default::default(),
                    endpoints: Default::default(),
                },
            );
        }
        document.definitions.insert(id, definition.into());
    }
    document
}

fn main() {
    let document = fixture();
    let mut editor = Editor::with_document(document.clone());
    let mut samples = Vec::new();
    for index in 0..44 {
        let units = if index % 2 == 0 { Units::Metres } else { Units::Centimetres };
        let started = Instant::now();
        black_box(editor.apply(Command::SetUnits { units }).unwrap());
        if index >= 3 {
            samples.push(started.elapsed().as_secs_f64() * 1000.);
        }
    }
    samples.sort_by(f64::total_cmp);
    let shared = document
        .definitions
        .iter()
        .filter(|(id, original)| Arc::ptr_eq(original, &editor.doc().definitions[id]))
        .count();
    assert_eq!(editor.doc().definitions, document.definitions);
    assert!(editor.undo());
    assert_eq!(editor.doc().units, Units::Metres);
    assert!(editor.redo());
    assert_eq!(editor.doc().units, Units::Centimetres);
    println!(
        "{}",
        serde_json::json!({
            "definitions": 100, "texts": 5000, "dimensions": 5000,
            "samples": samples.len(), "median_ms": samples[samples.len() / 2],
            "p95_ms": samples[(samples.len() * 95).div_ceil(100) - 1],
            "definitions_shared_with_initial": shared,
            "debug_assertions": cfg!(debug_assertions),
        })
    );
}
