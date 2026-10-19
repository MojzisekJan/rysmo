//! Event-driven frames preserve pointer responsiveness without an idle 60 Hz render loop.
use bevy::{
    prelude::*,
    winit::{UpdateMode, WinitSettings},
};
use std::time::Duration;
pub fn plugin(app: &mut App) {
    app.insert_resource(WinitSettings::desktop_app())
        .add_message::<bevy::window::WindowEvent>()
        .add_systems(Last, update);
}
fn update(
    app: Res<crate::state::App3d>,
    mut settings: ResMut<WinitSettings>,
    mut frames: Local<u32>,
    mut events: MessageReader<bevy::window::WindowEvent>,
    mut tail: Local<u8>,
    mut revision: Local<(u64, u64)>,
) {
    let current = (app.document_generation, app.editor.revision());
    if events.read().filter(|e| !matches!(e, bevy::window::WindowEvent::RequestRedraw(_))).count() > 0
        || *revision != current
    {
        *tail = 4;
    }
    *revision = current;
    *frames = frames.saturating_add(1);
    // Pipeline compilation, uploads and egui need several startup frames.
    let preview = cfg!(not(target_arch = "wasm32")) && std::env::var_os("RYSMO_SCREENSHOT").is_some();
    let active = *frames < 180
        || *tail > 0
        || preview
        || !app.preferences.sleep_renderer
        || app.walking.is_some()
        || app.left_down;
    *tail = tail.saturating_sub(1);
    let wait = if app.mcp.is_some() { Duration::from_millis(50) } else { Duration::from_secs(1) };
    settings.focused_mode = if active { UpdateMode::Continuous } else { UpdateMode::reactive_low_power(wait) };
    settings.unfocused_mode = UpdateMode::reactive_low_power(wait);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_sleeps_but_drawing_and_walking_keep_live_frames() {
        let mut app = App::new();
        app.insert_resource(crate::state::App3d::new(rysmo_model::Editor::new()));
        plugin(&mut app);
        for _ in 0..181 {
            app.update();
        }
        assert_ne!(app.world().resource::<WinitSettings>().focused_mode, UpdateMode::Continuous);
        app.world_mut().resource_mut::<crate::state::App3d>().left_down = true;
        app.update();
        assert_eq!(app.world().resource::<WinitSettings>().focused_mode, UpdateMode::Continuous);
        app.world_mut().resource_mut::<crate::state::App3d>().left_down = false;
        app.update();
        assert_ne!(app.world().resource::<WinitSettings>().focused_mode, UpdateMode::Continuous);
        app.world_mut().resource_mut::<crate::state::App3d>().preferences.sleep_renderer = false;
        app.update();
        assert_eq!(app.world().resource::<WinitSettings>().focused_mode, UpdateMode::Continuous);
    }
}
