//! Object clipboard shortcuts, placement preview and paste into the active context.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DMat4, DVec2, DVec3, DVec4};
use rysmo_model::{Command, Definition, InstanceId, clipboard::Clipboard};
use rysmo_tools::{Pointer, ToolKind};
use std::sync::Arc;
#[derive(Clone)]
pub struct Contents {
    pub snapshot: Arc<Clipboard>,
    generation: Option<u64>,
}
pub struct Placement {
    contents: Contents,
    origin: DVec3,
    revision: u64,
    context: Vec<InstanceId>,
}
impl Placement {
    fn valid(&self, app: &App3d) -> bool {
        self.revision == app.editor.revision() && self.context == app.editor.context()
    }
    fn transform(&self, app: &App3d) -> DMat4 {
        let mut axes = app.editor.context_transform().inverse() * self.contents.snapshot.source_transform;
        axes.w_axis = DVec4::W;
        DMat4::from_translation(self.origin) * axes
    }
}
pub fn copy(app: &mut App3d, cut: bool) {
    crate::texture_view::finish(app, false);
    let elements = app.editor.selected();
    match Clipboard::capture(&app.editor, elements.clone()) {
        Ok(snapshot) => {
            if cut && let Err(error) = app.editor.apply(Command::Erase { elements }) {
                app.message = Some(error.to_string());
                return;
            }
            app.object_clipboard =
                Some(Contents { snapshot: Arc::new(snapshot), generation: Some(app.document_generation) });
            app.paste_place = None;
            app.message = Some(if cut { "Cut objects." } else { "Copied objects." }.into());
        }
        Err(error) => app.message = Some(error.to_string()),
    }
}
pub fn paste(app: &mut App3d, in_place: bool) {
    let Some(contents) = app.object_clipboard.clone() else {
        app.message = Some("The object clipboard is empty.".into());
        return;
    };
    if in_place {
        app.set_tool(ToolKind::Select);
        let transform = app.editor.context_transform().inverse() * contents.snapshot.source_transform;
        commit(app, contents, transform);
    } else {
        begin(app, contents);
    }
}
pub fn duplicate(app: &mut App3d) {
    match Clipboard::capture(&app.editor, app.editor.selected()) {
        Ok(snapshot) => {
            begin(app, Contents { snapshot: Arc::new(snapshot), generation: Some(app.document_generation) })
        }
        Err(error) => app.message = Some(error.to_string()),
    }
}
fn begin(app: &mut App3d, contents: Contents) {
    app.set_tool(ToolKind::Select);
    let origin = app.editor.context_transform().inverse().transform_point3(app.camera.target);
    app.paste_place =
        Some(Placement { contents, origin, revision: app.editor.revision(), context: app.editor.context().to_vec() });
    app.message = Some("Paste: click to place objects. Enter places at the preview; Escape cancels.".into());
    if let Some(cursor) = app.cursor {
        pointer(app, cursor, Pointer::Move);
    }
}
fn commit(app: &mut App3d, contents: Contents, transform: DMat4) {
    match app.editor.apply(Command::Paste {
        clipboard: Box::new((*contents.snapshot).clone()),
        transform,
        reuse_definitions: contents.generation == Some(app.document_generation),
    }) {
        Ok(out) => {
            app.editor.set_selection(out.created);
            app.message = None;
        }
        Err(error) => app.message = Some(error.to_string()),
    }
}
pub fn finish(app: &mut App3d) {
    let Some(p) = app.paste_place.take() else {
        return;
    };
    if !p.valid(app) {
        app.message = Some("The model changed. Paste again.".into());
        return;
    }
    let transform = p.transform(app);
    commit(app, p.contents, transform);
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.paste_place.as_ref().is_some_and(|p| p.valid(app)) {
        app.paste_place = None;
        return;
    }
    app.with_ctx(at, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Move));
    if let Some(point) = app.tools.preview(&app.editor).inference.map(|i| i.point) {
        app.paste_place.as_mut().unwrap().origin = point;
    }
    if matches!(event, Pointer::Down { .. }) {
        finish(app);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(p) = app.paste_place.as_ref().filter(|p| p.valid(app)) else {
        return;
    };
    fn visit(painter: &egui::Painter, app: &App3d, contents: &Contents, def: &Definition, t: DMat4, depth: usize) {
        if depth > 64 {
            return;
        }
        for (_, edge) in def.mesh.edges() {
            if let Some((a, b)) = app.view().project_segment(
                t.transform_point3(def.mesh.pos(edge.v[0])),
                t.transform_point3(def.mesh.pos(edge.v[1])),
            ) {
                painter.line_segment(
                    [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                    egui::Stroke::new(1.8, egui::Color32::from_rgb(245, 165, 65)),
                );
            }
        }
        for instance in def.instances.values() {
            let source = contents.snapshot.document.definitions.get(&instance.definition);
            let definition = if contents.generation == Some(app.document_generation) && source.is_some_and(|d| !d.group)
            {
                app.editor.doc().definitions.get(&instance.definition).or(source)
            } else {
                source
            };
            if let Some(definition) = definition {
                visit(painter, app, contents, definition, t * instance.transform, depth + 1);
            }
        }
    }
    visit(
        painter,
        app,
        &p.contents,
        &p.contents.snapshot.document.root,
        app.editor.context_transform() * p.transform(app),
        0,
    );
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub fn screenshot_preview(app: &mut App3d) {
    let elements: Vec<_> = app.editor.mesh().faces().map(|(f, _)| rysmo_model::Element::Face(f)).collect();
    app.editor.set_selection(elements);
    duplicate(app);
    if let Some(preview) = app.paste_place.as_mut() {
        preview.origin = DVec3::new(0., -3., 0.);
    }
    app.camera.frame(DVec3::new(-0.5, -3.5, 0.), DVec3::new(8.5, 2.5, 1.2));
}

/// Place external geometry without replacing the user's object clipboard.
pub fn place_external(
    app: &mut App3d,
    mut document: rysmo_model::Document,
    name: String,
    preserve_origin: bool,
) -> Result<(), String> {
    if let Some(photo) = document.photo_match.take()
        && !document.materials.values().any(|m| m.images().any(|i| i == photo.image))
    {
        document.images.remove(&photo.image);
    }
    document.camera = None;
    document.scenes.clear();
    let bounds = document.definition_bounds(None).ok_or("Imported component has no geometry.")?;
    if document.root.mesh.vertex_count() == 0
        && document.root.instances.len() == 1
        && document.root.guides.is_empty()
        && document.root.dimensions.is_empty()
        && document.root.texts.is_empty()
        && document.root.sections.planes.is_empty()
    {
        let instance = document.root_mut().instances.values_mut().next().unwrap();
        instance.name = name;
        if !preserve_origin {
            instance.transform = DMat4::from_translation(-bounds.0) * instance.transform;
        }
        begin(
            app,
            Contents {
                snapshot: Arc::new(Clipboard { document, source_transform: DMat4::IDENTITY }),
                generation: None,
            },
        );
        return Ok(());
    }
    let definition = rysmo_model::DefinitionId(document.reserve_id());
    let instance = InstanceId(document.reserve_id());
    let mut root = std::mem::take(&mut document.root);
    let edited = std::sync::Arc::make_mut(&mut root);
    edited.name = name.clone();
    edited.group = false;
    document.definitions.insert(definition, root);
    document.root_mut().instances.insert(
        instance,
        rysmo_model::Instance {
            definition,
            transform: DMat4::from_translation(if preserve_origin { DVec3::ZERO } else { -bounds.0 }),
            name,
            material: None,
            tag: None,
            hidden: false,
            glued_to: None,
        },
    );
    begin(
        app,
        Contents { snapshot: Arc::new(Clipboard { document, source_transform: DMat4::IDENTITY }), generation: None },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::{Editor, Element};
    fn fixture() -> App3d {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::new(2., 3., 1.), u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.set_selection([Element::Face(face)]);
        App3d::new(editor)
    }
    #[test]
    fn cut_clipboard_survives_new_document_and_paste_in_place_is_undoable() {
        let mut app = fixture();
        copy(&mut app, true);
        assert_eq!(app.editor.mesh().faces().count(), 0);
        crate::files::new(&mut app);
        assert!(app.object_clipboard.is_some());
        paste(&mut app, true);
        let face = app.editor.mesh().faces().next().unwrap().0;
        assert!(app.editor.mesh().face_contains(face, DVec3::new(2.5, 3.5, 1.)));
        app.editor.undo();
        assert_eq!(app.editor.mesh().faces().count(), 0);
        app.editor.redo();
        assert_eq!(app.editor.mesh().faces().count(), 1);
    }
    #[test]
    fn canceled_or_stale_paste_creates_no_geometry_or_history() {
        let mut app = fixture();
        copy(&mut app, false);
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        paste(&mut app, false);
        assert!(app.paste_place.is_some());
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.paste_place.is_none());
        assert_eq!(app.editor.undo_depth(), depth);
        paste(&mut app, false);
        app.editor.apply(Command::AddMaterial { name: "Unrelated".into(), color: [1.; 4], texture: None }).unwrap();
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        finish(&mut app);
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_depth(), depth);
    }
    #[test]
    fn duplicate_preserves_clipboard_and_paste_in_place_preserves_world_position_in_group() {
        let mut app = fixture();
        copy(&mut app, false);
        let clipboard = app.object_clipboard.as_ref().unwrap().snapshot.clone();
        let made = app.editor.apply(Command::MakeGroup { elements: app.editor.selected() }).unwrap();
        let Element::Instance(group) = made.created[0] else { panic!() };
        app.editor.set_selection(made.created);
        duplicate(&mut app);
        assert!(Arc::ptr_eq(&clipboard, &app.object_clipboard.as_ref().unwrap().snapshot));
        app.set_tool(ToolKind::Select);
        app.editor
            .apply(Command::Rotate {
                elements: vec![Element::Instance(group)],
                centre: DVec3::ZERO,
                axis: DVec3::Z,
                angle: 30.,
                copies: 0,
            })
            .unwrap();
        app.editor.enter(group).unwrap();
        paste(&mut app, true);
        let p = app.editor.context_transform().inverse().transform_point3(DVec3::new(2.5, 3.5, 1.));
        assert!(app.editor.mesh().faces().any(|(f, _)| app.editor.mesh().face_contains(f, p)));
    }
}
