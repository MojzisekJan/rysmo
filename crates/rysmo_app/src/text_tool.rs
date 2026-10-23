//! Staged Text creation. Drafts never enter history until explicitly completed.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::DVec2;
use rysmo_model::text::{Text, TextAnchor, TextDefaults, TextLeader, TextPlacement, TextReference};
use rysmo_model::{Command, Element, InstanceId};
use rysmo_tools::Pointer;

pub struct TextCreation {
    pub text: Text,
    pub placing: bool,
    pub focus: bool,
    pub context: Vec<InstanceId>,
    pub revision: u64,
    defaults: TextDefaults,
    press: Option<DVec2>,
}
impl TextCreation {
    pub(crate) fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
}

pub fn finish(app: &mut App3d) {
    let Some(draft) = app.text_creation.take() else { return };
    if !draft.valid(app) {
        app.message = Some("Text creation cancelled because the model changed.".into());
        return;
    }
    if draft.text.content.trim().is_empty() {
        return;
    }
    match app.editor.apply(Command::AddText { text: draft.text }) {
        Ok(out) => app.editor.set_selection(out.created),
        Err(error) => app.message = Some(error.to_string()),
    }
}

fn default_content(app: &App3d, anchor: &TextAnchor, fallback: &str) -> String {
    use rysmo_model::dimensions::PointEntity;
    let doc = app.editor.doc();
    let (path, entity) = match &anchor.reference {
        Some(TextReference::Point { reference }) => (&reference.path, Some(reference.entity)),
        Some(TextReference::Face { path, .. }) => (path, None),
        None => return fallback.into(),
    };
    let mut owner = app.editor.active_definition();
    let mut transform = app.editor.context_transform();
    for id in path {
        let Some(instance) = doc.def(owner).and_then(|d| d.instances.get(id)) else { return fallback.into() };
        let Some(definition) = doc.def(Some(instance.definition)) else { return fallback.into() };
        if !instance.name.trim().is_empty() {
            return instance.name.clone();
        }
        if !definition.group && !definition.name.trim().is_empty() {
            return definition.name.clone();
        }
        transform *= instance.transform;
        owner = Some(instance.definition);
    }
    if let Some(PointEntity::Edge { edge, .. }) = entity
        && let Some(mesh) = doc.def(owner).map(|d| &d.mesh)
        && mesh.edge(edge).is_some()
    {
        let (a, b) = mesh.edge_points(edge);
        return doc.format_length(transform.transform_point3(a).distance(transform.transform_point3(b)));
    }
    if !fallback.is_empty() {
        return fallback.into();
    }
    let point = app.editor.context_transform().transform_point3(anchor.resolve(doc, app.editor.active_definition()).0);
    format!("({}, {}, {})", doc.format_length(point.x), doc.format_length(point.y), doc.format_length(point.z))
}

pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if app.text_creation.as_ref().is_some_and(|draft| !draft.valid(app)) {
        app.text_creation = None;
        app.message = Some("Text creation cancelled because the model changed.".into());
        return;
    }
    app.with_ctx(at, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Move));
    if let Pointer::Down { clicks } = event
        && (clicks >= 2 || app.text_creation.is_none())
    {
        for _ in 0..=64 {
            match app.pick_annotation(at) {
                Some(rysmo_infer::Pick::Element(element @ (Element::Text(_) | Element::Dimension(_)))) => {
                    app.text_creation = None;
                    app.editor.set_selection([element]);
                    if clicks >= 2 {
                        app.show_entity_info = true;
                    }
                    return;
                }
                Some(rysmo_infer::Pick::Element(Element::Instance(instance))) => {
                    app.text_creation = None;
                    if clicks < 2 {
                        app.editor.set_selection([Element::Instance(instance)]);
                        return;
                    }
                    if let Err(error) = app.editor.enter(instance) {
                        app.message = Some(error.to_string());
                        return;
                    }
                }
                _ => break,
            }
        }
    }
    if matches!(event, Pointer::Down { clicks: 2.. }) && app.text_creation.as_ref().is_some_and(|draft| draft.placing) {
        app.text_creation = None;
    }
    if let Some(mut draft) = app.text_creation.take() {
        if draft.placing {
            if (!matches!(event, Pointer::Up)
                || draft.press.is_some_and(|start| start.distance(at) > 6. * app.pick_scale()))
                && let TextPlacement::Attached { anchor, .. } = &draft.text.placement
                && let Some(text) = draft.defaults.attached(
                    draft.text.content.clone(),
                    rysmo_model::text::AttachmentRequest {
                        anchor: anchor.clone(),
                        position: at,
                        doc: app.editor.doc(),
                        owner: app.editor.active_definition(),
                        transform: app.editor.context_transform(),
                        view: &app.view(),
                    },
                )
            {
                draft.text = text;
            }
            if matches!(event, Pointer::Down { .. })
                || matches!(event, Pointer::Up)
                    && draft.press.is_some_and(|start| start.distance(at) > 6. * app.pick_scale())
            {
                draft.placing = false;
                draft.focus = true;
            }
            if matches!(event, Pointer::Up) {
                draft.press = None;
            }
            app.text_creation = Some(draft);
        } else {
            app.text_creation = Some(draft);
            if matches!(event, Pointer::Down { .. }) {
                finish(app);
            }
        }
        return;
    }
    let Pointer::Down { clicks } = event else { return };
    if clicks >= 2
        && let Some(rysmo_infer::Pick::Element(Element::Text(id))) = app.pick_annotation(at)
    {
        app.editor.set_selection([Element::Text(id)]);
        app.show_entity_info = true;
        return;
    }
    let defaults = app.editor.doc().style.text_defaults;
    let doc = app.editor.doc();
    let view = app.view();
    let mut content = String::new();
    let mut anchor = app.tools.inferred_anchor(&app.editor).and_then(|a| {
        a.reference.map(|reference| TextAnchor { point: a.point, reference: Some(TextReference::Point { reference }) })
    });
    let face = doc
        .occurrences(app.editor.context())
        .into_iter()
        .filter(|o| o.in_context)
        .filter_map(|occurrence| {
            let mesh = doc.effective_mesh(occurrence.definition);
            let ray = view.local(occurrence.transform).ray(at);
            let (face, depth) = mesh.pick_face(&ray)?;
            let point = occurrence.transform.transform_point3(ray.at(depth));
            Some((point.distance_squared(view.eye), occurrence, face, point))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let mut direct = false;
    if let Some((_, occurrence, face, world)) = face
        && (anchor.is_none() || clicks >= 2)
    {
        anchor = TextAnchor::on_face(
            doc,
            app.editor.active_definition(),
            occurrence.path[app.editor.context().len()..].to_vec(),
            face,
            app.editor.context_transform().inverse().transform_point3(world),
        );
        let mesh = doc.effective_mesh(occurrence.definition);
        let normal = mesh.face_plane(face).unwrap().normal;
        let area = mesh.face_area(face)
            * occurrence.transform.determinant().abs()
            * occurrence.transform.inverse().transpose().transform_vector3(normal).length();
        content = doc.format_area(area);
        direct = clicks >= 2;
    }
    let text = if let Some(anchor) = anchor {
        if !direct {
            content = default_content(app, &anchor, &content);
        }
        if content.is_empty() {
            content = default_content(app, &anchor, "");
        }
        if direct {
            Some(Text {
                content,
                placement: TextPlacement::Attached {
                    anchor,
                    leader: TextLeader::None { offset: rysmo_math::DVec3::ZERO },
                },
                font: defaults.leader_font,
                endpoint: defaults.endpoint,
            })
        } else {
            defaults.attached(
                content,
                rysmo_model::text::AttachmentRequest {
                    anchor,
                    position: at + DVec2::new(80., -40.),
                    doc,
                    owner: app.editor.active_definition(),
                    transform: app.editor.context_transform(),
                    view: &view,
                },
            )
        }
    } else {
        defaults.screen(content, at - app.canvas_origin(), app.canvas_size())
    };
    if let Some(text) = text {
        let placing = matches!(
            text.placement,
            TextPlacement::Attached { leader: TextLeader::ViewBased { .. } | TextLeader::Pushpin { .. }, .. }
        );
        app.text_creation = Some(TextCreation {
            text,
            placing,
            focus: !placing,
            context: app.editor.context().to_vec(),
            revision: app.editor.revision(),
            defaults,
            press: Some(at),
        });
    }
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if app.text_creation.as_ref().is_some_and(|draft| !draft.valid(app)) {
        app.text_creation = None;
        app.message = Some("Text creation cancelled because the model changed.".into());
    }
    if app.text_creation.as_ref().is_none_or(|draft| draft.placing) {
        return;
    }
    let mut open = true;
    let mut submit = false;
    let mut cancel = false;
    crate::theme::window("New text").open(&mut open).default_width(320.).show(ctx, |ui| {
        let draft = app.text_creation.as_mut().unwrap();
        let id = egui::Id::unique("new-text-content");
        // Enter keeps the selected suggestion: it moves to the end instead of replacing it,
        // so Enter twice accepts the suggested area or length while typing still replaces it.
        if ui.memory(|m| m.has_focus(id)) && ui.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::Enter)) {
            let len = draft.text.content.chars().count();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id)
                && len > 0
                && state.cursor.char_range().is_some_and(|r| {
                    let (a, b) = (r.primary.index.min(r.secondary.index), r.primary.index.max(r.secondary.index));
                    a.0 == 0 && b.0 == len
                })
            {
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(len))));
                state.store(ui.ctx(), id);
                draft.focus = false;
            }
        }
        let response = ui.add(crate::theme::multiline(&mut draft.text.content).desired_width(f32::INFINITY).id(id));
        // A new window's first frame only measures it, and the release of the click that
        // placed the text would take focus away; keep asking until both are past.
        if draft.focus {
            response.request_focus();
            // The suggested content (an area, a length) is selected, so typing replaces it.
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            let all = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(draft.text.content.chars().count()),
            );
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), id);
            draft.focus = !response.has_focus() || ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
        }
        ui.small("Enter adds a line. Enter twice or Ctrl+Enter finishes. Escape cancels.");
        submit = ui.add_enabled(!draft.text.content.trim().is_empty(), egui::Button::new("Create text")).clicked()
            || response.has_focus() && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
        if response.has_focus()
            && ui.input(|i| !i.modifiers.ctrl && i.key_pressed(egui::Key::Enter))
            && draft.text.content.ends_with("\n\n")
        {
            draft.text.content = draft.text.content.trim_end_matches('\n').into();
            submit = true;
        }
        cancel = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
    });
    if !open || cancel {
        app.text_creation = None;
    } else if submit {
        finish(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::{DMat4, DVec3};
    use rysmo_tools::{Key, ToolKind};
    fn app() -> App3d {
        let doc = rysmo_io::load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let mut app = App3d::new(rysmo_model::Editor::with_document(doc));
        app.set_tool(ToolKind::Text);
        app
    }
    #[test]
    fn screen_text_creation_cancel_commit_and_undo() {
        let mut app = app();
        let before = app.editor.doc().clone();
        let at = DVec2::new(100., 100.);
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(!app.text_creation.as_ref().unwrap().placing);
        assert!(matches!(app.text_creation.as_ref().unwrap().text.placement, TextPlacement::Screen { .. }));
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_key(&mut app, Key::Escape);
        assert!(app.text_creation.is_none());
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        app.text_creation.as_mut().unwrap().text.content = "Poznámka\nDruhý řádek".into();
        finish(&mut app);
        let text = app.editor.active().texts.values().next().unwrap();
        assert_eq!(text.layout(app.editor.doc(), None, DMat4::IDENTITY, &app.view()).unwrap().position, at);
        assert_eq!(app.editor.undo_label(), Some("Text"));
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn face_leader_placement_defaults_preview_and_double_click_area() {
        let mut app = app();
        let before = app.editor.doc().clone();
        let at = app.view().project(DVec3::new(0.7, 0.6, 0.)).unwrap();
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.text_creation.as_ref().unwrap().placing);
        crate::input::tool_pointer(&mut app, at, Pointer::Up);
        let end = at + DVec2::new(150., -80.);
        crate::input::tool_pointer(&mut app, end, Pointer::Move);
        let draft = app.text_creation.as_ref().unwrap();
        assert!(
            draft.text.layout(app.editor.doc(), None, DMat4::IDENTITY, &app.view()).unwrap().position.distance(end)
                < 1e-8
        );
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_pointer(&mut app, end, Pointer::Down { clicks: 1 });
        assert!(!app.text_creation.as_ref().unwrap().placing);
        finish(&mut app);
        assert!(app.editor.active().texts.values().next().unwrap().content.contains("3.000 m²"));
        app.editor.undo();
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        crate::input::tool_pointer(&mut app, at, Pointer::Up);
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 2 });
        assert!(!app.text_creation.as_ref().unwrap().placing);
        assert!(matches!(
            app.text_creation.as_ref().unwrap().text.placement,
            TextPlacement::Attached { leader: TextLeader::None { .. }, .. }
        ));
    }
    #[test]
    fn model_changes_and_tool_changes_discard_uncommitted_drafts() {
        let mut app = app();
        let at = DVec2::new(100., 100.);
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        app.text_creation.as_mut().unwrap().text.content = "Stale".into();
        app.editor.apply(Command::AddGuidePoint { point: DVec3::ZERO }).unwrap();
        finish(&mut app);
        assert!(app.editor.active().texts.is_empty());
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        app.set_tool(ToolKind::Select);
        assert!(app.text_creation.is_none());
    }
    #[test]
    fn text_entry_panel_confirmation_creates_one_undo_step() {
        let mut app = app();
        let before = app.editor.doc().clone();
        crate::input::tool_pointer(&mut app, DVec2::new(100., 100.), Pointer::Down { clicks: 1 });
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input(), |ui| panel(ui.ctx(), &mut app));
        output.textures_delta.clear();
        app.text_creation.as_mut().unwrap().text.content = "Nový popisek".into();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique("new-text-content")));
        let modifiers = egui::Modifiers { ctrl: true, ..Default::default() };
        let mut pressed = input();
        pressed.events.push(egui::Event::ModifiersChanged(modifiers));
        pressed.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        let mut output = ctx.run_ui(pressed, |ui| panel(ui.ctx(), &mut app));
        output.textures_delta.clear();
        assert!(app.text_creation.is_none());
        assert_eq!(app.editor.active().texts.values().next().unwrap().content, "Nový popisek");
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn two_enter_presses_complete_text_entry() {
        let mut app = app();
        crate::input::tool_pointer(&mut app, DVec2::new(100., 100.), Pointer::Down { clicks: 1 });
        let ctx = egui::Context::default();
        let frame = |app: &mut App3d, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| panel(ui.ctx(), app));
            output.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![egui::Event::Text("Typed text".into())]);
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        frame(&mut app, vec![key(true)]);
        assert!(app.text_creation.is_some());
        frame(&mut app, vec![key(false)]);
        frame(&mut app, vec![key(true)]);
        assert!(app.text_creation.is_none());
        assert_eq!(app.editor.active().texts.values().next().unwrap().content, "Typed text");
    }
    #[test]
    fn two_enter_presses_accept_the_selected_suggestion() {
        let mut app = app();
        crate::input::tool_pointer(&mut app, DVec2::new(100., 100.), Pointer::Down { clicks: 1 });
        app.text_creation.as_mut().unwrap().text.content = "2.000 m²".into();
        let ctx = egui::Context::default();
        let frame = |app: &mut App3d, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| panel(ui.ctx(), app));
            output.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        frame(&mut app, vec![key(true)]);
        frame(&mut app, vec![key(false)]);
        frame(&mut app, vec![key(true)]);
        assert!(app.text_creation.is_none());
        assert_eq!(app.editor.active().texts.values().next().unwrap().content, "2.000 m²");
    }
    #[test]
    fn drag_release_places_text_at_release_position() {
        let mut app = app();
        let at = app.view().project(DVec3::new(0.7, 0.6, 0.)).unwrap();
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        let end = at + DVec2::new(140., -70.);
        crate::input::tool_pointer(&mut app, end, Pointer::Up);
        let draft = app.text_creation.as_ref().unwrap();
        assert!(!draft.placing);
        assert!(
            draft.text.layout(app.editor.doc(), None, DMat4::IDENTITY, &app.view()).unwrap().position.distance(end)
                < 1e-8
        );
        assert!(app.editor.active().texts.is_empty());
    }
    #[test]
    fn creation_in_scaled_group_uses_local_anchors_world_area_and_correct_owner() {
        let mut editor = rysmo_model::Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: None })
            .unwrap();
        let elements = editor
            .mesh()
            .edges()
            .map(|(id, _)| Element::Edge(id))
            .chain(editor.mesh().faces().map(|(id, _)| Element::Face(id)))
            .collect();
        let Element::Instance(group) = editor.apply(Command::MakeGroup { elements }).unwrap().created[0] else {
            panic!()
        };
        editor
            .apply(Command::Scale {
                elements: vec![Element::Instance(group)],
                origin: DVec3::ZERO,
                factors: DVec3::new(2., 3., 1.),
            })
            .unwrap();
        editor.enter(group).unwrap();
        let mut app = App3d::new(editor);
        app.set_tool(ToolKind::Text);
        let local = DVec3::new(0.7, 0.6, 0.);
        let world = app.editor.context_transform().transform_point3(local);
        let at = app.view().project(world).unwrap();
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 2 });
        let draft = app.text_creation.as_ref().unwrap();
        assert_eq!(draft.text.content, "12.000 m²");
        let TextPlacement::Attached { anchor, .. } = &draft.text.placement else { panic!() };
        assert!(anchor.resolve(app.editor.doc(), app.editor.active_definition()).0.distance(local) < 1e-8);
        finish(&mut app);
        assert!(app.editor.doc().root.texts.is_empty());
        let owner = app.editor.active_definition();
        let id = *app.editor.active().texts.keys().next().unwrap();
        app.editor.exit();
        app.editor.apply(Command::Move { elements: vec![Element::Instance(group)], offset: DVec3::Z }).unwrap();
        let occurrence = app.editor.doc().occurrences(&[]).into_iter().find(|o| o.definition == owner).unwrap();
        let text = &app.editor.doc().def(owner).unwrap().texts[&id];
        let TextPlacement::Attached { anchor, .. } = &text.placement else { panic!() };
        assert!(anchor.resolve(app.editor.doc(), owner).1);
        let layout = text.layout(app.editor.doc(), owner, occurrence.transform, &app.view()).unwrap();
        assert!(layout.position.distance(app.view().project(world + DVec3::Z).unwrap()) < 1e-8);
    }
    #[test]
    fn double_click_group_annotation_opens_its_editor_without_duplicate_text() {
        let mut editor = rysmo_model::Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: None })
            .unwrap();
        let elements = editor
            .mesh()
            .edges()
            .map(|(id, _)| Element::Edge(id))
            .chain(editor.mesh().faces().map(|(id, _)| Element::Face(id)))
            .collect();
        let Element::Instance(group) = editor.apply(Command::MakeGroup { elements }).unwrap().created[0] else {
            panic!()
        };
        editor.enter(group).unwrap();
        let text = Text {
            content: "Inside group".into(),
            placement: TextPlacement::Attached {
                anchor: TextAnchor { point: DVec3::ZERO, reference: None },
                leader: TextLeader::ViewBased { offset: DVec2::new(150., -100.) },
            },
            font: Default::default(),
            endpoint: Default::default(),
        };
        let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
        let owner = editor.active_definition();
        editor.exit();
        let mut app = App3d::new(editor);
        app.set_tool(ToolKind::Text);
        let occurrence = app.editor.doc().occurrences(&[]).into_iter().find(|o| o.definition == owner).unwrap();
        let layout = text.layout(app.editor.doc(), owner, occurrence.transform, &app.view()).unwrap();
        let size = text.estimated_size();
        let at = text.geometry(&layout, size).unwrap().rect.centre;
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.text_creation.is_none());
        crate::input::tool_pointer(&mut app, at, Pointer::Up);
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 2 });
        assert_eq!(app.editor.context(), &[group]);
        assert!(app.editor.is_selected(Element::Text(id)));
        assert!(app.show_entity_info);
        assert!(app.text_creation.is_none());
        assert_eq!(app.editor.active().texts.len(), 1);
        assert!(app.editor.doc().root.texts.is_empty());
    }
    #[test]
    fn default_edge_content_respects_occurrence_scale_and_model_units() {
        use rysmo_model::dimensions::{PointEntity, PointReference};
        let mut app = app();
        let edge = app
            .editor
            .mesh()
            .edges()
            .find(|(_, e)| e.v == [rysmo_model::VertexId(1), rysmo_model::VertexId(2)])
            .unwrap()
            .0;
        let anchor = TextAnchor {
            point: DVec3::X,
            reference: Some(TextReference::Point {
                reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
            }),
        };
        assert_eq!(default_content(&app, &anchor, ""), "2000 mm");
        app.editor.apply(Command::SetUnits { units: rysmo_model::Units::Metres }).unwrap();
        assert_eq!(default_content(&app, &anchor, ""), "2 m");
        let face = rysmo_model::FaceId(9);
        let elements =
            app.editor.mesh().face_edges(face).into_iter().map(Element::Edge).chain([Element::Face(face)]).collect();
        let Element::Instance(group) = app.editor.apply(Command::MakeGroup { elements }).unwrap().created[0] else {
            panic!()
        };
        app.editor
            .apply(Command::Scale {
                elements: vec![Element::Instance(group)],
                origin: DVec3::ZERO,
                factors: DVec3::new(3., 1., 1.),
            })
            .unwrap();
        app.editor.enter(group).unwrap();
        let edge = app
            .editor
            .mesh()
            .edges()
            .find(|(id, _)| {
                let (a, b) = app.editor.mesh().edge_points(*id);
                (a.distance(b) - 2.).abs() < 1e-8
            })
            .unwrap()
            .0;
        let anchor = TextAnchor {
            point: DVec3::X,
            reference: Some(TextReference::Point {
                reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
            }),
        };
        assert_eq!(default_content(&app, &anchor, ""), "6 m");
    }
    #[test]
    fn component_name_and_instance_name_override_geometry_defaults() {
        use rysmo_model::dimensions::{PointEntity, PointReference};
        let mut editor = rysmo_model::Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let elements = editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect();
        let Element::Instance(instance) =
            editor.apply(Command::MakeComponent { elements, name: Some("Door".into()) }).unwrap().created[0]
        else {
            panic!()
        };
        let definition = editor.doc().root.instances[&instance].definition;
        let edge = editor.doc().def(Some(definition)).unwrap().mesh.edges().next().unwrap().0;
        let anchor = TextAnchor {
            point: DVec3::X * 0.5,
            reference: Some(TextReference::Point {
                reference: PointReference { path: vec![instance], entity: PointEntity::Edge { edge, parameter: 0.5 } },
            }),
        };
        let mut app = App3d::new(editor);
        assert_eq!(default_content(&app, &anchor, "1 m"), "Door");
        app.editor.apply(Command::RenameInstance { instance, name: "Door A".into() }).unwrap();
        assert_eq!(default_content(&app, &anchor, "1 m"), "Door A");
    }
}
