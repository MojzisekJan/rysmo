//! Image-based lighting from equirectangular panoramas. The cube maps are filtered on the CPU,
//! so the browser build, which has no compute shaders, lights models the same way.
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension},
};
use std::f32::consts::PI;

/// Mean radiance the panorama is scaled to, matching the built-in studio environments.
const MEAN_RADIANCE: f32 = 0.3;
const SPECULAR_SAMPLES: u32 = 96;

/// The three cube maps one panorama becomes.
pub struct Maps {
    pub background: Handle<Image>,
    pub diffuse: Handle<Image>,
    pub specular: Handle<Image>,
}

/// A panorama as linear radiance, with progressively halved copies for filtered lookups.
struct Panorama {
    levels: Vec<(u32, u32, Vec<Vec3>)>,
}

impl Panorama {
    fn decode(bytes: &[u8], max_width: u32) -> Option<Self> {
        let format = image::guess_format(bytes).ok()?;
        let decoded = image::load_from_memory_with_format(bytes, format).ok()?;
        let linear = format == image::ImageFormat::Hdr;
        let mut rgb = decoded.to_rgb32f();
        if rgb.width() > max_width {
            let height = (max_width / 2).max(1);
            rgb = image::imageops::resize(&rgb, max_width, height, image::imageops::FilterType::Triangle);
        }
        let (w, h) = rgb.dimensions();
        if w < 8 || h < 4 {
            return None;
        }
        let to_linear = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let mut base: Vec<Vec3> = rgb
            .pixels()
            .map(|p| {
                let c = Vec3::from_array(p.0);
                let c = if linear { c } else { c.map(to_linear) };
                if c.is_finite() { c.max(Vec3::ZERO) } else { Vec3::ZERO }
            })
            .collect();
        // Scale to a consistent brightness: the solid-angle weighted mean luminance.
        let mut sum = 0.;
        let mut weight = 0.;
        for y in 0..h {
            let solid = ((y as f32 + 0.5) / h as f32 * PI).sin();
            for x in 0..w {
                sum += base[(y * w + x) as usize].dot(Vec3::new(0.2126, 0.7152, 0.0722)) * solid;
                weight += solid;
            }
        }
        let mean = sum / weight;
        if !(mean.is_finite() && mean > 0.) {
            return None;
        }
        for c in &mut base {
            *c *= MEAN_RADIANCE / mean;
        }
        let mut levels = vec![(w, h, base)];
        while levels.last().is_some_and(|(w, h, _)| *w > 8 && *h > 4) {
            let (w, h, data) = levels.last().unwrap();
            let (nw, nh) = (w / 2, h / 2);
            let next = (0..nh * nw)
                .map(|i| {
                    let (x, y) = (i % nw * 2, i / nw * 2);
                    let at = |x: u32, y: u32| data[(y.min(h - 1) * w + x.min(w - 1)) as usize];
                    (at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1)) * 0.25
                })
                .collect();
            levels.push((nw, nh, next));
        }
        Some(Self { levels })
    }

    /// Solid angle of one texel of the full-resolution level.
    fn texel_solid_angle(&self) -> f32 {
        let (w, h, _) = self.levels[0];
        4. * PI / (w * h) as f32
    }

    fn bilinear(&self, level: usize, d: Vec3) -> Vec3 {
        let (w, h, data) = &self.levels[level.min(self.levels.len() - 1)];
        let (w, h) = (*w as i64, *h as i64);
        // The centre of the image looks along -Z; to its right lies +X.
        let u = 0.5 + d.x.atan2(-d.z) / (2. * PI);
        let v = d.y.clamp(-1., 1.).acos() / PI;
        let x = u * w as f32 - 0.5;
        let y = (v * h as f32 - 0.5).clamp(0., (h - 1) as f32);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: i64, y: i64| data[(y.clamp(0, h - 1) * w + x.rem_euclid(w)) as usize];
        let (x0, y0) = (x0 as i64, y0 as i64);
        at(x0, y0).lerp(at(x0 + 1, y0), fx).lerp(at(x0, y0 + 1).lerp(at(x0 + 1, y0 + 1), fx), fy)
    }

    /// Radiance averaged over a cone about `lod` halvings wide.
    fn sample(&self, d: Vec3, lod: f32) -> Vec3 {
        let lod = lod.clamp(0., (self.levels.len() - 1) as f32);
        let low = lod.floor() as usize;
        self.bilinear(low, d).lerp(self.bilinear(low + 1, d), lod - low as f32)
    }

    fn lod(&self, solid_angle: f32) -> f32 {
        (0.5 * (solid_angle / self.texel_solid_angle()).log2()).max(0.)
    }

    /// The panorama projected onto the first nine spherical harmonics.
    fn harmonics(&self) -> [Vec3; 9] {
        let level = self.levels.iter().position(|(w, _, _)| *w <= 128).unwrap_or(self.levels.len() - 1);
        let (w, h, _) = self.levels[level];
        let mut sh = [Vec3::ZERO; 9];
        for y in 0..h {
            let theta = (y as f32 + 0.5) / h as f32 * PI;
            let solid = theta.sin() * (PI / h as f32) * (2. * PI / w as f32);
            for x in 0..w {
                let u = (x as f32 + 0.5) / w as f32;
                let phi = (u - 0.5) * 2. * PI;
                let d = Vec3::new(theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos());
                let c = self.bilinear(level, d) * solid;
                for (k, basis) in basis(d).into_iter().enumerate() {
                    sh[k] += c * basis;
                }
            }
        }
        sh
    }
}

fn basis(d: Vec3) -> [f32; 9] {
    [
        0.282095,
        0.488603 * d.y,
        0.488603 * d.z,
        0.488603 * d.x,
        1.092548 * d.x * d.y,
        1.092548 * d.y * d.z,
        0.315392 * (3. * d.z * d.z - 1.),
        1.092548 * d.x * d.z,
        0.546274 * (d.x * d.x - d.y * d.y),
    ]
}

/// World direction of a cube texel. Bevy samples its cube maps with Z negated.
fn direction(face: u32, x: u32, y: u32, size: u32) -> Vec3 {
    let u = 2. * (x as f32 + 0.5) / size as f32 - 1.;
    let v = 2. * (y as f32 + 0.5) / size as f32 - 1.;
    let c = match face {
        0 => Vec3::new(1., -v, -u),
        1 => Vec3::new(-1., -v, u),
        2 => Vec3::new(u, 1., v),
        3 => Vec3::new(u, -1., -v),
        4 => Vec3::new(u, -v, 1.),
        _ => Vec3::new(-u, -v, -1.),
    }
    .normalize();
    Vec3::new(c.x, c.y, -c.z)
}

fn hammersley(i: u32, n: u32) -> Vec2 {
    Vec2::new(i as f32 / n as f32, i.reverse_bits() as f32 * 2.328_306_4e-10)
}

/// The panorama seen through a GGX lobe of the given perceptual roughness.
fn prefiltered(panorama: &Panorama, n: Vec3, roughness: f32, texel_solid_angle: f32) -> Vec3 {
    if roughness <= 0. {
        return panorama.sample(n, panorama.lod(texel_solid_angle));
    }
    let alpha = roughness * roughness;
    let a2 = alpha * alpha;
    let up = if n.y.abs() < 0.999 { Vec3::Y } else { Vec3::X };
    let tx = up.cross(n).normalize();
    let ty = n.cross(tx);
    let mut sum = Vec3::ZERO;
    let mut weight = 0.;
    for i in 0..SPECULAR_SAMPLES {
        let xi = hammersley(i, SPECULAR_SAMPLES);
        let phi = 2. * PI * xi.x;
        let cos_theta = ((1. - xi.y) / (1. + (a2 - 1.) * xi.y)).sqrt();
        let sin_theta = (1. - cos_theta * cos_theta).max(0.).sqrt();
        let h = tx * (sin_theta * phi.cos()) + ty * (sin_theta * phi.sin()) + n * cos_theta;
        let l = 2. * n.dot(h) * h - n;
        let n_dot_l = n.dot(l);
        if n_dot_l <= 0. {
            continue;
        }
        // With the view along the normal the sample density is D(h) / 4.
        let d = a2 / (PI * (cos_theta * cos_theta * (a2 - 1.) + 1.).powi(2));
        let solid_angle = 4. / (SPECULAR_SAMPLES as f32 * d.max(1e-6));
        sum += panorama.sample(l, panorama.lod(solid_angle) + 1.) * n_dot_l;
        weight += n_dot_l;
    }
    if weight > 0. { sum / weight } else { panorama.sample(n, 0.) }
}

/// Runs one closure per cube face, on separate threads where there are threads.
fn per_face(f: impl Fn(u32) -> Vec<u8> + Sync) -> Vec<u8> {
    #[cfg(not(target_arch = "wasm32"))]
    let faces: Vec<Vec<u8>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..6)
            .map(|face| {
                scope.spawn({
                    let f = &f;
                    move || f(face)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("environment face")).collect()
    });
    #[cfg(target_arch = "wasm32")]
    let faces: Vec<Vec<u8>> = (0..6).map(f).collect();
    faces.concat()
}

fn push(data: &mut Vec<u8>, c: Vec3) {
    for v in [c.x, c.y, c.z, 1.] {
        data.extend_from_slice(&half::f16::from_f32(v.min(65_000.)).to_le_bytes());
    }
}

fn cube(size: u32, levels: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new_uninit(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = levels;
    image.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image.data = Some(data);
    image
}

/// Background, diffuse and specular cube maps of a panorama, or `None` if it does not decode.
pub fn build(bytes: &[u8]) -> Option<[Image; 3]> {
    let web = cfg!(target_arch = "wasm32");
    let panorama = Panorama::decode(bytes, if web { 2048 } else { 4096 })?;
    let width = panorama.levels[0].0;
    let background_size = (width / 4).next_power_of_two().clamp(64, if web { 512 } else { 1024 });
    let background = per_face(|face| {
        let mut data = Vec::new();
        let solid = 4. * PI / (6 * background_size * background_size) as f32;
        let lod = panorama.lod(solid);
        for y in 0..background_size {
            for x in 0..background_size {
                push(&mut data, panorama.sample(direction(face, x, y, background_size), lod));
            }
        }
        data
    });
    let specular_size: u32 = if web { 128 } else { 256 };
    let levels = specular_size.ilog2() + 1;
    let specular = per_face(|face| {
        let mut data = Vec::new();
        for mip in 0..levels {
            let size = specular_size >> mip;
            let roughness = mip as f32 / (levels - 1) as f32;
            let solid = 4. * PI / (6 * size * size) as f32;
            for y in 0..size {
                for x in 0..size {
                    push(&mut data, prefiltered(&panorama, direction(face, x, y, size), roughness, solid));
                }
            }
        }
        data
    });
    // Lambertian irradiance from the harmonics, divided by π as Bevy expects.
    let sh = panorama.harmonics();
    let band = [1., 2. / 3., 2. / 3., 2. / 3., 0.25, 0.25, 0.25, 0.25, 0.25];
    const DIFFUSE: u32 = 32;
    let diffuse = per_face(|face| {
        let mut data = Vec::new();
        for y in 0..DIFFUSE {
            for x in 0..DIFFUSE {
                let d = direction(face, x, y, DIFFUSE);
                let e = basis(d).into_iter().zip(sh).zip(band).fold(Vec3::ZERO, |sum, ((b, c), a)| sum + c * b * a);
                push(&mut data, e.max(Vec3::ZERO));
            }
        }
        data
    });
    Some([cube(background_size, 1, background), cube(DIFFUSE, 1, diffuse), cube(specular_size, levels, specular)])
}

/// Decoded maps of the current panorama, keyed by its bytes.
#[derive(Resource, Default)]
pub struct Cache {
    pub key: Option<usize>,
    pub maps: Option<Maps>,
}

impl Cache {
    /// The maps for these panorama bytes, building them the first time.
    pub fn get(&mut self, bytes: &std::sync::Arc<Vec<u8>>, images: &mut Assets<Image>) -> Option<&Maps> {
        let key = std::sync::Arc::as_ptr(bytes) as usize;
        if self.key != Some(key) {
            self.key = Some(key);
            let start = bevy::platform::time::Instant::now();
            let built = build(bytes);
            info!("Environment panorama filtered in {:.0} ms", start.elapsed().as_secs_f64() * 1000.);
            self.maps = built.map(|[background, diffuse, specular]| Maps {
                background: images.add(background),
                diffuse: images.add(diffuse),
                specular: images.add(specular),
            });
        }
        self.maps.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny Radiance HDR file: a bright band above the horizon, dark below.
    fn panorama(w: u32, h: u32) -> Vec<u8> {
        let mut image = image::Rgb32FImage::new(w, h);
        for (_, y, p) in image.enumerate_pixels_mut() {
            p.0 = if y < h / 2 { [2., 2.5, 3.] } else { [0.1, 0.08, 0.05] };
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb32F(image).write_to(&mut out, image::ImageFormat::Hdr).unwrap();
        out.into_inner()
    }

    #[test]
    fn panorama_becomes_complete_cube_maps_lit_from_above() {
        let [background, diffuse, specular] = build(&panorama(64, 32)).unwrap();
        assert_eq!(background.texture_descriptor.size.width, 64);
        assert_eq!(specular.texture_descriptor.mip_level_count, 9);
        let texels: u32 = (0..9).map(|m| (256u32 >> m).pow(2)).sum();
        assert_eq!(specular.data.as_ref().unwrap().len(), 6 * 8 * texels as usize);
        // The +Y face of the irradiance map is brighter than the -Y face.
        let data = diffuse.data.as_ref().unwrap();
        let face = 32 * 32 * 8;
        let red = |face_index: usize| {
            let at = face_index * face + (16 * 32 + 16) * 8;
            half::f16::from_le_bytes([data[at], data[at + 1]]).to_f32()
        };
        assert!(red(2) > 3. * red(3), "{} {}", red(2), red(3));
    }

    #[test]
    fn equirectangular_lookup_faces_forward_at_the_centre() {
        // Each texel holds its column; +X is a quarter turn to the right of -Z.
        let p = Panorama { levels: vec![(8, 4, (0..32).map(|i| Vec3::splat((i % 8) as f32)).collect())] };
        assert!((p.bilinear(0, Vec3::NEG_Z).x - 3.5).abs() < 1e-4);
        assert!((p.bilinear(0, Vec3::X).x - 5.5).abs() < 1e-4);
        assert!(Panorama::decode(b"not an image", 64).is_none());
    }
}
