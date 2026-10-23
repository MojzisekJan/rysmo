//! Writes a stand-alone Cybertruck model on a plain ground, for checking it against photos.
#[path = "shared/cybertruck.rs"]
mod cybertruck;

use rysmo_math::{DVec3, OrbitCamera};
use rysmo_model::Editor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("Usage: cybertruck output.rysmo")?;
    let mut e = Editor::new();
    cybertruck::build(&mut e, DVec3::ZERO)?;
    let mut doc = e.doc().clone();
    doc.style.lighting = rysmo_model::Lighting::Sun;
    doc.style.shadows = true;
    doc.style.edges = false;
    doc.style.ground = true;
    doc.style.ground_color = [0.42, 0.42, 0.41];
    doc.style.sky = true;
    doc.style.tone_mapping = rysmo_model::ToneMapping::TonyMcMapface;
    doc.style.exposure = 3.;
    doc.style.sun.day = 172;
    doc.style.sun.hour = 15.;
    doc.camera = Some(OrbitCamera {
        target: DVec3::new(0., 0., 0.9),
        distance: 9.,
        yaw: -2.2,
        pitch: 0.15,
        fov_y: 0.6,
        ..Default::default()
    });
    std::fs::write(&path, rysmo_io::save(&doc))?;
    Ok(())
}
