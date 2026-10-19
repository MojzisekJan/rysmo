//! iOS window size and safe area.
//!
//! winit 0.30 reports the safe area as the iOS window's inner size, and Bevy answers any
//! change of `Window` by requesting that size. The view and touches cover the whole
//! screen, so keep rendering at the screen size and tell egui which edges the notch,
//! status bar and home indicator cover. `Context::content_rect` then excludes them.
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResized};
use bevy::winit::WINIT_WINDOWS;
use bevy_egui::{EguiInput, EguiPreUpdateSet, EguiPrimaryContextPass};

pub fn plugin(app: &mut App) {
    app.add_systems(PreUpdate, sync.after(EguiPreUpdateSet::ProcessInput).before(EguiPreUpdateSet::BeginPass))
        .add_systems(EguiPrimaryContextPass, cover_insets);
}

fn sync(
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut inputs: Query<&mut EguiInput>,
    mut resized: MessageWriter<WindowResized>,
    // WINIT_WINDOWS is thread-local to the main thread.
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    let Ok((entity, mut window)) = windows.single_mut() else { return };
    let Some((size, insets)) = WINIT_WINDOWS.with_borrow(|windows| {
        let w = windows.get_window(entity)?;
        let outer = w.outer_size();
        let inner = w.inner_size();
        let (outer_pos, inner_pos) = (w.outer_position().ok()?, w.inner_position().ok()?);
        let scale = w.scale_factor() as f32;
        let left = (inner_pos.x - outer_pos.x).max(0) as f32 / scale;
        let top = (inner_pos.y - outer_pos.y).max(0) as f32 / scale;
        let right = (outer.width as f32 / scale - inner.width as f32 / scale - left).max(0.);
        let bottom = (outer.height as f32 / scale - inner.height as f32 / scale - top).max(0.);
        Some((outer, bevy_egui::egui::SafeAreaInsets(bevy_egui::egui::epaint::MarginF32 { left, right, top, bottom })))
    }) else {
        return;
    };
    if size.width > 0
        && size.height > 0
        && (window.physical_width(), window.physical_height()) != (size.width, size.height)
    {
        // Bevy would answer a change of `Window` with winit's safe-area size; keep it unseen.
        window.bypass_change_detection().resolution.set_physical_resolution(size.width, size.height);
        resized.write(WindowResized { window: entity, width: window.width(), height: window.height() });
    }
    for mut input in &mut inputs {
        input.safe_area_insets = Some(insets);
    }
}

/// Paints the panel colour behind the status bar and home indicator so the menu and
/// status bar appear to reach the screen edges.
fn cover_insets(mut contexts: bevy_egui::EguiContexts) -> Result {
    let ctx = contexts.ctx_mut()?;
    let (full, safe) = (ctx.viewport_rect(), ctx.content_rect());
    // A layer painter is clipped to the content rect; these strips lie outside it.
    let mut painter = ctx.layer_painter(bevy_egui::egui::LayerId::background());
    painter.set_clip_rect(full);
    let fill = ctx.style_of(bevy_egui::egui::Theme::Dark).visuals.panel_fill;
    for rect in [
        bevy_egui::egui::Rect::from_min_max(full.min, bevy_egui::egui::pos2(full.right(), safe.top())),
        bevy_egui::egui::Rect::from_min_max(bevy_egui::egui::pos2(full.left(), safe.bottom()), full.max),
    ] {
        if rect.height() > 0. {
            painter.rect_filled(rect, 0., fill);
        }
    }
    Ok(())
}
