use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DVec2, OrbitCamera};
use rysmo_model::{Command, ImageId, photo::Setup};
#[derive(Default)]
pub struct State {
    pub open: bool,
    pub active: bool,
    setup: Option<Setup>,
    saved_setup: Option<Setup>,
    texture: Option<(ImageId, egui::TextureHandle)>,
    generation: u64,
    last_camera: Option<OrbitCamera>,
    error: Option<String>,
}
fn sync(app: &mut App3d) {
    if app.features.photo.generation != app.document_generation {
        app.features.photo =
            State { open: app.features.photo.open, generation: app.document_generation, ..Default::default() };
    }
    if app.features.photo.saved_setup != app.editor.doc().photo_match {
        app.features.photo.saved_setup = app.editor.doc().photo_match.clone();
        app.features.photo.setup = app.editor.doc().photo_match.clone();
        app.features.photo.texture = None;
        app.features.photo.active = false;
    }
}
fn texture(ctx: &egui::Context, app: &App3d, state: &mut State) -> Option<egui::TextureHandle> {
    let setup = state.setup.as_ref()?;
    if let Some((id, texture)) = &state.texture
        && *id == setup.image
    {
        return Some(texture.clone());
    }
    let image = app.editor.doc().images.get(&setup.image)?;
    let format = image::guess_format(&image.bytes).ok()?;
    let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(&*image.bytes), format).into_dimensions().ok()?;
    if [w, h] != setup.size || w as u64 * h as u64 > 16_777_216 {
        state.error = Some("Photograph size does not match saved calibration.".into());
        return None;
    }
    let decoded = image::load_from_memory(&image.bytes).ok()?;
    let cap = ctx.input(|input| input.max_texture_side).clamp(1, 4096) as u32;
    let decoded = if decoded.width().max(decoded.height()) > cap {
        decoded.resize(cap, cap, image::imageops::FilterType::Triangle)
    } else {
        decoded
    }
    .to_rgba8();
    let pixels = egui::ColorImage::from_rgba_unmultiplied(
        [decoded.width() as usize, decoded.height() as usize],
        decoded.as_raw(),
    );
    let handle = ctx.load_texture(format!("photo-{}", setup.image.0), pixels, egui::TextureOptions::LINEAR);
    state.texture = Some((setup.image, handle.clone()));
    Some(handle)
}
fn fitted(rect: egui::Rect, size: [u32; 2]) -> egui::Rect {
    let factor = (rect.width() / size[0] as f32).min(rect.height() / size[1] as f32);
    egui::Rect::from_center_size(rect.center(), egui::vec2(size[0] as f32 * factor, size[1] as f32 * factor))
}
fn camera(setup: &Setup, size: DVec2) -> Result<OrbitCamera, String> {
    let mut camera = setup.camera()?;
    let fit = (size.x / setup.size[0] as f64).min(size.y / setup.size[1] as f64);
    if fit <= 0. || !fit.is_finite() {
        return Err("Invalid canvas size".into());
    }
    camera.fov_y = 2. * ((camera.fov_y * 0.5).tan() * size.y / (setup.size[1] as f64 * fit)).atan();
    Ok(camera)
}
/// Paint the photograph before compositing the transparent 3D target.
pub fn canvas(ctx: &egui::Context, ui: &mut egui::Ui, rect: egui::Rect, app: &mut App3d) {
    sync(app);
    let mut state = std::mem::take(&mut app.features.photo);
    if state.active {
        if state.last_camera.is_some_and(|camera| camera != app.camera) {
            state.active = false;
        } else if let Some(setup) = &state.setup {
            match camera(setup, DVec2::new(rect.width() as f64, rect.height() as f64)) {
                Ok(camera) => {
                    app.camera = camera;
                    state.last_camera = Some(camera);
                }
                Err(error) => {
                    state.error = Some(error);
                    state.active = false;
                }
            }
        }
    }
    if state.active
        && let Some(handle) = texture(ctx, app, &mut state)
        && let Some(setup) = &state.setup
    {
        ui.painter().rect_filled(rect, 0., egui::Color32::from_gray(25));
        ui.painter().image(
            handle.id(),
            fitted(rect, setup.size),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            egui::Color32::WHITE,
        );
    }
    app.features.photo = state;
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.photo.open {
        return;
    }
    sync(app);
    let mut state = std::mem::take(&mut app.features.photo);
    let image = texture(ctx, app, &mut state);
    let mut apply = false;
    let mut load = false;
    let mut clear = false;
    let response=crate::theme::window("Photo matching").open(&mut state.open).default_width(620.).resizable(true).show(ctx,|ui| {
        load=ui.button("Load photograph...").clicked();
        ui.weak("Align two red lines with parallel X edges and two green lines with perpendicular Y edges. Orange marks the model origin. Blue marks the other endpoint of a known length on the ground plane. Use an uncropped photo with little lens distortion.");
        if let Some(setup)=&mut state.setup {
            if let Some(image)=&image {
                let height=(ui.available_width()*setup.size[1] as f32/setup.size[0] as f32).clamp(180.,500.);
                let (space,_)=ui.allocate_exact_size(egui::vec2(ui.available_width(),height),egui::Sense::hover());
                let rect=fitted(space,setup.size);
                ui.painter().image(image.id(),rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);
                let at=|p:DVec2|rect.min+egui::vec2(p.x as f32*rect.width(),p.y as f32*rect.height());
                for family in 0..2 {
                    let color=if family==0 {egui::Color32::from_rgb(255,90,80)}else{egui::Color32::from_rgb(80,230,130)};
                    for line in 0..2 {
                        ui.painter().line_segment(setup.guides[family][line].map(at),egui::Stroke::new(2.,color));
                        for end in 0..2 {handle(ui,rect,&mut setup.guides[family][line][end],egui::Id::unique(("photo-guide",family,line,end)),color);}
                    }
                }
                ui.painter().line_segment([at(setup.origin),at(setup.reference)],egui::Stroke::new(1.5,egui::Color32::LIGHT_BLUE));
                handle(ui,rect,&mut setup.origin,egui::Id::unique("photo-origin"),egui::Color32::from_rgb(255,170,50));
                handle(ui,rect,&mut setup.reference,egui::Id::unique("photo-reference"),egui::Color32::LIGHT_BLUE);
            }
            ui.add(crate::widgets::Number::new(&mut setup.reference_length).range(0.001..=1e6).speed(0.01).prefix("Reference length ").suffix(" m"));
            ui.horizontal(|ui| {crate::widgets::checkbox(ui,&mut setup.flip_x,"Reverse X");crate::widgets::checkbox(ui,&mut setup.flip_y,"Reverse Y");});
            ui.horizontal_wrapped(|ui| {apply=ui.button("Apply / restore matched view").clicked();if ui.button("Hide photograph").clicked(){state.active=false;}clear=ui.button("Remove photo matching").clicked();});
            ui.weak("Orbiting, panning or zooming detaches the photograph. Apply restores the saved perspective. Photograph and guides are saved in the project.");
        }
        if let Some(error)=&state.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
    });
    super::remember(app, &response);
    if apply && let Some(setup) = state.setup.clone() {
        match camera(&setup, app.canvas_size()).and_then(|camera| {
            app.editor.apply(Command::SetPhotoMatch { setup: Some(setup.clone()) }).map_err(|e| e.to_string())?;
            state.saved_setup = Some(setup);
            app.camera = camera;
            state.last_camera = Some(camera);
            state.active = true;
            ctx.request_repaint();
            Ok(())
        }) {
            Ok(()) => state.error = None,
            Err(error) => state.error = Some(error),
        }
    }
    if clear {
        match app.editor.apply(Command::SetPhotoMatch { setup: None }) {
            Ok(_) => {
                state.setup = None;
                state.active = false;
                state.texture = None;
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    app.features.photo = state;
    if load {
        super::data_files::pick(app, super::data_files::Purpose::Photo);
    }
}
fn handle(ui: &egui::Ui, rect: egui::Rect, point: &mut DVec2, id: egui::Id, color: egui::Color32) {
    let at = rect.min + egui::vec2(point.x as f32 * rect.width(), point.y as f32 * rect.height());
    let response = ui.interact(egui::Rect::from_center_size(at, egui::vec2(16., 16.)), id, egui::Sense::drag());
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        *point = DVec2::new(
            ((pointer.x - rect.left()) / rect.width()) as f64,
            ((pointer.y - rect.top()) / rect.height()) as f64,
        )
        .clamp(DVec2::ZERO, DVec2::ONE);
    }
    ui.painter().circle_filled(at, 5., color);
    ui.painter().circle_stroke(at, 5., egui::Stroke::new(1., egui::Color32::BLACK));
}
