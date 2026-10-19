//! Small self-contained studio light maps with analytic roughness filtering.
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension},
};
#[derive(Resource)]
pub struct StudioMaps {
    pub neutral: [Handle<Image>; 2],
    pub warm: [Handle<Image>; 2],
}
pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(StudioMaps {
        neutral: [images.add(cubemap(false, true)), images.add(cubemap(false, false))],
        warm: [images.add(cubemap(true, true)), images.add(cubemap(true, false))],
    });
}
fn direction(face: u32, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1., -v, -u),
        1 => Vec3::new(-1., -v, u),
        2 => Vec3::new(u, 1., v),
        3 => Vec3::new(u, -1., -v),
        4 => Vec3::new(u, -v, 1.),
        _ => Vec3::new(-u, -v, -1.),
    }
    .normalize()
}
fn cubemap(warm: bool, diffuse: bool) -> Image {
    const SIZE: u32 = 32;
    let levels = if diffuse { 1 } else { 6 };
    let mut data = Vec::new();
    for face in 0..6 {
        for mip in 0..levels {
            let size = SIZE >> mip;
            let sharpness = if diffuse { 1.5 } else { 64. / 4f32.powi(mip as i32) };
            for y in 0..size {
                for x in 0..size {
                    let d = direction(
                        face,
                        2. * (x as f32 + 0.5) / size as f32 - 1.,
                        2. * (y as f32 + 0.5) / size as f32 - 1.,
                    );
                    let key = d.dot(Vec3::new(-0.5, 0.8, 0.35).normalize()).max(0.).powf(sharpness);
                    let fill = d.dot(Vec3::new(0.8, 0.3, -0.5).normalize()).max(0.).powf(sharpness);
                    let base = 0.12 + 0.1 * (d.y * 0.5 + 0.5);
                    let rgb = if warm {
                        Vec3::new(
                            base + 0.65 * key + 0.18 * fill,
                            base * 0.9 + 0.48 * key + 0.22 * fill,
                            base * 0.8 + 0.30 * key + 0.30 * fill,
                        )
                    } else {
                        Vec3::splat(base + 0.55 * key + 0.22 * fill)
                    };
                    data.extend(rgb.clamp(Vec3::ZERO, Vec3::ONE).to_array().map(|v| (v * 255.).round() as u8));
                    data.push(255);
                }
            }
        }
    }
    let mut image = Image::new_uninit(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 6 },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = levels;
    image.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image.data = Some(data);
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_cube_faces_and_mips_have_distinct_environment_colours() {
        let neutral = cubemap(false, false);
        let warm = cubemap(true, false);
        assert_eq!(neutral.texture_descriptor.mip_level_count, 6);
        assert_eq!(neutral.data.as_ref().unwrap().len(), 6 * 4 * (1024 + 256 + 64 + 16 + 4 + 1));
        assert_ne!(neutral.data, warm.data);
        let diffuse = cubemap(false, true);
        assert_eq!(diffuse.data.as_ref().unwrap().len(), 6 * 4 * 1024);
        assert_eq!(diffuse.texture_view_descriptor.unwrap().dimension, Some(TextureViewDimension::Cube));
    }
}
