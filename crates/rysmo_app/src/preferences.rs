//! Application preferences live outside the model and its undo history.
use bevy_egui::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub show_welcome: bool,
    pub sleep_renderer: bool,
    pub remember_layout: bool,
    pub tool_panel: bool,
    pub touch: bool,
    pub orbit_sensitivity: f64,
    pub zoom_sensitivity: f64,
    pub reverse_zoom: bool,
    pub shift_middle_orbits: bool,
    pub inspector: bool,
    pub inspector_tab: usize,
    pub panels: [bool; 5],
    pub windows: BTreeMap<String, [f32; 4]>,
    pub recent: Vec<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            show_welcome: true,
            sleep_renderer: true,
            remember_layout: true,
            tool_panel: true,
            touch: false,
            orbit_sensitivity: 1.,
            zoom_sensitivity: 1.,
            reverse_zoom: false,
            shift_middle_orbits: false,
            inspector: false,
            inspector_tab: 1,
            panels: [false; 5],
            windows: BTreeMap::new(),
            recent: Vec::new(),
        }
    }
}
impl Preferences {
    fn normalize(&mut self) {
        for n in [&mut self.orbit_sensitivity, &mut self.zoom_sensitivity] {
            *n = if n.is_finite() { n.clamp(0.25, 3.) } else { 1. };
        }
        self.inspector_tab = self.inspector_tab.min(1);
        self.recent.truncate(8);
        self.windows.retain(|_, r| r.iter().all(|v| v.is_finite()) && r[2] >= 80. && r[3] >= 40.);
    }
    pub fn load() -> Self {
        let mut value: Self = read().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        value.normalize();
        value
    }
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn remember(&mut self, name: &str) {
        self.recent.retain(|n| n != name);
        self.recent.insert(0, name.into());
        self.recent.truncate(8);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("RYSMO_PREFERENCES_PATH") {
        return Some(p.into());
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join("Library/Application Support"));
    #[cfg(target_os = "android")]
    let base = bevy::android::ANDROID_APP.get().and_then(|a| a.internal_data_path());
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".config")));
    base.map(|p| p.join("rysmo/preferences.json"))
}
#[cfg(not(target_arch = "wasm32"))]
fn read() -> Option<String> {
    std::fs::read_to_string(path()?).ok()
}
#[cfg(not(target_arch = "wasm32"))]
fn write(text: &str) -> Result<(), String> {
    let path = path().ok_or("No preferences directory is available.")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/web_files.js")]
extern "C" {
    fn load_preferences() -> Option<String>;
    fn save_preferences(text: &str) -> bool;
}
#[cfg(target_arch = "wasm32")]
fn read() -> Option<String> {
    load_preferences()
}
#[cfg(target_arch = "wasm32")]
fn write(text: &str) -> Result<(), String> {
    if save_preferences(text) { Ok(()) } else { Err("Browser storage refused preferences.".into()) }
}

#[derive(Clone, Default)]
pub struct Layout(pub BTreeMap<String, [f32; 4]>);
pub fn layout(ctx: &egui::Context) -> Layout {
    ctx.data_mut(|d| d.get_temp::<Layout>(egui::Id::new("saved-window-layout"))).unwrap_or_default()
}
pub fn set_layout(ctx: &egui::Context, value: Layout) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("saved-window-layout"), value));
}
#[derive(Default)]
pub struct Store {
    initialized: bool,
    saved: String,
    reset_requested: bool,
    pub error: Option<String>,
}
pub fn begin(ctx: &egui::Context, app: &mut crate::state::App3d) {
    if app.preference_store.initialized {
        return;
    }
    if app.preferences.remember_layout {
        set_layout(ctx, Layout(app.preferences.windows.clone()));
    }
    app.preference_store.initialized = true;
}
pub fn end(ctx: &egui::Context, app: &mut crate::state::App3d) {
    if app.preference_store.reset_requested {
        set_layout(ctx, Layout::default());
        ctx.memory_mut(|m| m.reset_areas());
        app.preferences.windows.clear();
        app.preference_store.reset_requested = false;
    }
    app.preferences.touch = app.touch.enabled;
    if app.preferences.remember_layout {
        app.preferences.windows = layout(ctx).0;
        app.preferences.inspector = app.show_entity_info;
        app.preferences.inspector_tab = app.inspector_tab;
        app.preferences.panels =
            [app.show_materials, app.show_tags, app.show_sections, app.show_scenes, app.show_components];
    }
    if ctx.input(|i| i.pointer.any_down()) {
        return;
    }
    let text = serde_json::to_string(&app.preferences).unwrap_or_default();
    if cfg!(test) {
        return;
    }
    if text != app.preference_store.saved {
        let result = write(&text);
        app.preference_store.error = result.err();
        app.preference_store.saved = text;
    }
}
pub fn reset_layout(ctx: &egui::Context, app: &mut crate::state::App3d) {
    app.preference_store.reset_requested = true;
    set_layout(ctx, Layout::default());
    ctx.memory_mut(|m| m.reset_areas());
    app.preferences.windows.clear();
    app.show_entity_info = false;
    app.inspector_tab = 1;
    app.show_materials = false;
    app.show_tags = false;
    app.show_sections = false;
    app.show_scenes = false;
    app.show_components = false;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_preferences_get_new_defaults_and_bad_layouts_are_rejected() {
        let mut p: Preferences =
            serde_json::from_str(r#"{"zoom_sensitivity":100,"windows":{"bad":[0,0,-1,20],"good":[20,30,400,300]}}"#)
                .unwrap();
        p.normalize();
        assert!(p.sleep_renderer && p.show_welcome);
        assert_eq!(p.zoom_sensitivity, 3.);
        assert_eq!(p.windows.len(), 1);
        assert_eq!(serde_json::from_str::<Preferences>(&serde_json::to_string(&p).unwrap()).unwrap(), p);
    }
    #[test]
    fn recent_files_are_deduplicated_and_bounded() {
        let mut p = Preferences::default();
        for i in 0..10 {
            p.remember(&format!("{i}"));
        }
        p.remember("5");
        assert_eq!(p.recent[0], "5");
        assert_eq!(p.recent.len(), 8);
        assert_eq!(p.recent.iter().filter(|n| *n == "5").count(), 1);
    }
}

/// Flush the latest UI snapshot before the native window exits.
pub fn flush(app: &mut crate::state::App3d) {
    if cfg!(test) {
        return;
    }
    if let Ok(text) = serde_json::to_string(&app.preferences) {
        app.preference_store.error = write(&text).err();
    }
}
