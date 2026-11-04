//! CPU timings for hierarchy traversal used by viewport annotations.
//! Run with `cargo run -p rysmo_model --example perf_annotations -- 10000 50000`.
use rysmo_model::{Definition, DefinitionId, Document, Instance, InstanceId};
use std::{hint::black_box, time::Instant};

fn fixture(count: usize) -> Document {
    let mut doc = Document::new();
    doc.definitions.insert(DefinitionId(1), Definition::default().into());
    for index in 0..count {
        doc.root_mut().instances.insert(
            InstanceId(index as u64 + 2),
            Instance {
                definition: DefinitionId(1),
                transform: Default::default(),
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
fn measure(name: &str, mut run: impl FnMut()) {
    for _ in 0..3 {
        run();
    }
    let mut samples = Vec::new();
    for _ in 0..31 {
        let start = Instant::now();
        run();
        samples.push(start.elapsed().as_secs_f64() * 1000.);
    }
    samples.sort_by(f64::total_cmp);
    println!("{name}: median {:.6} ms, p95 {:.6} ms", samples[15], samples[29]);
}
fn main() {
    let counts: Vec<usize> = std::env::args().skip(1).map(|arg| arg.parse().expect("instance count")).collect();
    for count in if counts.is_empty() { vec![10000, 50000] } else { counts } {
        let mut doc = fixture(count);
        println!("{count} visible instances without annotations");
        measure("full hierarchy twice", || {
            black_box(doc.occurrences(&[]));
            black_box(doc.occurrences(&[]));
        });
        measure("annotation hierarchy once", || {
            black_box(doc.annotation_occurrences(&[]));
        });
        let text = rysmo_model::text::TextDefaults::default()
            .screen("Note".into(), Default::default(), [800., 600.].into())
            .unwrap();
        doc.root_mut().texts.insert(rysmo_model::TextId(count as u64 + 2), text.clone());
        println!("{count} visible instances with one root annotation");
        measure("full hierarchy twice", || {
            black_box(doc.occurrences(&[]));
            black_box(doc.occurrences(&[]));
        });
        measure("annotation hierarchy once", || {
            black_box(doc.annotation_occurrences(&[]));
        });
        doc.def_mut(Some(DefinitionId(1))).unwrap().texts.insert(rysmo_model::TextId(count as u64 + 3), text);
        println!("{count} visible instances with annotations on every instance");
        measure("full hierarchy twice", || {
            black_box(doc.occurrences(&[]));
            black_box(doc.occurrences(&[]));
        });
        measure("annotation hierarchy once", || {
            black_box(doc.annotation_occurrences(&[]));
        });
    }
}
