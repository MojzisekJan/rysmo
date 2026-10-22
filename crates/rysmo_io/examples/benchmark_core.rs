//! CPU-only model timings. This does not measure browser frames or GPU memory.
use std::{error::Error, hint::black_box, path::Path, time::Instant};

use rysmo_math::DVec3;
use rysmo_model::{Command, Editor, Element};
use serde::Serialize;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const SAMPLES: usize = 21;

#[derive(Default)]
struct Samples(Vec<f64>);
#[derive(Serialize)]
struct Timing {
    median_ms: f64,
    p95_ms: f64,
}
impl Samples {
    fn record<T>(&mut self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let start = Instant::now();
        let result = operation()?;
        self.0.push(start.elapsed().as_secs_f64() * 1000.);
        Ok(result)
    }
    fn finish(mut self) -> Timing {
        self.0.sort_by(f64::total_cmp);
        Timing { median_ms: self.0[self.0.len() / 2], p95_ms: self.0[(self.0.len() * 95).div_ceil(100) - 1] }
    }
}
#[derive(Serialize)]
struct Report {
    file: String,
    bytes: usize,
    occurrences: usize,
    occurrence_faces: usize,
    samples: usize,
    debug_assertions: bool,
    load: Timing,
    save: Timing,
    clone_and_drop: Timing,
    move_instance: Timing,
    undo: Timing,
    redo: Timing,
}
fn benchmark(path: &Path) -> Result<Report> {
    let text = std::fs::read_to_string(path)?;
    let document = rysmo_io::load(&text)?;
    if rysmo_io::load(&rysmo_io::save(&document))? != document {
        return Err("model changed during the save/load round trip".into());
    }
    let instance = *document.root.instances.keys().next().ok_or("benchmark needs a root instance")?;
    let occurrences = document.occurrences(&[]);
    let occurrence_faces = occurrences.iter().map(|o| document.effective_mesh(o.definition).face_count()).sum();
    let mut load = Samples::default();
    let mut save = Samples::default();
    let mut clone = Samples::default();
    let mut movement = Samples::default();
    let mut undo = Samples::default();
    let mut redo = Samples::default();
    // Warm allocation and document caches before recording samples.
    for _ in 0..3 {
        black_box(rysmo_io::load(&text)?);
        black_box(rysmo_io::save(&document));
    }
    for _ in 0..SAMPLES {
        // Return values are dropped inside these samples, including their allocation cost.
        load.record(|| {
            black_box(rysmo_io::load(black_box(&text))?);
            Ok(())
        })?;
        save.record(|| {
            black_box(rysmo_io::save(black_box(&document)));
            Ok(())
        })?;
        clone.record(|| {
            black_box(black_box(&document).clone());
            Ok(())
        })?;
        // Initial editor construction and equality checks are outside the edit timings.
        let mut editor = Editor::with_document(document.clone());
        movement.record(|| {
            editor
                .apply(Command::Move { elements: vec![Element::Instance(instance)], offset: DVec3::X * 0.1 })
                .map_err(Into::into)
        })?;
        let moved = editor.doc().clone();
        if !undo.record(|| Ok(editor.undo()))? || editor.doc() != &document {
            return Err("Undo did not restore the original model".into());
        }
        if !redo.record(|| Ok(editor.redo()))? || editor.doc() != &moved {
            return Err("Redo did not restore the moved model".into());
        }
    }
    Ok(Report {
        file: path.display().to_string(),
        bytes: text.len(),
        occurrences: occurrences.len(),
        occurrence_faces,
        samples: SAMPLES,
        debug_assertions: cfg!(debug_assertions),
        load: load.finish(),
        save: save.finish(),
        clone_and_drop: clone.finish(),
        move_instance: movement.finish(),
        undo: undo.finish(),
        redo: redo.finish(),
    })
}
fn main() -> Result<()> {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    if paths.is_empty() {
        return Err("Usage: cargo run -p rysmo_io --example benchmark_core -- model.rysmo...".into());
    }
    let reports = paths.iter().map(|p| benchmark(Path::new(p))).collect::<Result<Vec<_>>>()?;
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
