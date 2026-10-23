//! Seamless procedural surface textures for the demo scene, each with a matching normal map,
//! generated here so the scene carries no third-party images.
use rysmo_model::{Command, Editor, MaterialId, Pbr};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const N: usize = 512;

fn hash(x: i64, y: i64, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise repeating every `period` cells.
fn noise(x: f32, y: f32, period: i64, seed: u64) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let s = |t: f32| t * t * (3. - 2. * t);
    let c = |i: i64, j: i64| hash(i.rem_euclid(period), j.rem_euclid(period), seed);
    let (i, j) = (x0 as i64, y0 as i64);
    let a = c(i, j) + (c(i + 1, j) - c(i, j)) * s(fx);
    let b = c(i, j + 1) + (c(i + 1, j + 1) - c(i, j + 1)) * s(fx);
    a + (b - a) * s(fy)
}

/// Tiling fractal noise over the image, 0..1, starting at `base` cells across.
fn fbm(x: f32, y: f32, base: i64, octaves: u32, seed: u64) -> f32 {
    let (mut sum, mut amp, mut total) = (0., 0.5, 0.);
    for o in 0..octaves {
        let cells = base << o;
        let f = cells as f32 / N as f32;
        sum += amp * noise(x * f, y * f, cells, seed + o as u64);
        total += amp;
        amp *= 0.5;
    }
    sum / total
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0., 1.);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn shade(c: [f32; 3], k: f32) -> [f32; 3] {
    c.map(|v| v * k)
}

/// A surface: colour and height at each pixel.
struct Surface {
    color: Vec<[f32; 3]>,
    height: Vec<f32>,
}

fn surface(f: impl Fn(f32, f32) -> ([f32; 3], f32)) -> Surface {
    let (color, height) = (0..N * N).map(|i| f((i % N) as f32, (i / N) as f32)).unzip();
    Surface { color, height }
}

fn png(rgb: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, N as u32, N as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("PNG header");
    let data: Vec<u8> = rgb.iter().flat_map(|p| p.map(|c| (c.clamp(0., 1.) * 255.).round() as u8)).collect();
    w.write_image_data(&data).expect("PNG data");
    drop(w);
    out
}

/// A tangent-space normal map (OpenGL convention) from the height field, wrapping at edges.
fn normals(height: &[f32], strength: f32) -> Vec<[f32; 3]> {
    let at = |x: i64, y: i64| height[(y.rem_euclid(N as i64) as usize) * N + x.rem_euclid(N as i64) as usize];
    (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as i64, (i / N) as i64);
            let dx = (at(x + 1, y) - at(x - 1, y)) * strength;
            let dy = (at(x, y + 1) - at(x, y - 1)) * strength;
            let n = [-dx, dy, 1.];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            n.map(|v| v / l * 0.5 + 0.5)
        })
        .collect()
}

/// Adds a textured, normal-mapped material: `size` metres per tile.
#[allow(clippy::too_many_arguments)]
fn add(
    e: &mut Editor,
    name: &str,
    s: Surface,
    size: [f64; 2],
    roughness: f32,
    metallic: f32,
    bump: f32,
    tint: Option<[f32; 4]>,
) -> Result<MaterialId> {
    let n = s.color.len() as f32;
    let avg = s.color.iter().fold([0.; 3], |a, c| [a[0] + c[0] / n, a[1] + c[1] / n, a[2] + c[2] / n]);
    let color = tint.unwrap_or([avg[0], avg[1], avg[2], 1.]);
    let out = e.apply(Command::AddTexturedMaterial {
        name: name.into(),
        color,
        image_name: format!("{}.png", name.to_lowercase().replace(' ', "-")),
        mime: "image/png".into(),
        data: png(&s.color),
        size,
    })?;
    let material = out.material.ok_or("no material")?;
    let normal = e
        .apply(Command::AddImage {
            name: format!("{}-normal.png", name.to_lowercase().replace(' ', "-")),
            mime: "image/png".into(),
            data: png(&normals(&s.height, bump)),
        })?
        .image
        .ok_or("no image")?;
    let pbr = Pbr {
        roughness,
        metallic,
        normal_map: Some(normal),
        normal_scale: 1.,
        tint_texture: tint.is_some(),
        ..Default::default()
    };
    e.apply(Command::SetMaterialPbr { material, pbr })?;
    Ok(material)
}

/// Autumn lawn: olive and straw-coloured blades with darker, damp patches.
pub fn grass(e: &mut Editor) -> Result<MaterialId> {
    let s = surface(|x, y| {
        let patch = fbm(x, y, 4, 4, 1);
        let blades = noise(x * 0.9, y * 0.25, (N as f32 * 0.9) as i64, 2) * 0.6 + hash(x as i64, y as i64, 3) * 0.4;
        let dry = fbm(x, y, 16, 3, 4);
        let base = mix([0.30, 0.36, 0.16], [0.58, 0.54, 0.28], dry * 1.3 - 0.2);
        let c = shade(mix(base, [0.36, 0.30, 0.18], (patch - 0.6) * 3.), 0.72 + 0.5 * blades);
        (c, blades * 0.6 + patch * 0.4)
    });
    add(e, "Lawn", s, [1.2, 1.2], 0.95, 0., 3., None)
}

/// Asphalt: fine dark aggregate with lighter stones and slight wear.
pub fn asphalt(e: &mut Editor) -> Result<MaterialId> {
    let s = surface(|x, y| {
        let grain = hash(x as i64, y as i64, 11);
        let stones = noise(x / 3., y / 3., N as i64 / 3, 12);
        let wear = fbm(x, y, 4, 3, 13);
        let k = 0.75 + 0.35 * grain + if stones > 0.82 { 0.45 } else { 0. };
        (shade(mix([0.13, 0.13, 0.135], [0.2, 0.2, 0.2], wear), k), stones * 0.7 + grain * 0.3)
    });
    add(e, "Asphalt", s, [1.5, 1.5], 0.9, 0., 2.5, None)
}

/// Board-marked concrete: soft cloudy tone, small pores and faint board lines.
pub fn concrete(e: &mut Editor) -> Result<MaterialId> {
    let s = surface(|x, y| {
        let cloud = fbm(x, y, 6, 5, 21);
        let pore = if hash(x as i64, y as i64, 22) > 0.993 { 0.6 } else { 1. };
        let board = if (y as usize) % (N / 4) < 2 { 0.9 } else { 1. };
        (shade([0.62, 0.61, 0.58], (0.82 + 0.3 * cloud) * pore * board), cloud * 0.5 + (1. - pore) * 0.5)
    });
    add(e, "Concrete", s, [2.4, 2.4], 0.88, 0., 2., None)
}

/// Fine lime render: near white with subtle trowel texture.
pub fn render(e: &mut Editor) -> Result<MaterialId> {
    let s = surface(|x, y| {
        let t = fbm(x, y, 12, 5, 31);
        (shade([0.93, 0.92, 0.89], 0.94 + 0.08 * t), t)
    });
    add(e, "White render", s, [2., 2.], 0.92, 0., 1.5, None)
}

/// Vertical larch cladding: boards 125 mm wide with grooves, grain and weathering.
pub fn larch(e: &mut Editor) -> Result<MaterialId> {
    let boards = 8.;
    let s = surface(|x, y| {
        let u = x / N as f32 * boards;
        let board = u.floor();
        let f = u - board;
        let groove = if !(0.03..=0.97).contains(&f) { 0.35 } else { 1. };
        let tone = hash(board as i64, 0, 41);
        let grain = (x * 0.35 + fbm(x, y, 4, 3, 42 + board as u64) * 14.).sin() * 0.5 + 0.5;
        let streak = noise(x * 0.5, y * 0.02, (N as f32 * 0.5) as i64, 43);
        let c = mix([0.62, 0.42, 0.26], [0.46, 0.3, 0.18], tone * 0.6 + grain * 0.3 + streak * 0.2);
        (shade(c, groove), if groove < 1. { 0. } else { 0.6 + 0.4 * grain })
    });
    add(e, "Larch cladding", s, [1., 1.], 0.78, 0., 4., None)
}

/// Decking boards' wood grain, running along the image width.
pub fn decking(e: &mut Editor) -> Result<MaterialId> {
    let s = surface(|x, y| {
        let warp = fbm(x, y, 4, 3, 51) * 8.;
        let rings = ((y / N as f32 * 14. + warp) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let fine = noise(x * 0.05, y * 1.2, (N as f32 * 0.05) as i64, 52);
        let c = mix([0.6, 0.42, 0.27], [0.42, 0.28, 0.17], rings.powf(2.) * 0.7 + fine * 0.3);
        (c, rings * 0.6 + fine * 0.4)
    });
    add(e, "Decking", s, [1.2, 1.2], 0.72, 0., 2., None)
}

/// Random ashlar stone: blocks of varied length in courses, with recessed mortar.
pub fn stone(e: &mut Editor) -> Result<MaterialId> {
    let courses = 5.;
    let s = surface(|x, y| {
        let v = y / N as f32 * courses;
        let row = v.floor();
        let fv = v - row;
        // Block joints at irregular positions per course, wrapping across the tile.
        let shift = hash(row as i64, 0, 61) * 3.;
        let u = x / N as f32 * 3. + shift;
        let col = u.floor();
        let fu = u - col;
        let joint = fv < 0.05 || fu < 0.03;
        let tone = hash(col as i64 % 3, row as i64, 62);
        let rough = fbm(x, y, 16, 4, 63);
        if joint {
            return (shade([0.55, 0.53, 0.5], 0.85 + 0.2 * rough), 0.);
        }
        let c = mix([0.6, 0.56, 0.48], [0.72, 0.68, 0.6], tone);
        (shade(c, 0.8 + 0.35 * rough), 0.5 + 0.5 * rough)
    });
    add(e, "Stone", s, [2.4, 1.5], 0.85, 0., 5., None)
}

/// Large-format paving slabs, 600 mm square, with narrow joints.
pub fn paving(e: &mut Editor) -> Result<MaterialId> {
    let slabs = 2.;
    let s = surface(|x, y| {
        let (u, v) = (x / N as f32 * slabs, y / N as f32 * slabs);
        let (fu, fv) = (u.fract(), v.fract());
        let joint = fu < 0.008 || fv < 0.008;
        let tone = hash(u as i64, v as i64, 71);
        let t = fbm(x, y, 24, 3, 72);
        if joint {
            return ([0.32, 0.31, 0.3], 0.);
        }
        (shade(mix([0.66, 0.65, 0.62], [0.74, 0.73, 0.7], tone), 0.9 + 0.15 * t), 0.7 + 0.3 * t)
    });
    add(e, "Paving", s, [1.2, 1.2], 0.82, 0., 4., None)
}

/// Bark with vertical fissures.
pub fn bark(e: &mut Editor) -> Result<MaterialId> {
    let s = surface(|x, y| {
        let f = noise(x * 0.06, y * 0.008, (N as f32 * 0.06) as i64, 81);
        let d = fbm(x, y, 16, 3, 82);
        let ridge = (1. - (f * 2. - 1.).abs()).powf(3.);
        (shade([0.32, 0.25, 0.19], 0.55 + 0.6 * ridge + 0.15 * d), ridge)
    });
    add(e, "Bark", s, [0.6, 1.2], 0.95, 0., 6., None)
}

/// Leaf clusters in neutral tones, tinted by the given autumn colours.
pub fn foliage(e: &mut Editor, name: &str, tint: [f32; 4]) -> Result<MaterialId> {
    let s = surface(|x, y| {
        let leaves = noise(x / 6., y / 6., N as i64 / 6, 91);
        let shadow = fbm(x, y, 6, 3, 92);
        let k = 0.55 + 0.55 * leaves.powf(1.5) * (0.6 + 0.6 * shadow);
        ([k, k, k], leaves)
    });
    add(e, name, s, [0.8, 0.8], 0.8, 0., 5., Some(tint))
}
