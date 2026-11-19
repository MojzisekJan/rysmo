//! CPU-only native loading, including validation and disposal, from an in-memory file.
use std::{error::Error, hint::black_box, time::Instant};
fn main() -> Result<(), Box<dyn Error>> {
    for path in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&path)?;
        let doc = rysmo_io::load(&text)?;
        let saved = rysmo_io::save(&doc);
        assert_eq!(rysmo_io::save(&rysmo_io::load(&saved)?), saved);
        let mut samples = Vec::new();
        for i in 0..24 {
            let start = Instant::now();
            black_box(rysmo_io::load(black_box(&text))?);
            if i >= 3 {
                samples.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({"file":path,"bytes":text.len(),"samples":samples.len(),"median_ms":samples[10],"p95_ms":samples[19]})
        );
    }
    Ok(())
}
