//! The demo scene's Tesla Cybertruck: "Tesla Cybertruck" by PolyDucky
//! (https://sketchfab.com/3d-models/tesla-cybertruck-1ab0c3f248fa47019c4311abce9731a0),
//! licensed under CC-BY-4.0; see `assets/cybertruck/LICENSE.txt`. The model is imported with
//! Rysmo's glTF importer, scaled to the production truck (5.68 m long) and given brushed
//! stainless and satin black surfaces.
use rysmo_math::{DMat4, DVec3};
use rysmo_model::{Command, Editor, clipboard::Clipboard};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MODEL: &[u8] = include_bytes!("../assets/cybertruck/cybertruck.glb");
const LENGTH: f64 = 5.683;

/// Surfaces by source material name: sRGB colour, roughness, metalness.
fn surface(name: &str) -> Option<([f32; 4], f32, f32)> {
    Some(match name {
        "body" => ([0.68, 0.69, 0.7, 1.], 0.38, 1.),
        "gray" => ([0.2, 0.205, 0.21, 1.], 0.85, 0.),
        "glass" => ([0.05, 0.055, 0.06, 0.9], 0.04, 0.),
        "glassframes" | "black" | "shader" => ([0.04, 0.04, 0.045, 1.], 0.5, 0.),
        "tires" => ([0.035, 0.035, 0.035, 1.], 0.92, 0.),
        "rims" => ([0.07, 0.07, 0.075, 1.], 0.3, 0.3),
        "lights" => ([0.96, 0.96, 0.98, 1.], 0.15, 0.),
        "warninglights" => ([0.6, 0.03, 0.02, 1.], 0.2, 0.),
        _ => return None,
    })
}

/// Places the truck with its nose towards -Y, the middle of its length at `at`.
pub fn build(e: &mut Editor, at: DVec3) -> Result<()> {
    let imported = rysmo_io::import::read("cybertruck.glb", MODEL, Default::default())?;
    let mut document = imported.document;
    for material in document.materials.values_mut() {
        if let Some((color, roughness, metallic)) = surface(&material.name) {
            material.color = color;
            material.pbr = rysmo_model::Pbr { roughness, metallic, ..Default::default() };
        }
    }
    let (lo, hi) = document.definition_bounds(None).ok_or("empty truck model")?;
    let size = hi - lo;
    // Turn the length onto Y.
    let turn = if size.x > size.y { DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2) } else { DMat4::IDENTITY };
    let scale = LENGTH / size.x.max(size.y);
    let centre = DVec3::new((lo.x + hi.x) / 2., (lo.y + hi.y) / 2., lo.z);
    let mut transform =
        DMat4::from_translation(at) * DMat4::from_scale(DVec3::splat(scale)) * turn * DMat4::from_translation(-centre);
    // The nose is the lower end; keep it towards -Y.
    let ends = |t: &DMat4| {
        let points: Vec<DVec3> = document
            .occurrences(&[])
            .iter()
            .flat_map(|o| {
                let m = *t * o.transform;
                document
                    .def(o.definition)
                    .map(|d| d.mesh.vertices().map(|(_, v)| m.transform_point3(v.pos)).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .collect();
        let top = |front: bool| {
            points
                .iter()
                .filter(|p| if front { p.y < at.y - 2.4 } else { p.y > at.y + 2.4 })
                .map(|p| p.z)
                .fold(f64::MIN, f64::max)
        };
        (top(true), top(false))
    };
    let (front, back) = ends(&transform);
    if front > back {
        transform = DMat4::from_translation(at)
            * DMat4::from_rotation_z(std::f64::consts::PI)
            * DMat4::from_translation(-at)
            * transform;
    }
    e.apply(Command::Paste {
        clipboard: Box::new(Clipboard { document, source_transform: DMat4::IDENTITY }),
        transform,
        reuse_definitions: false,
    })?;
    Ok(())
}
