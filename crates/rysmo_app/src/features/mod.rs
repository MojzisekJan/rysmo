//! Editor workflows built on the headless model and I/O codecs.
pub mod data_files;
pub mod drawing;
pub mod imports;
pub mod library;
pub mod parametric;
pub mod photo;

#[derive(Default)]
pub struct State {
    pub import: Option<imports::Draft>,
    pub library: library::State,
    pub parametric: parametric::State,
    pub drawing: drawing::State,
    pub photo: photo::State,
}
pub fn panels(ctx: &bevy_egui::egui::Context, app: &mut crate::state::App3d) {
    imports::panel(ctx, app);
    library::panel(ctx, app);
    parametric::panel(ctx, app);
    drawing::panel(ctx, app);
    photo::panel(ctx, app);
}
pub fn remember<R>(app: &mut crate::state::App3d, response: &Option<bevy_egui::egui::InnerResponse<Option<R>>>) {
    if let Some(response) = response {
        let r = response.response.rect;
        app.ui_rects.push((
            rysmo_math::DVec2::new(r.min.x as f64, r.min.y as f64),
            rysmo_math::DVec2::new(r.max.x as f64, r.max.y as f64),
        ));
    }
}
