//! Stamp stages and preview. The live editor is mutated only by commit.
use rysmo_math::DMat4;
use rysmo_model::{Command, Document, Editor, Element, InstanceId};
#[derive(Clone)]
pub enum Stage {
    Source,
    Terrain,
    Elevation { target: Element, height: f64, preview: Box<Document> },
}
pub struct Draft {
    pub sources: Vec<Element>,
    pub offset: f64,
    pub source_height: f64,
    pub stage: Stage,
    pub outlines: Vec<Vec<rysmo_math::DVec2>>,
    pub transitions: Vec<Vec<rysmo_math::DVec2>>,
    anchor: Option<rysmo_math::DVec3>,
    pointer_at: Option<rysmo_math::DVec2>,
    context: Vec<InstanceId>,
    revision: u64,
    correction: Option<Command>,
}
impl Draft {
    pub fn begin(editor: &Editor, sources: Vec<Element>, offset: f64) -> Result<Self, String> {
        if !offset.is_finite() || offset <= 0. {
            return Err("Stamp offset must be positive.".into());
        }
        let axes = editor.active().axes;
        let projection = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
        let (outlines, source_height) = if sources.is_empty() {
            (Vec::new(), 0.)
        } else {
            rysmo_model::terrain_footprint::source_footprint(
                editor.doc(),
                editor.active_definition(),
                &sources,
                projection,
            )?
        };
        let transitions = if outlines.is_empty() {
            Vec::new()
        } else {
            rysmo_model::terrain_surface::TerrainSurface::transition_rings(&outlines, offset)?
        };
        let stage = if sources.is_empty() { Stage::Source } else { Stage::Terrain };
        Ok(Self {
            source_height,
            outlines,
            transitions,
            sources,
            offset,
            stage,
            anchor: None,
            pointer_at: None,
            context: editor.context().to_vec(),
            revision: editor.revision(),
            correction: None,
        })
    }
    pub fn valid(&self, editor: &Editor) -> bool {
        self.context == editor.context() && self.revision == editor.revision()
    }
    fn command(&self, target: Element, height: f64, offset: f64) -> Command {
        match target {
            Element::Instance(target) => {
                Command::StampTerrainInstance { sources: self.sources.clone(), target, offset, height }
            }
            Element::Face(target) => Command::StampOnFace { sources: self.sources.clone(), target, offset, height },
            _ => unreachable!("Stamp targets are faces or instances"),
        }
    }
    fn preview(&self, editor: &Editor, target: Element, height: f64, offset: f64) -> Result<Document, String> {
        if !self.valid(editor) {
            return Err("Stamp cancelled because the model changed.".into());
        }
        let mut candidate = editor.clone();
        candidate.apply(self.command(target, height, offset)).map_err(|e| e.to_string())?;
        Ok(candidate.doc().clone())
    }
    pub fn choose_element(&mut self, editor: &Editor, target: Element, height: f64) -> Result<(), String> {
        if !matches!(target, Element::Face(_) | Element::Instance(_)) {
            return Err("Choose a terrain face or instance.".into());
        }
        let preview = self.preview(editor, target, height, self.offset)?;
        self.stage = Stage::Elevation { target, height, preview: Box::new(preview) };
        Ok(())
    }
    pub fn set_offset(&mut self, editor: &Editor, offset: f64) -> Result<(), String> {
        if !self.valid(editor) {
            return Err("Stamp cancelled because the model changed.".into());
        }
        if !offset.is_finite() || offset <= 0. {
            return Err("Stamp offset must be positive.".into());
        }
        let transitions = if self.outlines.is_empty() {
            Vec::new()
        } else {
            rysmo_model::terrain_surface::TerrainSurface::transition_rings(&self.outlines, offset)?
        };
        if let Stage::Elevation { target, height, .. } = self.stage {
            let preview = self.preview(editor, target, height, offset)?;
            self.stage = Stage::Elevation { target, height, preview: Box::new(preview) };
        }
        self.offset = offset;
        self.transitions = transitions;
        Ok(())
    }
    pub fn set_height(&mut self, editor: &Editor, height: f64) -> Result<(), String> {
        let Stage::Elevation { target, .. } = self.stage else {
            return Err("Choose the terrain first.".into());
        };
        self.choose_element(editor, target, height)
    }
    pub fn commit(&self, editor: &mut Editor) -> Result<(), String> {
        if self.correction.is_some() {
            return Err("Stamp has already been committed.".into());
        }
        if !self.valid(editor) {
            return Err("Stamp cancelled because the model changed.".into());
        }
        let Stage::Elevation { target, height, .. } = self.stage else {
            return Err("Choose the terrain first.".into());
        };
        editor.apply(self.command(target, height, self.offset)).map_err(|e| e.to_string())?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn preview_is_atomic_and_commit_is_one_history_operation() {
        let mut editor = Editor::new();
        let output = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 6., v: DVec3::Y * 6., spacing: 1. })
            .unwrap();
        let Element::Instance(target) = output.created[0] else { panic!() };
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 5.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let before = editor.doc().clone();
        let revision = editor.revision();
        let mut draft = Draft::begin(&editor, vec![Element::Face(face)], 1.).unwrap();
        assert_eq!(draft.source_height, 5.);
        assert!((rysmo_math::polygon_area_2d(&draft.outlines[0]) - 4.).abs() < 1e-9);
        assert!((rysmo_math::polygon_area_2d(&draft.transitions[0]) - 16.).abs() < 1e-9);
        assert!(draft.set_height(&editor, 2.).is_err());
        draft.choose_element(&editor, Element::Instance(target), 1.).unwrap();
        draft.set_height(&editor, 2.).unwrap();
        draft.set_offset(&editor, 0.5).unwrap();
        assert!((rysmo_math::polygon_area_2d(&draft.transitions[0]) - 9.).abs() < 1e-9);
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        let Stage::Elevation { preview, height, .. } = &draft.stage else { panic!() };
        assert_eq!(*height, 2.);
        let expected = preview.as_ref().clone();
        let outlines_before = draft.transitions.clone();
        assert!(draft.set_offset(&editor, -1.).is_err());
        assert_eq!(draft.transitions, outlines_before);
        draft.commit(&mut editor).unwrap();
        assert_eq!(editor.doc(), &expected);
        assert!(!draft.valid(&editor));
        assert!(draft.commit(&mut editor).is_err());
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }
}

impl Draft {
    pub fn status(&self) -> &'static str {
        if self.correction.is_some() {
            return "Stamp: enter elevation or offset with suffix o to correct the last operation. Click or Escape ends correction.";
        }
        match self.stage {
            Stage::Source => "Stamp: click a source face, group or component. Escape cancels.",
            Stage::Terrain => {
                "Stamp: click a terrain triangle or closed terrain group. Enter transition offset; Escape cancels."
            }
            Stage::Elevation { .. } => {
                "Stamp: move vertically or enter elevation; suffix o edits offset. Then click to commit. Escape cancels."
            }
        }
    }
}
pub fn begin(app: &mut crate::state::App3d) -> Result<(), String> {
    let sources = app.editor.selection().iter().copied().collect();
    let draft = Draft::begin(&app.editor, sources, 1.)?;
    app.set_tool(rysmo_tools::ToolKind::Select);
    app.stamp = Some(draft);
    Ok(())
}
pub fn pointer(app: &mut crate::state::App3d, at: rysmo_math::DVec2, event: rysmo_tools::Pointer) {
    if !app.stamp.as_ref().is_some_and(|d| d.valid(&app.editor)) {
        app.stamp = None;
        return;
    }
    if app.stamp.as_ref().unwrap().correction.is_some() {
        if matches!(event, rysmo_tools::Pointer::Down { .. }) {
            app.stamp = None;
        }
        return;
    }
    if matches!(event, rysmo_tools::Pointer::Move) {
        let draft = app.stamp.as_ref().unwrap();
        if matches!(draft.stage, Stage::Elevation { .. }) && draft.pointer_at != Some(at) {
            if let Some(anchor) = draft.anchor {
                let axes = app.editor.active().axes;
                let ray = app.local_view().ray(at);
                if let Some((distance, _)) = rysmo_math::closest_line_ray(anchor, axes.z(), &ray) {
                    let height = (anchor - axes.origin).dot(axes.z()) + distance;
                    app.message = app.stamp.as_mut().unwrap().set_height(&app.editor, height).err();
                }
            }
            app.stamp.as_mut().unwrap().pointer_at = Some(at);
        }
        return;
    }
    if !matches!(event, rysmo_tools::Pointer::Down { .. }) {
        return;
    }
    if matches!(app.stamp.as_ref().unwrap().stage, Stage::Elevation { .. }) {
        let mut draft = app.stamp.take().unwrap();
        if let Err(error) = draft.commit(&mut app.editor) {
            app.message = Some(error);
        } else {
            if let Stage::Elevation { target, height, .. } = draft.stage {
                draft.correction = Some(draft.command(target, height, draft.offset));
            }
            draft.revision = app.editor.revision();
        }
        app.stamp = Some(draft);
        return;
    }
    let hit = visible_face_pick(app, at);
    if matches!(app.stamp.as_ref().unwrap().stage, Stage::Source) {
        if let Some((source, _)) = hit {
            let offset = app.stamp.as_ref().unwrap().offset;
            match Draft::begin(&app.editor, vec![source], offset) {
                Ok(mut draft) => {
                    app.editor.set_selection([source]);
                    draft.revision = app.editor.revision();
                    app.stamp = Some(draft);
                    app.message = None;
                }
                Err(error) => app.message = Some(error),
            }
        } else {
            app.message = Some("Click a visible source face or closed group.".into());
        }
        return;
    }
    if let Some((target @ (Element::Instance(_) | Element::Face(_)), point)) = hit {
        let axes = app.editor.active().axes;
        let height = (point - axes.origin).dot(axes.z());
        app.message = app.stamp.as_mut().unwrap().choose_element(&app.editor, target, height).err();
        if app.message.is_none() {
            let draft = app.stamp.as_mut().unwrap();
            draft.anchor = Some(point);
            draft.pointer_at = Some(at);
        }
    } else {
        app.message = Some("Click a visible terrain triangle or closed terrain group.".into());
    }
}
pub fn value(app: &mut crate::state::App3d, text: &str) -> Result<(), String> {
    let text = text.trim();
    let offset_input = text.ends_with(['o', 'O']);
    let number = if offset_input { text[..text.len() - 1].trim() } else { text };
    let value = rysmo_model::parse_length(number, app.editor.doc().units).map_err(|e| e.to_string())?;
    let draft = app.stamp.as_mut().ok_or("No Stamp draft.")?;
    if !draft.valid(&app.editor) {
        return Err("Stamp cancelled because the model changed.".into());
    }
    if let Some(mut command) = draft.correction.clone() {
        let (target, height, offset) = match &mut command {
            Command::StampTerrainInstance { target, height, offset, .. } => {
                (Element::Instance(*target), height, offset)
            }
            Command::StampOnFace { target, height, offset, .. } => (Element::Face(*target), height, offset),
            _ => unreachable!(),
        };
        let transitions = if offset_input {
            let transitions = rysmo_model::terrain_surface::TerrainSurface::transition_rings(&draft.outlines, value)?;
            *offset = value;
            Some(transitions)
        } else {
            *height = value;
            None
        };
        let (target, height, offset) = (target, *height, *offset);
        app.editor.amend_last(command.clone()).map_err(|e| e.to_string())?;
        draft.stage = Stage::Elevation { target, height, preview: Box::new(app.editor.doc().clone()) };
        draft.offset = offset;
        if let Some(transitions) = transitions {
            draft.transitions = transitions;
        }
        draft.correction = Some(command);
        draft.revision = app.editor.revision();
        return Ok(());
    }
    if offset_input {
        return draft.set_offset(&app.editor, value);
    }
    match draft.stage {
        Stage::Source | Stage::Terrain => draft.set_offset(&app.editor, value),
        Stage::Elevation { .. } => draft.set_height(&app.editor, value),
    }
}

pub fn draw(painter: &bevy_egui::egui::Painter, app: &crate::state::App3d) {
    let Some(draft) = &app.stamp else { return };
    if !draft.valid(&app.editor) {
        return;
    }
    let axes = app.editor.active().axes;
    let height = match draft.stage {
        Stage::Source | Stage::Terrain => draft.source_height,
        Stage::Elevation { height, .. } => height,
    };
    for (rings, colour) in
        [(&draft.outlines, bevy_egui::egui::Color32::MAGENTA), (&draft.transitions, bevy_egui::egui::Color32::YELLOW)]
    {
        for ring in rings {
            for i in 0..ring.len() {
                let points = [ring[i], ring[(i + 1) % ring.len()]]
                    .map(|p| axes.origin + axes.basis().transform_vector3(rysmo_math::DVec3::new(p.x, p.y, height)));
                if let (Some(a), Some(b)) = (app.local_view().project(points[0]), app.local_view().project(points[1])) {
                    painter.line_segment(
                        [bevy_egui::egui::pos2(a.x as f32, a.y as f32), bevy_egui::egui::pos2(b.x as f32, b.y as f32)],
                        bevy_egui::egui::Stroke::new(2., colour),
                    );
                }
            }
        }
    }
    let Stage::Elevation { target, preview, .. } = &draft.stage else { return };
    let Some(parent) = preview.def(app.editor.active_definition()) else { return };
    let (definition, transform, original) = match target {
        Element::Instance(target) => {
            let Some(instance) = parent.instances.get(target) else { return };
            let Some(definition) = preview.def(Some(instance.definition)) else { return };
            (definition, instance.transform, app.editor.doc().def(Some(instance.definition)))
        }
        Element::Face(_) => (parent, DMat4::IDENTITY, app.editor.doc().def(app.editor.active_definition())),
        _ => return,
    };
    if let (Some(anchor), Stage::Elevation { height, .. }) = (draft.anchor, &draft.stage) {
        let axes = app.editor.active().axes;
        let endpoint = anchor + axes.z() * (*height - (anchor - axes.origin).dot(axes.z()));
        if let (Some(a), Some(b)) = (app.local_view().project(anchor), app.local_view().project(endpoint)) {
            painter.line_segment(
                [bevy_egui::egui::pos2(a.x as f32, a.y as f32), bevy_egui::egui::pos2(b.x as f32, b.y as f32)],
                bevy_egui::egui::Stroke::new(2., bevy_egui::egui::Color32::MAGENTA),
            );
        }
    }
    let view = app.local_view().local(transform);
    for (id, edge) in definition.mesh.edges() {
        if edge.hidden {
            continue;
        }
        if original.is_some_and(|original| {
            original.mesh.edge(id).is_some_and(|old| {
                old.v == edge.v
                    && edge
                        .v
                        .iter()
                        .all(|vertex| original.mesh.pos(*vertex).distance(definition.mesh.pos(*vertex)) < 1e-8)
            })
        }) {
            continue;
        }
        if let (Some(a), Some(b)) =
            (view.project(definition.mesh.pos(edge.v[0])), view.project(definition.mesh.pos(edge.v[1])))
        {
            painter.line_segment(
                [bevy_egui::egui::pos2(a.x as f32, a.y as f32), bevy_egui::egui::pos2(b.x as f32, b.y as f32)],
                bevy_egui::egui::Stroke::new(1.5, bevy_egui::egui::Color32::MAGENTA),
            );
        }
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn raw_terrain_pointer_preview_commit_and_correction() {
        let mut document = rysmo_model::Document::default();
        document.with_mesh(None, |mesh| *mesh = rysmo_model::terrain::grid(6., 6., 1.).unwrap()).unwrap();
        let mut editor = Editor::new();
        editor.replace_document(document);
        let mut app = crate::state::App3d::new(editor);
        app.editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 5.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let source = app.editor.mesh().faces().find(|(_, face)| face.outer.len() == 4).unwrap().0;
        app.editor.set_selection([Element::Face(source)]);
        app.camera.set_standard_view(rysmo_math::StandardView::Top);
        app.zoom_extents();
        let before = app.editor.doc().clone();
        begin(&mut app).unwrap();
        let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
        pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Elevation { target: Element::Face(_), .. }));
        value(&mut app, "2m").unwrap();
        value(&mut app, "0.5mo").unwrap();
        assert_eq!(app.editor.doc(), &before);
        let Stage::Elevation { preview, .. } = &app.stamp.as_ref().unwrap().stage else { panic!() };
        let expected = preview.as_ref().clone();
        pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert_eq!(app.editor.doc(), &expected);
        value(&mut app, "2.5m").unwrap();
        value(&mut app, "0.75mo").unwrap();
        assert!(app.editor.mesh().find_vertex(DVec3::new(3., 3., 2.5)).is_some());
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }

    #[test]
    fn unselected_source_face_or_group_can_be_clicked_before_terrain() {
        for grouped in [false, true] {
            let mut app = crate::state::App3d::new(Editor::new());
            app.editor
                .apply(Command::CreateTerrainGrid {
                    origin: DVec3::ZERO,
                    u: DVec3::X * 6.,
                    v: DVec3::Y * 6.,
                    spacing: 1.,
                })
                .unwrap();
            app.editor
                .apply(Command::DrawRectangle {
                    corner: DVec3::new(2., 2., 5.),
                    u: DVec3::X * 2.,
                    v: DVec3::Y * 2.,
                    facing: Some(DVec3::Z),
                })
                .unwrap();
            let face = app.editor.mesh().faces().next().unwrap().0;
            let source = if grouped {
                app.editor.apply(Command::MakeGroup { elements: vec![Element::Face(face)] }).unwrap().created[0]
            } else {
                Element::Face(face)
            };
            app.editor.clear_selection();
            app.camera.set_standard_view(rysmo_math::StandardView::Top);
            app.zoom_extents();
            let before = app.editor.doc().clone();
            begin(&mut app).unwrap();
            assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Source));
            value(&mut app, "0.5m").unwrap();
            let at = app.local_view().project(DVec3::new(3., 3., 5.)).unwrap();
            pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
            assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Terrain));
            assert_eq!(app.stamp.as_ref().unwrap().sources, vec![source]);
            assert_eq!(app.stamp.as_ref().unwrap().source_height, 5.);
            assert_eq!(app.stamp.as_ref().unwrap().offset, 0.5);
            let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
            pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
            assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Elevation { .. }));
            value(&mut app, "2m").unwrap();
            pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
            app.editor.undo();
            assert_eq!(app.editor.doc(), &before);
        }
    }

    #[test]
    fn terrain_pick_respects_foreground_faces_and_hidden_tags() {
        let mut app = crate::state::App3d::new(Editor::new());
        let out = app
            .editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 6., v: DVec3::Y * 6., spacing: 1. })
            .unwrap();
        let Element::Instance(target) = out.created[0] else { panic!() };
        app.editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 5.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.set_selection([Element::Face(face)]);
        app.camera.set_standard_view(rysmo_math::StandardView::Top);
        app.zoom_extents();
        begin(&mut app).unwrap();
        let at = app.local_view().project(DVec3::new(3., 3., 0.)).unwrap();
        pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Terrain));
        let tag = app.editor.apply(Command::AddTag { name: "Terrain".into() }).unwrap().tag.unwrap();
        app.editor.apply(Command::AssignTag { instances: vec![target], tag: Some(tag) }).unwrap();
        app.editor.apply(Command::SetTag { tag, name: None, visible: Some(false) }).unwrap();
        begin(&mut app).unwrap();
        let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
        pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Terrain));
        app.editor.apply(Command::SetTag { tag, name: None, visible: Some(true) }).unwrap();
        begin(&mut app).unwrap();
        pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Elevation { .. }));
    }

    #[test]
    fn action_pointer_exact_height_and_escape_drive_stamp() {
        let mut app = crate::state::App3d::new(Editor::new());
        app.editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 6., v: DVec3::Y * 6., spacing: 1. })
            .unwrap();
        app.editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 5.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.set_selection([Element::Face(face)]);
        app.zoom_extents();
        let before = app.editor.doc().clone();
        crate::actions::run(&mut app, crate::actions::Action::Stamp);
        assert!(app.stamp.is_some());
        let at = app.local_view().project(DVec3::new(1., 1., 0.)).unwrap();
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(matches!(app.stamp.as_ref().unwrap().stage, Stage::Elevation { .. }));
        let anchor = app.stamp.as_ref().unwrap().anchor.unwrap();
        let moved = app.local_view().project(anchor + app.editor.active().axes.z() * 1.25).unwrap();
        crate::input::tool_pointer(&mut app, moved, rysmo_tools::Pointer::Move);
        let Stage::Elevation { height, .. } = app.stamp.as_ref().unwrap().stage else { panic!() };
        assert!((height - 1.25).abs() < 1e-8);
        value(&mut app, "2m").unwrap();
        crate::input::tool_pointer(&mut app, moved, rysmo_tools::Pointer::Move);
        let Stage::Elevation { height, .. } = app.stamp.as_ref().unwrap().stage else { panic!() };
        assert_eq!(height, 2.);
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(app.stamp.as_ref().unwrap().correction.is_some());
        let vertices = app.editor.doc().definitions.values().map(|d| d.mesh.vertices().count()).sum::<usize>();
        value(&mut app, "3m").unwrap();
        value(&mut app, "1.5m").unwrap();
        assert_eq!(vertices, app.editor.doc().definitions.values().map(|d| d.mesh.vertices().count()).sum::<usize>());
        value(&mut app, "0.5mo").unwrap();
        assert_eq!(app.stamp.as_ref().unwrap().offset, 0.5);
        assert!((rysmo_math::polygon_area_2d(&app.stamp.as_ref().unwrap().transitions[0]) - 9.).abs() < 1e-9);
        let corrected = app.editor.doc().clone();
        let outline_cache = app.stamp.as_ref().unwrap().transitions.clone();
        assert!(value(&mut app, "0mo").is_err());
        assert_eq!(app.stamp.as_ref().unwrap().transitions, outline_cache);
        assert_eq!(app.editor.doc(), &corrected);
        let instance = corrected.root.instances.values().next().unwrap();
        let mesh = &corrected.definitions[&instance.definition].mesh;
        assert!(
            (rysmo_model::terrain_surface::TerrainSurface::new(mesh)
                .unwrap()
                .sample(rysmo_math::DVec2::new(3., 3.))
                .unwrap()
                .0
                - 1.5)
                .abs()
                < 1e-8
        );
        assert!(app.stamp.as_ref().unwrap().commit(&mut app.editor).is_err());
        assert!(value(&mut app, "NaN").is_err());
        assert_eq!(app.editor.doc(), &corrected);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        app.editor.set_selection([Element::Face(face)]);
        begin(&mut app).unwrap();
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.stamp.is_none());
    }
}

pub(crate) fn visible_face_pick(
    app: &crate::state::App3d,
    at: rysmo_math::DVec2,
) -> Option<(Element, rysmo_math::DVec3)> {
    let view = app.view();
    let ray = view.ray(at);
    app.editor
        .doc()
        .occurrences(app.editor.context())
        .into_iter()
        .filter_map(|occurrence| {
            let definition = app.editor.doc().def(occurrence.definition)?;
            let local_ray = view.local(occurrence.transform).ray(at);
            let (face, t) = definition.mesh.pick_face(&local_ray)?;
            let point = occurrence.transform.transform_point3(local_ray.at(t));
            let distance = (point - ray.origin).dot(ray.dir);
            (distance >= 0.).then_some((
                if occurrence.in_context {
                    Some(occurrence.top.map(Element::Instance).unwrap_or(Element::Face(face)))
                } else {
                    None
                },
                distance,
                point,
            ))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .and_then(|(element, _, point)| {
            element.map(|element| (element, app.editor.context_transform().inverse().transform_point3(point)))
        })
}
