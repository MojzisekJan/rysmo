//! Shared embedded branding. No asset server or network request is needed in the editor.
use bevy_egui::egui;

fn decode(bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("embedded brand PNG");
    let mut pixels = vec![0; reader.output_buffer_size().expect("brand image size")];
    let info = reader.next_frame(&mut pixels).expect("brand image pixels");
    pixels.truncate(info.buffer_size());
    (pixels, info.width, info.height)
}

pub fn image(ui: &mut egui::Ui, size: f32) -> egui::Response {
    // Downscale the 512 px logo to the displayed pixel size on the CPU: sampling it directly
    // without mipmaps aliases its curved edges.
    let pixels = (size * ui.ctx().pixels_per_point()).round().clamp(16., 512.) as u32;
    let id = egui::Id::unique(("rysmo-brand-texture", pixels));
    let texture = ui.ctx().data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = texture.unwrap_or_else(|| {
        let (rgba, width, height) = decode(include_bytes!("../brand/logo.png"));
        let source = image::RgbaImage::from_raw(width, height, rgba).expect("brand image buffer");
        let scaled = image::imageops::resize(&source, pixels, pixels, image::imageops::FilterType::Lanczos3);
        let texture = ui.ctx().load_texture(
            "Rysmo logo",
            egui::ColorImage::from_rgba_unmultiplied([pixels as usize, pixels as usize], scaled.as_raw()),
            egui::TextureOptions::LINEAR,
        );
        ui.ctx().data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    ui.image((texture.id(), egui::vec2(size, size)))
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub fn window_icon(_main_thread: bevy::ecs::system::NonSendMarker, mut done: bevy::prelude::Local<bool>) {
    if *done {
        return;
    }
    // Bevy 0.20 keeps native windows in thread-local storage, not an ECS resource.
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        if windows.windows.is_empty() {
            return;
        }
        let (rgba, width, height) = decode(include_bytes!("../brand/icon-64.png"));
        let icon = winit::window::Icon::from_rgba(rgba, width, height).expect("embedded window icon");
        for window in windows.windows.values() {
            window.set_window_icon(Some(icon.clone()));
        }
        *done = true;
    });
}

#[cfg(all(test, not(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))))]
mod tests {
    #[test]
    fn icon_system_can_wait_for_window_creation_without_a_non_send_resource() {
        let mut app = bevy::prelude::App::new();
        app.add_systems(bevy::prelude::Update, super::window_icon);
        app.update();
    }
}
