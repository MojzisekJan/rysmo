//! Add Detail draft: insertion and movement commit together.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DVec2, DVec3, Plane, closest_line_ray, closest_on_segment};
use rysmo_mesh::{FaceId, Mesh, VertexId};
use rysmo_model::{Command, InstanceId};
use rysmo_tools::{Pointer, ToolKind};
pub struct Draft {
    pub point: Option<(FaceId, DVec3)>,
    pub offset: DVec3,
    pub preview: Option<(Mesh, VertexId)>,
    correction: Option<Command>,
    correction_added: bool,
    context: Vec<InstanceId>,
    revision: u64,
}
impl Draft {
    pub(crate) fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
    pub fn status(&self) -> &'static str {
        if self.correction.is_some() {
            return "Add Detail: enter offset to correct the last operation, or click another terrain point.";
        }
        if self.point.is_some() {
            "Add Detail: move the vertex and click, or enter offset. Shift moves freely; Escape cancels."
        } else {
            "Add Detail: click a terrain point, edge or face in its editing context."
        }
    }
    pub fn value(&self, normal: DVec3) -> f64 {
        let offset =
            if let Some(Command::AddTerrainDetail { offset, .. }) = &self.correction { *offset } else { self.offset };
        offset.length() * if offset.dot(normal) < 0. { -1. } else { 1. }
    }
}
pub fn begin(app: &mut App3d) {
    app.set_tool(ToolKind::Select);
    app.detail = Some(Draft {
        point: None,
        offset: DVec3::ZERO,
        preview: None,
        correction: None,
        correction_added: false,
        context: app.editor.context().to_vec(),
        revision: app.editor.revision(),
    });
}
fn update(app: &mut App3d) -> Result<(), String> {
    let d = app.detail.as_mut().ok_or("no Add Detail draft")?;
    let (face, point) = d.point.ok_or("choose a terrain point")?;
    let mut mesh = app.editor.mesh().clone();
    let (vertex, _) = mesh.add_terrain_detail(face, point, d.offset).map_err(|e| e.to_string())?;
    d.preview = Some((mesh, vertex));
    Ok(())
}
pub fn finish(app: &mut App3d) -> Result<(), String> {
    let d = app.detail.as_ref().ok_or("no Add Detail draft")?;
    if !d.valid(app) {
        app.detail = None;
        return Err("Add Detail cancelled because the model changed.".into());
    }
    let (face, point) = d.point.ok_or("choose a terrain point")?;
    let added = !app
        .editor
        .mesh()
        .face(face)
        .unwrap()
        .outer
        .iter()
        .any(|id| app.editor.mesh().pos(*id).distance(point) < rysmo_math::EPS);
    let command = Command::AddTerrainDetail { face, point, offset: d.offset };
    let out = app.editor.apply(command.clone()).map_err(|e| e.to_string())?;
    begin(app);
    if out.changed {
        app.detail.as_mut().unwrap().correction = Some(command);
        app.detail.as_mut().unwrap().correction_added = added;
    }
    Ok(())
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.detail.as_ref().is_some_and(|d| d.valid(app)) {
        app.detail = None;
        return;
    }
    if matches!(event, Pointer::Down { .. }) {
        app.detail.as_mut().unwrap().correction = None;
    }
    let view = app.local_view();
    let ray = view.ray(at);
    if let Some((_, point)) = app.detail.as_ref().unwrap().point {
        let normal = app.editor.active().axes.z();
        let offset = if app.modifiers().shift {
            let plane = Plane::new(view.view.transpose().transform_vector3(DVec3::Z).normalize(), point);
            plane.intersect_ray(&ray).map(|t| ray.at(t) - point)
        } else {
            closest_line_ray(point, normal, &ray).map(|(distance, _)| normal * distance)
        };
        if let Some(offset) = offset {
            app.detail.as_mut().unwrap().offset = offset;
            app.message = update(app).err();
        }
        if matches!(event, Pointer::Down { .. }) {
            app.message = finish(app).err();
        }
    } else if matches!(event, Pointer::Down { .. }) {
        let hit = app.editor.mesh().pick_face(&ray).and_then(|(face, t)| {
            let mesh = app.editor.mesh();
            let data = mesh.face(face)?;
            if data.outer.len() != 3 || !data.holes.is_empty() {
                return None;
            }
            let mut point = ray.at(t);
            let nearest = |points: Vec<DVec3>| {
                points
                    .into_iter()
                    .filter_map(|p| view.project(p).map(|px| (px.distance(at), p)))
                    .filter(|(distance, _)| *distance <= 6. * app.pick_scale())
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, p)| p)
            };
            let vertex = nearest(data.outer.iter().map(|id| mesh.pos(*id)).collect());
            let edge = || {
                nearest(
                    data.loop_edges()
                        .into_iter()
                        .map(|(a, b)| closest_on_segment(mesh.pos(a), mesh.pos(b), point).0)
                        .collect(),
                )
            };
            if let Some(snap) = vertex.or_else(edge) {
                point = snap;
            }
            Some((face, point))
        });
        if let Some(hit) = hit {
            app.detail.as_mut().unwrap().point = Some(hit);
            app.message = update(app).err();
        } else {
            app.message = Some("Edit the terrain group and click a triangle.".into());
        }
    }
}
pub fn value(app: &mut App3d, text: &str) -> Result<(), String> {
    if !app.detail.as_ref().is_some_and(|d| d.valid(app)) {
        app.detail = None;
        return Err("Add Detail cancelled because the model changed.".into());
    }
    let amount = rysmo_model::parse_length(text, app.editor.doc().units).map_err(|e| e.to_string())?;
    let normal = app.editor.active().axes.z();
    let free = app.modifiers().shift;
    if let Some(mut command) = app.detail.as_ref().unwrap().correction.clone() {
        let Command::AddTerrainDetail { offset, .. } = &mut command else { unreachable!() };
        let direction = if free { offset.try_normalize().unwrap_or(normal) } else { normal };
        *offset = direction * amount;
        let moved = matches!(&command,Command::AddTerrainDetail {offset,..} if offset.length()>=rysmo_math::EPS);
        let added = app.detail.as_ref().unwrap().correction_added;
        app.editor.amend_last(command.clone()).map_err(|e| e.to_string())?;
        begin(app);
        let d = app.detail.as_mut().unwrap();
        d.correction = (added || moved).then_some(command);
        d.correction_added = added;
        return Ok(());
    }
    let d = app.detail.as_mut().unwrap();
    if d.point.is_none() {
        return Err("Choose a terrain point first.".into());
    }
    let direction = if free { d.offset.try_normalize().unwrap_or(normal) } else { normal };
    d.offset = direction * amount;
    update(app)?;
    finish(app)
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(d) = &app.detail else { return };
    if !d.valid(app) {
        return;
    }
    let Some((mesh, vertex)) = &d.preview else { return };
    let view = app.local_view();
    for (id, edge) in mesh.edges() {
        if app.editor.mesh().edge(id).is_some_and(|old| {
            old.v == edge.v && old.v.iter().all(|v| app.editor.mesh().pos(*v).distance(mesh.pos(*v)) < 1e-8)
        }) {
            continue;
        }
        if let (Some(a), Some(b)) = (view.project(mesh.pos(edge.v[0])), view.project(mesh.pos(edge.v[1]))) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(1.5, egui::Color32::MAGENTA),
            );
        }
    }
    if let Some(p) = view.project(mesh.pos(*vertex)) {
        painter.circle_filled(egui::pos2(p.x as f32, p.y as f32), 5., egui::Color32::MAGENTA);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::{Editor, Element};
    fn app() -> App3d {
        let mut app = App3d::new(Editor::new());
        let out = app
            .editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., spacing: 2. })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        app.editor.enter(id).unwrap();
        app
    }
    #[test]
    fn zero_correction_of_existing_vertex_restores_original_document() {
        let mut app = app();
        let before = app.editor.doc().clone();
        begin(&mut app);
        let at = app.view().project(DVec3::ZERO).unwrap();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        value(&mut app, "1m").unwrap();
        value(&mut app, "0m").unwrap();
        assert_eq!(app.editor.doc(), &before);
        assert!(app.detail.as_ref().unwrap().correction.is_none());
    }
    #[test]
    fn corrections_replace_one_detail_operation_without_extra_vertices() {
        let mut app = app();
        let before = app.editor.doc().clone();
        begin(&mut app);
        let at = app.view().project(DVec3::new(1.5, 0.5, 0.)).unwrap();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        let point = app.detail.as_ref().unwrap().point.unwrap().1;
        let count = app.editor.mesh().vertices().count();
        value(&mut app, "50cm").unwrap();
        value(&mut app, "1m").unwrap();
        assert_eq!(app.editor.mesh().vertices().count(), count + 1);
        assert!(app.editor.mesh().find_vertex(point + DVec3::Z).is_some());
        let corrected = app.editor.doc().clone();
        assert!(value(&mut app, "bad").is_err());
        assert_eq!(app.editor.doc(), &corrected);
        value(&mut app, "0m").unwrap();
        assert_eq!(app.editor.mesh().vertices().count(), count + 1);
        assert!(app.editor.mesh().find_vertex(point).is_some());
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn model_change_cancels_before_commit() {
        let mut app = app();
        begin(&mut app);
        let at = app.view().project(DVec3::new(1.5, 0.5, 0.)).unwrap();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        app.editor.apply(Command::AddGuidePoint { point: DVec3::ZERO }).unwrap();
        let before = app.editor.doc().clone();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.detail.is_none());
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn preview_and_typed_offset_commit_in_one_step() {
        let mut app = app();
        let before = app.editor.doc().clone();
        begin(&mut app);
        let point = DVec3::new(1.5, 0.5, 0.);
        let at = app.view().project(point).unwrap();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.detail.as_ref().unwrap().preview.is_some());
        assert_eq!(app.editor.doc(), &before);
        let chosen = app.detail.as_ref().unwrap().point.unwrap().1;
        value(&mut app, "50cm").unwrap();
        assert!(app.editor.mesh().find_vertex(chosen + DVec3::Z * 0.5).is_some());
        assert_eq!(app.editor.undo_label(), Some("Add Detail"));
        assert!(app.detail.as_ref().unwrap().point.is_none());
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn mouse_shift_and_cancellation_keep_draft_out_of_history() {
        let mut app = app();
        let before = app.editor.doc().clone();
        begin(&mut app);
        let at = app.view().project(DVec3::new(1.5, 0.5, 0.)).unwrap();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        let point = app.detail.as_ref().unwrap().point.unwrap().1;
        app.held.shift = true;
        let at = app.view().project(point + DVec3::new(0.25, 0., 0.25)).unwrap();
        pointer(&mut app, at, Pointer::Move);
        assert!(app.detail.as_ref().unwrap().offset.cross(DVec3::Z).length() > 0.01);
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.detail.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        assert!(value(&mut app, "1m").is_err());
        app.set_tool(ToolKind::Line);
        assert!(app.detail.is_none());
    }
}
