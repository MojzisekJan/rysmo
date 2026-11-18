//! Linux process RSS snapshots for native loading, saving and a full Undo history.
use rysmo_math::DVec3;
use rysmo_model::{Command, Editor, Element};
use std::{hint::black_box, time::Instant};
fn memory(stage: &str) {
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux /proc memory statistics");
    let get = |key: &str| {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u64>()
            .unwrap()
    };
    println!("{}", serde_json::json!({"stage": stage, "rss_kib": get("VmRSS:"), "peak_rss_kib": get("VmHWM:")}));
}
fn main() {
    let path = std::env::args().nth(1).expect("model path");
    memory("baseline");
    let text = std::fs::read_to_string(path).unwrap();
    memory("input");
    let started = Instant::now();
    let doc = rysmo_io::load(&text).unwrap();
    println!(
        "{}",
        serde_json::json!({"operation":"load", "milliseconds": started.elapsed().as_secs_f64()*1000., "input_bytes": text.len()})
    );
    memory("loaded_with_input");
    drop(text);
    memory("loaded");
    let started = Instant::now();
    let saved = rysmo_io::save(&doc);
    black_box(&saved);
    memory("saved_with_output");
    println!(
        "{}",
        serde_json::json!({"operation":"save", "milliseconds": started.elapsed().as_secs_f64()*1000., "output_bytes": saved.len()})
    );
    drop(saved);
    memory("saved");
    let mut editor = Editor::with_document(doc);
    let element = editor
        .doc()
        .root
        .instances
        .keys()
        .next()
        .map(|id| Element::Instance(*id))
        .or_else(|| editor.mesh().edges().next().map(|(id, _)| Element::Edge(id)))
        .expect("editable geometry");
    for _ in 0..rysmo_model::UNDO_LIMIT + 20 {
        editor.apply(Command::Move { elements: vec![element], offset: DVec3::Y * 0.01 }).unwrap();
    }
    assert_eq!(editor.undo_depth(), rysmo_model::UNDO_LIMIT);
    memory("full_history");
    for _ in 0..rysmo_model::UNDO_LIMIT {
        assert!(editor.undo());
    }
    memory("all_undone");
    for _ in 0..rysmo_model::UNDO_LIMIT {
        assert!(editor.redo());
    }
    memory("all_redone");
    editor.replace_document(rysmo_model::Document::new());
    memory("document_released");
}
