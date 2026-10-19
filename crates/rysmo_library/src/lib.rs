//! The starter material library: plain colours and a few textures (brick, wood,
//! concrete, metal, grass, paving), all generated here by code, so they carry the
//! project's own licence and no third-party assets. See NOTICE.md.
//!
//! Textures are 256 by 256 pixels, tile seamlessly, and come with their real-world size.

#![forbid(unsafe_code)]

/// A material in the library.
#[derive(Clone, Debug)]
pub struct LibraryMaterial {
    pub name: &'static str,
    /// sRGB with opacity; for textured materials, the texture's average colour.
    pub color: [f32; 4],
    pub texture: Option<LibraryTexture>,
}

#[derive(Clone, Debug)]
pub struct LibraryTexture {
    /// A file name for the image.
    pub file: &'static str,
    /// Metres one copy of the image covers.
    pub size: [f64; 2],
    /// The image, PNG encoded.
    pub png: Vec<u8>,
}

const N: usize = 256;

type Pixels = Vec<[f32; 3]>;

/// A hash of integer coordinates to 0..1.
fn hash(x: i64, y: i64, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise that repeats every `period` cells.
fn noise(x: f32, y: f32, period: i64, seed: u64) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let c = |i: i64, j: i64| hash(i.rem_euclid(period), j.rem_euclid(period), seed);
    let (i, j) = (x0 as i64, y0 as i64);
    let a = c(i, j) + (c(i + 1, j) - c(i, j)) * s(fx);
    let b = c(i, j + 1) + (c(i + 1, j + 1) - c(i, j + 1)) * s(fx);
    a + (b - a) * s(fy)
}

/// Several octaves of noise over the image, tiling: 0..1.
fn fbm(px: usize, py: usize, base: i64, octaves: u32, seed: u64) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut total = 0.0;
    for o in 0..octaves {
        let cells = base << o;
        let f = cells as f32 / N as f32;
        sum += amp * noise(px as f32 * f, py as f32 * f, cells, seed + o as u64);
        total += amp;
        amp *= 0.5;
    }
    sum / total
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn image(f: impl Fn(usize, usize) -> [f32; 3]) -> Pixels {
    (0..N * N).map(|i| f(i % N, i / N)).collect()
}

/// Running-bond brickwork: two bricks across, four courses down; 225 by 75 mm per brick
/// with its joint.
fn brick() -> Pixels {
    let course = N / 4;
    let unit = N / 2;
    let joint = 6;
    image(|x, y| {
        let row = y / course;
        let shift = if row % 2 == 1 { unit / 2 } else { 0 };
        let xs = (x + shift) % N;
        let col = xs / unit;
        let in_joint = y % course < joint || xs % unit < joint;
        let n = fbm(x, y, 16, 3, 7);
        if in_joint {
            return scale([0.72, 0.70, 0.66], 0.85 + 0.25 * n);
        }
        let tone = hash(col as i64 + row as i64 * 7, row as i64, 11);
        let base = mix([0.62, 0.26, 0.18], [0.74, 0.38, 0.25], tone);
        scale(base, 0.8 + 0.35 * n)
    })
}

/// Wood grain running across, with growth rings bent by noise.
fn wood() -> Pixels {
    image(|x, y| {
        let warp = fbm(x, y, 4, 3, 21) * 6.0;
        let rings = ((y as f32 / N as f32 * 10.0 + warp) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let fine = fbm(x, y, 64, 2, 23);
        let light = [0.80, 0.62, 0.42];
        let dark = [0.58, 0.40, 0.24];
        scale(mix(light, dark, rings.powf(3.0) * 0.8), 0.9 + 0.15 * fine)
    })
}

fn concrete() -> Pixels {
    image(|x, y| {
        let n = fbm(x, y, 8, 5, 31);
        let pores = if hash(x as i64, y as i64, 33) > 0.985 { 0.75 } else { 1.0 };
        scale([0.66, 0.66, 0.64], (0.82 + 0.3 * n) * pores)
    })
}

/// Brushed metal: fine streaks along the width.
fn metal() -> Pixels {
    image(|x, y| {
        let streak = noise(x as f32 / 32.0, y as f32 * 0.9, 8, 41);
        let n = fbm(x, y, 8, 2, 43);
        scale([0.74, 0.76, 0.78], 0.88 + 0.12 * streak + 0.06 * n)
    })
}

fn grass() -> Pixels {
    image(|x, y| {
        let n = fbm(x, y, 16, 4, 51);
        let blade = hash(x as i64, y as i64, 53);
        let base = mix([0.22, 0.42, 0.14], [0.42, 0.60, 0.22], n);
        scale(base, 0.85 + 0.3 * blade)
    })
}

/// Square paving slabs, four by four, with joints.
fn paving() -> Pixels {
    let slab = N / 4;
    image(|x, y| {
        let joint = x % slab < 3 || y % slab < 3;
        let n = fbm(x, y, 16, 3, 61);
        if joint {
            return scale([0.45, 0.44, 0.42], 0.9 + 0.2 * n);
        }
        let tone = hash((x / slab) as i64, (y / slab) as i64, 63);
        scale(mix([0.70, 0.68, 0.64], [0.80, 0.78, 0.74], tone), 0.88 + 0.2 * n)
    })
}

fn encode(pixels: &Pixels) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, N as u32, N as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("PNG header");
        let data: Vec<u8> = pixels.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect();
        w.write_image_data(&data).expect("PNG data");
    }
    out
}

fn average(pixels: &Pixels) -> [f32; 4] {
    let mut s = [0.0f32; 3];
    for p in pixels {
        for k in 0..3 {
            s[k] += p[k];
        }
    }
    let n = pixels.len() as f32;
    [s[0] / n, s[1] / n, s[2] / n, 1.0]
}

fn textured(name: &'static str, file: &'static str, size: [f64; 2], pixels: Pixels) -> LibraryMaterial {
    LibraryMaterial {
        name,
        color: average(&pixels),
        texture: Some(LibraryTexture { file, size, png: encode(&pixels) }),
    }
}

fn plain(name: &'static str, color: [f32; 4]) -> LibraryMaterial {
    LibraryMaterial { name, color, texture: None }
}

/// The library, textures generated now (a few milliseconds each).
pub fn materials() -> Vec<LibraryMaterial> {
    vec![
        textured("Brick", "brick.png", [0.45, 0.30], brick()),
        textured("Wood", "wood.png", [0.8, 0.8], wood()),
        textured("Concrete", "concrete.png", [1.5, 1.5], concrete()),
        textured("Brushed metal", "metal.png", [0.5, 0.5], metal()),
        textured("Grass", "grass.png", [1.0, 1.0], grass()),
        textured("Paving", "paving.png", [1.6, 1.6], paving()),
        plain("Glass", [0.65, 0.80, 0.92, 0.35]),
        plain("White", [0.96, 0.96, 0.95, 1.0]),
        plain("Light grey", [0.75, 0.75, 0.74, 1.0]),
        plain("Dark grey", [0.30, 0.30, 0.31, 1.0]),
        plain("Black", [0.08, 0.08, 0.08, 1.0]),
        plain("Plaster", [0.93, 0.90, 0.84, 1.0]),
        plain("Terracotta", [0.72, 0.38, 0.24, 1.0]),
        plain("Roof red", [0.55, 0.20, 0.15, 1.0]),
        plain("Sand", [0.86, 0.78, 0.60, 1.0]),
        plain("Leaf green", [0.36, 0.56, 0.25, 1.0]),
        plain("Sky blue", [0.45, 0.65, 0.88, 1.0]),
        plain("Water", [0.25, 0.45, 0.75, 0.6]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_decode_and_tile() {
        let lib = materials();
        assert!(lib.len() >= 15);
        for m in lib.iter().filter(|m| m.texture.is_some()) {
            let t = m.texture.as_ref().unwrap();
            let dec = png::Decoder::new(std::io::Cursor::new(&t.png));
            let mut reader = dec.read_info().unwrap();
            let mut buf = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut buf).unwrap();
            assert_eq!((info.width, info.height), (N as u32, N as u32), "{}", m.name);
            assert!(t.size[0] > 0.0 && t.size[1] > 0.0);
            // Seamless: the first and last columns of the noise textures differ about as
            // much as neighbours do. (Brick and paving have their joints at one edge.)
            if matches!(m.name, "Brick" | "Paving") {
                continue;
            }
            let px = |x: usize, y: usize| -> i32 { buf[(y * N + x) * 3] as i32 };
            let edge: i32 = (0..N).map(|y| (px(0, y) - px(N - 1, y)).abs()).sum();
            let inner: i32 = (0..N).map(|y| (px(100, y) - px(101, y)).abs()).sum();
            assert!(edge <= inner * 4 + N as i32 * 8, "{} does not tile: {edge} vs {inner}", m.name);
        }
    }

    #[test]
    fn glass_is_see_through() {
        let glass = materials().into_iter().find(|m| m.name == "Glass").unwrap();
        assert!(glass.color[3] < 0.5 && glass.texture.is_none());
    }
}
