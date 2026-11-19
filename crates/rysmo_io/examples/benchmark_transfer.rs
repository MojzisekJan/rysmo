//! Native CPU proxy for worker-result serialization and main-thread reconstruction.
//! Excludes worker startup, JS/WASM copies, disk reads and rendering.
use std::{error::Error, hint::black_box, time::Instant};

fn measure(mut operation: impl FnMut() -> Result<(), Box<dyn Error>>) -> Result<serde_json::Value, Box<dyn Error>> {
    let mut samples = Vec::new();
    for i in 0..24 {
        let start = Instant::now();
        operation()?;
        if i >= 3 {
            samples.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    samples.sort_by(f64::total_cmp);
    Ok(serde_json::json!({"median_ms":samples[10],"p95_ms":samples[19],"samples":21}))
}

fn main() -> Result<(), Box<dyn Error>> {
    for path in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&path)?;
        let doc = rysmo_io::load(&text)?;
        let transfer = serde_json::to_string(&doc)?;
        let reconstructed: rysmo_model::Document = serde_json::from_str(&transfer)?;
        assert_eq!(rysmo_io::save(&reconstructed), rysmo_io::save(&doc));
        let load = measure(|| {
            black_box(rysmo_io::load(black_box(&text))?);
            Ok(())
        })?;
        let encode = measure(|| {
            black_box(serde_json::to_string(black_box(&doc))?);
            Ok(())
        })?;
        let reconstruct = measure(|| {
            black_box(serde_json::from_str::<rysmo_model::Document>(black_box(&transfer))?);
            Ok(())
        })?;
        println!(
            "{}",
            serde_json::json!({"file":path,"native_bytes":text.len(),"transfer_bytes":transfer.len(),
            "load":load,"serialize_for_worker_transfer":encode,"reconstruct_on_recipient":reconstruct})
        );
    }
    Ok(())
}
