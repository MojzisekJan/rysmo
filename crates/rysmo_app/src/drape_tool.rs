//! Drape preview on a closed terrain; confirmation creates parent-context edges.
use crate::state::App3d;
use rysmo_math::DVec2;
use rysmo_model::{Command, Editor, Element, InstanceId};
use rysmo_tools::{Pointer, ToolKind};
pub struct Draft {
    sources: Vec<Element>,
    pub preview: Option<(Element, Editor)>,
    context: Vec<InstanceId>,
    revision: u64,
}
impl Draft {
    pub fn status(&self) -> &'static str {
        if self.sources.is_empty() {
            "Drape: click source edges, a face or a closed group. Escape cancels."
        } else {
            "Drape: hover terrain triangles or a closed terrain group for preview and click to project. Escape cancels."
        }
    }
    pub fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
}
pub fn begin(app: &mut App3d) -> Result<(), String> {
    let sources = app.editor.selection().iter().copied().collect::<Vec<_>>();
    let lines = rysmo_model::terrain_footprint::source_segments(
        app.editor.doc(),
        app.editor.active_definition(),
        &sources,
        rysmo_math::DMat4::IDENTITY,
    )?;
    if !sources.is_empty() && lines.is_empty() {
        return Err("Select source edges, faces or a closed group first.".into());
    }
    app.set_tool(ToolKind::Select);
    app.drape =
        Some(Draft { sources, preview: None, context: app.editor.context().to_vec(), revision: app.editor.revision() });
    Ok(())
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.drape.as_ref().is_some_and(|d| d.valid(app)) {
        app.drape = None;
        return;
    }
    if app.drape.as_ref().unwrap().sources.is_empty() {
        if matches!(event, Pointer::Down { .. }) {
            let pick = app.with_ctx(at, |tools, editor, ctx| tools.pick_full(editor, ctx));
            if let rysmo_infer::Pick::Element(element) = pick {
                let sources = if let Element::Edge(id) = element {
                    if let Some(curve) = app.editor.mesh().edge(id).and_then(|edge| edge.curve) {
                        app.editor.mesh().curve_edges(curve).into_iter().map(Element::Edge).collect()
                    } else {
                        vec![element]
                    }
                } else {
                    vec![element]
                };
                match rysmo_model::terrain_footprint::source_segments(
                    app.editor.doc(),
                    app.editor.active_definition(),
                    &sources,
                    rysmo_math::DMat4::IDENTITY,
                ) {
                    Ok(lines) if !lines.is_empty() => {
                        app.editor.set_selection(sources);
                        app.message = begin(app).err();
                    }
                    _ => app.message = Some("Click source edges, a face or a closed group.".into()),
                }
            }
        }
        return;
    }
    let Some((target, _)) = crate::stamp_tool::visible_face_pick(app, at) else {
        app.drape.as_mut().unwrap().preview = None;
        return;
    };
    let sources = app.drape.as_ref().unwrap().sources.clone();
    let command = match target {
        Element::Instance(target) => Command::DrapeSources { sources, target },
        Element::Face(target) => Command::DrapeOnFace { sources, target },
        _ => return,
    };
    if app.drape.as_ref().unwrap().preview.as_ref().is_none_or(|(id, _)| *id != target) {
        let mut preview = app.editor.clone();
        match preview.apply(command.clone()) {
            Ok(_) => app.drape.as_mut().unwrap().preview = Some((target, preview)),
            Err(error) => {
                app.drape.as_mut().unwrap().preview = None;
                if matches!(event, Pointer::Down { .. }) {
                    app.message = Some(error.to_string());
                }
                return;
            }
        }
    }
    if matches!(event, Pointer::Down { .. }) {
        match app.editor.apply(command) {
            Ok(_) => {
                app.drape = None;
                app.message = None;
            }
            Err(error) => app.message = Some(error.to_string()),
        }
    }
}
pub fn draw(painter: &bevy_egui::egui::Painter, app: &App3d) {
    let Some(draft) = &app.drape else { return };
    if !draft.valid(app) {
        return;
    }
    let Some((_, preview)) = &draft.preview else { return };
    let view = app.local_view();
    for (id, edge) in preview.mesh().edges() {
        if app.editor.mesh().edge(id).is_some() || edge.hidden {
            continue;
        }
        if let (Some(a), Some(b)) =
            (view.project(preview.mesh().pos(edge.v[0])), view.project(preview.mesh().pos(edge.v[1])))
        {
            painter.line_segment(
                [bevy_egui::egui::pos2(a.x as f32, a.y as f32), bevy_egui::egui::pos2(b.x as f32, b.y as f32)],
                bevy_egui::egui::Stroke::new(2., bevy_egui::egui::Color32::MAGENTA),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn model_or_context_changes_cancel_stale_projection_without_committing() {
        for context_change in [false, true] {
            let mut app = App3d::new(Editor::new());
            let out = app
                .editor
                .apply(Command::CreateTerrainGrid {
                    origin: DVec3::ZERO,
                    u: DVec3::X * 4.,
                    v: DVec3::Y * 4.,
                    spacing: 1.,
                })
                .unwrap();
            let Element::Instance(target) = out.created[0] else { panic!() };
            app.editor
                .apply(Command::DrawLines {
                    lines: vec![[DVec3::new(-1., 1.5, 4.), DVec3::new(5., 1.5, 4.)]],
                    facing: None,
                })
                .unwrap();
            let sources = app.editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect::<Vec<_>>();
            app.editor.set_selection(sources);
            app.camera.set_standard_view(rysmo_math::StandardView::Top);
            app.zoom_extents();
            begin(&mut app).unwrap();
            let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
            pointer(&mut app, at, Pointer::Move);
            assert!(app.drape.as_ref().unwrap().preview.is_some());
            if context_change {
                app.editor.enter(target).unwrap();
            } else {
                app.editor
                    .apply(Command::DrawLines {
                        lines: vec![[DVec3::new(8., 8., 0.), DVec3::new(9., 8., 0.)]],
                        facing: None,
                    })
                    .unwrap();
            }
            let changed = app.editor.doc().clone();
            pointer(&mut app, at, Pointer::Down { clicks: 1 });
            assert!(app.drape.is_none());
            assert_eq!(app.editor.doc(), &changed);
        }
    }

    #[test]
    fn raw_triangles_in_editing_context_support_preview_commit_and_undo() {
        let mut app = App3d::new(Editor::new());
        let out = app
            .editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 4.,
                v: DVec3::Y * 4.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        app.editor.enter(instance).unwrap();
        app.editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::new(-1., 1.5, 4.), DVec3::new(5., 1.5, 4.)]],
                facing: None,
            })
            .unwrap();
        let sources = app
            .editor
            .mesh()
            .edges()
            .filter(|(_, edge)| edge.v.iter().all(|id| app.editor.mesh().pos(*id).z == 4.))
            .map(|(id, _)| Element::Edge(id))
            .collect::<Vec<_>>();
        app.editor.set_selection(sources);
        app.camera.set_standard_view(rysmo_math::StandardView::Top);
        app.zoom_extents();
        let before = app.editor.doc().clone();
        begin(&mut app).unwrap();
        let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
        pointer(&mut app, at, Pointer::Move);
        let (target, preview) = app.drape.as_ref().unwrap().preview.as_ref().unwrap();
        assert!(matches!(target, Element::Face(_)));
        let expected = preview.doc().clone();
        assert_eq!(app.editor.doc(), &before);
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert_eq!(app.editor.doc(), &expected);
        assert_eq!(app.editor.context(), &[instance]);
        assert!(app.editor.mesh().find_vertex(DVec3::new(1.5, 1.5, 0.)).is_some());
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }

    #[test]
    fn source_edge_or_wire_group_can_be_clicked_without_preselection() {
        for grouped in [false, true] {
            let mut app = App3d::new(Editor::new());
            app.editor
                .apply(Command::CreateTerrainGrid {
                    origin: DVec3::ZERO,
                    u: DVec3::X * 4.,
                    v: DVec3::Y * 4.,
                    spacing: 1.,
                })
                .unwrap();
            app.editor
                .apply(Command::DrawLines {
                    lines: vec![[DVec3::new(-1., 1.5, 4.), DVec3::new(5., 1.5, 4.)]],
                    facing: None,
                })
                .unwrap();
            let elements = app.editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect::<Vec<_>>();
            let sources = if grouped {
                app.editor.apply(Command::MakeGroup { elements: elements.clone() }).unwrap().created
            } else {
                elements
            };
            app.editor.clear_selection();
            app.camera.set_standard_view(rysmo_math::StandardView::Top);
            app.zoom_extents();
            let before = app.editor.doc().clone();
            crate::actions::run(&mut app, crate::actions::Action::Drape);
            assert!(app.drape.as_ref().unwrap().sources.is_empty());
            let at = app.local_view().project(DVec3::new(2., 1.5, 4.)).unwrap();
            crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
            assert_eq!(app.drape.as_ref().unwrap().sources, sources);
            assert_eq!(app.editor.doc(), &before);
            let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
            crate::input::tool_pointer(&mut app, at, Pointer::Move);
            assert!(app.drape.as_ref().unwrap().preview.is_some());
            crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
            assert!(app.drape.is_none());
            app.editor.undo();
            assert_eq!(app.editor.doc(), &before);
        }
    }

    #[test]
    fn hover_preview_click_commit_undo_and_escape_preserve_live_model() {
        let mut app = App3d::new(Editor::new());
        app.editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., spacing: 1. })
            .unwrap();
        app.editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::new(-1., 1.5, 4.), DVec3::new(5., 1.5, 4.)]],
                facing: None,
            })
            .unwrap();
        let sources = app.editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect::<Vec<_>>();
        app.editor.set_selection(sources.clone());
        app.camera.set_standard_view(rysmo_math::StandardView::Top);
        app.zoom_extents();
        let before = app.editor.doc().clone();
        crate::actions::run(&mut app, crate::actions::Action::Drape);
        assert!(app.drape.is_some());
        let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
        crate::input::tool_pointer(&mut app, at, Pointer::Move);
        let expected = app.drape.as_ref().unwrap().preview.as_ref().unwrap().1.doc().clone();
        assert_ne!(expected, before);
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.drape.is_none());
        assert_eq!(app.editor.doc(), &expected);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        app.editor.set_selection(sources);
        begin(&mut app).unwrap();
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.drape.is_none());
        assert_eq!(app.editor.doc(), &before);
    }
}
