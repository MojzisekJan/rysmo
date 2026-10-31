//! Independent editing sessions with shared application preferences and clipboard.
use crate::state::App3d;
use bevy_egui::egui;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub struct Documents {
    pub active: u64,
    inactive: BTreeMap<u64, Box<App3d>>,
}
impl Default for Documents {
    fn default() -> Self {
        Self { active: generation(), inactive: BTreeMap::new() }
    }
}
impl Documents {
    #[cfg(target_arch = "wasm32")]
    pub fn dirty(&self) -> bool {
        self.inactive.values().any(|app| app.editor.is_dirty())
    }
    #[cfg(target_arch = "wasm32")]
    pub fn sessions<'a>(&'a self, active: &'a App3d) -> impl Iterator<Item = (u64, &'a App3d)> {
        std::iter::once((self.active, active)).chain(self.inactive.iter().map(|(id, app)| (*id, app.as_ref())))
    }
    #[cfg(target_arch = "wasm32")]
    pub fn session_mut(&mut self, generation: u64) -> Option<&mut App3d> {
        self.inactive.values_mut().find(|app| app.document_generation == generation).map(Box::as_mut)
    }
    pub fn single(&self) -> bool {
        self.inactive.is_empty()
    }
    pub fn full(&self) -> bool {
        self.inactive.len() >= 15
    }
    pub fn first_dirty(&self) -> Option<u64> {
        self.inactive.iter().find_map(|(id, app)| app.editor.is_dirty().then_some(*id))
    }
}

fn shared(from: &mut App3d, to: &mut App3d) {
    std::mem::swap(&mut from.preference_store, &mut to.preference_store);
    std::mem::swap(&mut from.features.library, &mut to.features.library);
    std::mem::swap(&mut from.help, &mut to.help);
    to.preferences = from.preferences.clone();
    to.object_clipboard = from.object_clipboard.clone();
    to.recent = from.recent.clone();
    to.mcp = from.mcp.clone();
    to.held = from.held;
    to.cursor = from.cursor;
    to.hover_at = None;
    to.left_down = false;
    to.last_click = None;
    to.context_menu = None;
    to.touch.gestures = Default::default();
    to.viewport = from.viewport;
    to.canvas = from.canvas;
    to.touch.enabled = from.touch.enabled;
    to.show_settings = from.show_settings;
    to.settings_tab = from.settings_tab;
    to.show_entity_info = from.show_entity_info;
    to.inspector_tab = from.inspector_tab;
    #[cfg(target_arch = "wasm32")]
    {
        to.autosave_settings = from.autosave_settings;
    }
}

pub fn switch(app: &mut App3d, id: u64) {
    if id == app.documents.active || app.unsaved.active() {
        return;
    }
    let mut tabs = std::mem::take(&mut app.documents);
    let Some(mut next) = tabs.inactive.remove(&id) else {
        app.documents = tabs;
        return;
    };
    crate::texture_view::finish(app, false);
    app.set_tool(app.tools.active());
    crate::features::parametric::cancel_resize(app);
    shared(app, &mut next);
    std::mem::swap(app, &mut next);
    tabs.inactive.insert(tabs.active, next);
    tabs.active = id;
    app.documents = tabs;
    #[cfg(target_arch = "wasm32")]
    {
        crate::web::activate_tab(id);
        crate::web::persist_tabs(app);
    }
}

/// Preserve the current model before opening a new document.
pub fn create(app: &mut App3d) -> bool {
    if !app.editor.is_dirty()
        && app.path.is_none()
        && (app.welcome || (app.documents.inactive.is_empty() && app.editor.doc() == &rysmo_model::Document::new()))
    {
        return true;
    }
    if app.documents.full() {
        app.message = Some("Close a document before opening more than 16 models.".into());
        return false;
    }
    crate::texture_view::finish(app, false);
    app.set_tool(app.tools.active());
    crate::features::parametric::cancel_resize(app);
    let mut tabs = std::mem::take(&mut app.documents);
    let mut next = Box::new(App3d::new(rysmo_model::Editor::new()));
    shared(app, &mut next);
    next.welcome = false;
    let id = next.documents.active;
    std::mem::swap(app, &mut next);
    tabs.inactive.insert(tabs.active, next);
    tabs.active = id;
    app.documents = tabs;
    #[cfg(target_arch = "wasm32")]
    crate::web::activate_tab(id);
    true
}

pub fn close(app: &mut App3d) {
    let old = app.documents.active;
    if let Some(id) = app.documents.inactive.keys().next().copied() {
        switch(app, id);
        app.documents.inactive.remove(&old);
        #[cfg(target_arch = "wasm32")]
        crate::web::forget_tab(old);
        #[cfg(target_arch = "wasm32")]
        crate::web::persist_tabs(app);
    } else {
        app.editor.mark_saved();
        app.welcome = true;
        crate::files::new(app);
        app.welcome = true;
    }
}

/// Tabs for switching documents. A single document needs no tab row.
pub fn panel(ui: &mut egui::Ui, app: &mut App3d) {
    if app.documents.single() {
        return;
    }
    let mut tabs: Vec<_> = app.documents.inactive.iter().map(|(id, a)| (*id, title(a))).collect();
    tabs.push((app.documents.active, title(app)));
    tabs.sort_by_key(|(id, _)| *id);
    let mut activate = None;
    let mut close = false;
    ui.horizontal_wrapped(|ui| {
        for (id, name) in tabs {
            if ui.selectable_label(id == app.documents.active, name).clicked() {
                activate = Some(id);
            }
        }
        close = ui.small_button("×").on_hover_text("Close active document").clicked();
    });
    if let Some(id) = activate {
        switch(app, id);
    }
    if close {
        crate::unsaved::request(app, crate::unsaved::Action::CloseTab);
    }
}
fn title(app: &App3d) -> String {
    let name = app
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into());
    format!("{}{}", name, if app.editor.is_dirty() { " *" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};
    #[test]
    fn tabs_preserve_independent_undo_camera_paths_and_clipboard() {
        let mut app = App3d::new(Editor::new());
        app.welcome = false;
        let first = app.documents.active;
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        app.path = Some("first.rysmo".into());
        let camera = app.camera;
        assert!(create(&mut app));
        let second = app.documents.active;
        assert_ne!(first, second);
        assert_eq!(app.editor.mesh().edge_count(), 0);
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::Y * 2.]], facing: None }).unwrap();
        app.path = Some("second.rysmo".into());
        switch(&mut app, first);
        assert_eq!(app.camera, camera);
        assert_eq!(app.path, Some("first.rysmo".into()));
        assert_eq!(app.editor.mesh().edge_count(), 1);
        app.editor.undo();
        assert_eq!(app.editor.mesh().edge_count(), 0);
        switch(&mut app, second);
        assert_eq!(app.path, Some("second.rysmo".into()));
        assert_eq!(app.editor.mesh().edge_count(), 1);
    }
    #[test]
    fn tab_row_appears_only_with_several_documents() {
        let mut app = App3d::new(Editor::new());
        app.welcome = false;
        assert!(app.documents.single());
        app.editor.mark_dirty();
        assert!(create(&mut app));
        assert!(!app.documents.single());
        close(&mut app);
        assert!(app.documents.single());
    }
    #[test]
    fn closing_app_checks_dirty_background_documents() {
        let mut app = App3d::new(Editor::new());
        app.editor.mark_dirty();
        let first = app.documents.active;
        assert!(create(&mut app));
        crate::unsaved::request(&mut app, crate::unsaved::Action::Close);
        assert_eq!(app.documents.active, first);
        assert!(app.unsaved.active());
        assert!(!app.unsaved.close_ready);
    }
}
