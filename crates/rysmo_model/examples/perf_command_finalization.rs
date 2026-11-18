//! CPU timings for hierarchy validation and commands on a large shared model.
use rysmo_math::{DMat4, DVec3};
use rysmo_model::{Command, Definition, DefinitionId, Document, Editor, Element, Instance, InstanceId};
use std::{hint::black_box, time::Instant};

fn fixture() -> Document {
    let mut doc = Document::new();
    let definition = DefinitionId(doc.reserve_id());
    doc.definitions.insert(definition, Definition::default().into());
    for index in 0..50_000 {
        let id = InstanceId(doc.reserve_id());
        doc.root_mut().instances.insert(
            id,
            Instance {
                definition,
                transform: DMat4::from_translation(DVec3::X * index as f64),
                name: String::new(),
                tag: None,
                material: None,
                hidden: false,
                glued_to: None,
                attributes: Default::default(),
            },
        );
    }
    doc
}

fn measure(mut run: impl FnMut()) -> serde_json::Value {
    let mut samples = Vec::new();
    for index in 0..44 {
        let start = Instant::now();
        run();
        if index >= 3 {
            samples.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    samples.sort_by(f64::total_cmp);
    serde_json::json!({"median_ms": samples[20], "p95_ms": samples[38]})
}

fn main() {
    let mut doc = fixture();
    let warm = measure(|| black_box(rysmo_model::limits::validate_document(&doc)).unwrap());
    let id = *doc.root.instances.keys().next().unwrap();
    let changed = measure(|| {
        let instance = doc.root_mut().instances.get_mut(&id).unwrap();
        instance.hidden = !instance.hidden;
        black_box(rysmo_model::limits::validate_document(&doc)).unwrap();
    });
    let mut editor = Editor::with_document(doc);
    let command = measure(|| {
        black_box(editor.apply(Command::Move { elements: vec![Element::Instance(id)], offset: DVec3::Y }).unwrap());
    });
    assert!(editor.undo());
    assert!(editor.redo());
    println!(
        "{}",
        serde_json::json!({"instances": 50_000, "samples": 41,
        "hierarchy_warm": warm, "hierarchy_hidden_change": changed, "move_one": command,
        "debug_assertions": cfg!(debug_assertions)})
    );
}
