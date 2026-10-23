//! Imports a model file and writes it as a native model on a plain ground, scaled to a given
//! length along Y and centred, for side-by-side comparison renders.
use rysmo_math::{DVec3, OrbitCamera};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, length] = args.as_slice() else {
        return Err("Usage: import_view input.glb output.rysmo length".into());
    };
    let bytes = std::fs::read(input)?;
    let imported = rysmo_io::import::read(input, &bytes, Default::default())?;
    let mut e = rysmo_model::Editor::with_document(imported.document);
    let (lo, hi) = e.doc().definition_bounds(None).ok_or("empty model")?;
    let size = hi - lo;
    let all = |e: &rysmo_model::Editor| -> Vec<rysmo_model::Element> {
        let mut v: Vec<_> = e.active().instances.keys().map(|i| rysmo_model::Element::Instance(*i)).collect();
        v.extend(e.mesh().faces().map(|(f, _)| rysmo_model::Element::Face(f)));
        v.extend(e.mesh().edges().map(|(x, _)| rysmo_model::Element::Edge(x)));
        v
    };
    let centre = DVec3::new((lo.x + hi.x) / 2., (lo.y + hi.y) / 2., lo.z);
    e.apply(rysmo_model::Command::Move { elements: all(&e), offset: -centre })?;
    // Turn the longest horizontal axis onto Y and scale it to the requested length.
    if size.x > size.y {
        e.apply(rysmo_model::Command::Rotate {
            elements: all(&e),
            centre: DVec3::ZERO,
            axis: DVec3::Z,
            angle: 90.,
            copies: 0,
        })?;
    }
    let factor = length.parse::<f64>()? / size.x.max(size.y);
    e.apply(rysmo_model::Command::Scale { elements: all(&e), origin: DVec3::ZERO, factors: DVec3::splat(factor) })?;
    let mut doc = e.doc().clone();
    for w in &imported.warnings {
        eprintln!("{w}");
    }
    doc.style.lighting = rysmo_model::Lighting::Sun;
    doc.style.shadows = true;
    doc.style.edges = false;
    doc.style.ground = true;
    doc.style.ground_color = [0.42, 0.42, 0.41];
    doc.style.sky = true;
    doc.style.exposure = 3.;
    doc.style.sun.day = 172;
    doc.style.sun.hour = 15.;
    doc.camera = Some(OrbitCamera { target: DVec3::new(0., 0., 0.9), distance: 9., fov_y: 0.6, ..Default::default() });
    std::fs::write(output, rysmo_io::save(&doc))?;
    Ok(())
}
