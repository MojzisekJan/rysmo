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
        let validate_geometry = measure(|| {
            black_box(&doc.root.mesh).validate()?;
            for definition in doc.definitions.values() {
                black_box(&definition.mesh).validate()?;
            }
            Ok(())
        })?;
        let save_native = measure(|| {
            black_box(rysmo_io::save(black_box(&doc)));
            Ok(())
        })?;
        let saved = rysmo_io::native::SavedText::new(&doc);
        let camera = rysmo_math::OrbitCamera { yaw: 0.123456789123, ..Default::default() };
        let mut viewed = doc.clone();
        viewed.camera = Some(camera);
        assert_eq!(saved.with_camera(camera).text().as_str(), rysmo_io::save(&viewed));
        let update_saved_camera = measure(|| {
            black_box(saved.with_camera(black_box(camera)));
            Ok(())
        })?;
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
            "load":load,"validate_geometry":validate_geometry,"save_native":save_native,"update_saved_camera":update_saved_camera,"serialize_for_worker_transfer":encode,"reconstruct_on_recipient":reconstruct})
        );
    }
    Ok(())
}
