//! Offscreen 3D target; egui composites it at native UI resolution.
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
};
#[derive(Resource)]
pub struct RenderView {
    pub image: Handle<Image>,
}
pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_uninit(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_DST;
    commands.insert_resource(RenderView { image: images.add(image) });
}
pub fn target_size(canvas: UVec2, scale: f32) -> UVec2 {
    let max = if cfg!(target_arch = "wasm32") { 4096. } else { 8192. };
    let factor = scale.clamp(0.5, 1.5).min(max / canvas.max_element().max(1) as f32);
    (canvas.as_vec2() * factor).round().as_uvec2().max(UVec2::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaled_target_preserves_aspect_and_bounds_allocations() {
        assert_eq!(target_size(UVec2::new(1920, 1080), 0.5), UVec2::new(960, 540));
        assert_eq!(target_size(UVec2::new(1920, 1080), 1.5), UVec2::new(2880, 1620));
        assert_eq!(target_size(UVec2::ZERO, 0.5), UVec2::ONE);
        let capped = target_size(UVec2::new(16000, 8000), 1.5);
        assert_eq!(capped, UVec2::new(8192, 4096));
    }
}
