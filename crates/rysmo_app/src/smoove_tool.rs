//! Staged Smoove with a radius brush and deformation outside history.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DVec2, DVec3, Plane, closest_line_ray};
use rysmo_mesh::{Mesh, VertexId};
use rysmo_model::{Command, InstanceId};
use rysmo_tools::{Pointer, ToolKind};
use std::collections::{BTreeMap, BTreeSet};
pub struct Draft {
    pub radius: f64,
    normal: DVec3,
    pub origin: Option<DVec3>,
    pub offset: DVec3,
    pub hover: Option<DVec3>,
    vertices: Vec<VertexId>,
    centres: Vec<DVec3>,
    pub preview: Option<Mesh>,
    affected: Vec<(VertexId, bool)>,
    cached_radius: Option<f64>,
    preselected: bool,
    last_offset: Option<DVec3>,
    correction: Option<Command>,
    context: Vec<InstanceId>,
    revision: u64,
}
impl Draft {
    pub(crate) fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
    pub fn measurement(&self) -> (&'static str, f64) {
        if let Some(Command::Smoove { offset, normal, .. }) = &self.correction {
            return ("Offset", offset.length() * if offset.dot(*normal) < 0. { -1. } else { 1. });
        }
        if self.origin.is_some() {
            ("Offset", self.offset.length() * if self.offset.dot(self.normal) < 0. { -1. } else { 1. })
        } else {
            ("Radius", self.radius)
        }
    }
    pub fn status(&self) -> &'static str {
        if self.correction.is_some() {
            return "Smoove: enter offset or radius r to correct the last operation; click terrain to start another.";
        }
        if self.origin.is_some() {
            "Smoove: move and click, or enter an offset. Use r for radius. Escape cancels."
        } else {
            "Smoove: enter radius, then click terrain in its editing context."
        }
    }
}
fn eligible(mesh: &Mesh, seeds: &BTreeSet<VertexId>) -> Vec<VertexId> {
    let mut neighbours: BTreeMap<VertexId, Vec<VertexId>> = BTreeMap::new();
    for (_, face) in mesh.faces().filter(|(_, f)| f.outer.len() == 3 && f.holes.is_empty()) {
        for vertex in &face.outer {
            neighbours.entry(*vertex).or_default().extend(face.outer.iter().copied());
        }
    }
    let mut seen = seeds.clone();
    let mut queue = seeds.iter().copied().collect::<Vec<_>>();
    while let Some(vertex) = queue.pop() {
        if let Some(next) = neighbours.get(&vertex) {
            for next in next {
                if seen.insert(*next) {
                    queue.push(*next);
                }
            }
        }
    }
    seen.into_iter().collect()
}
pub fn begin(app: &mut App3d) {
    let seeds = app.editor.mesh().vertices_of(&app.editor.selected());
    app.set_tool(ToolKind::Move);
    let centres = seeds.iter().map(|id| app.editor.mesh().pos(*id)).collect::<Vec<_>>();
    let origin = (!centres.is_empty()).then(|| centres.iter().copied().sum::<DVec3>() / centres.len() as f64);
    app.smoove = Some(Draft {
        radius: app.smoove_radius,
        normal: app.editor.active().axes.z(),
        origin,
        offset: DVec3::ZERO,
        hover: None,
        vertices: eligible(app.editor.mesh(), &seeds),
        centres,
        preview: None,
        affected: Vec::new(),
        cached_radius: None,
        preselected: origin.is_some(),
        last_offset: None,
        correction: None,
        context: app.editor.context().to_vec(),
        revision: app.editor.revision(),
    });
    if app.smoove.as_ref().is_some_and(|d| d.origin.is_some()) {
        app.message = update(app).err();
    }
}
fn update(app: &mut App3d) -> Result<(), String> {
    let draft = app.smoove.as_mut().ok_or("no Smoove draft")?;
    let mut mesh = app.editor.mesh().clone();
    rysmo_model::terrain::smoove(
        &mut mesh,
        &draft.vertices,
        &draft.centres,
        draft.radius,
        app.editor.active().axes.z(),
        draft.offset,
    )?;
    if draft.cached_radius != Some(draft.radius) {
        let influence = rysmo_model::terrain::SmooveInfluence::new(&draft.centres, draft.normal)?;
        draft.affected = draft
            .vertices
            .iter()
            .filter_map(|id| {
                let point = app.editor.mesh().pos(*id);
                (influence.weight(point, draft.radius) > 1e-12).then_some((*id, influence.is_seed(point)))
            })
            .collect();
        draft.cached_radius = Some(draft.radius);
    }
    draft.preview = Some(mesh);
    Ok(())
}
pub fn finish(app: &mut App3d) -> Result<(), String> {
    let d = app.smoove.as_ref().ok_or("no Smoove draft")?;
    if !d.valid(app) {
        app.smoove = None;
        return Err("Smoove cancelled because the model changed.".into());
    }
    let command = Command::Smoove {
        vertices: d.vertices.clone(),
        centres: d.centres.clone(),
        radius: d.radius,
        normal: d.normal,
        offset: d.offset,
    };
    let out = app.editor.apply(command.clone()).map_err(|e| e.to_string())?;
    let correction = if out.changed { Some(command) } else { d.correction.clone() };
    let radius = d.radius;
    let last_offset = if out.changed { Some(d.offset) } else { d.last_offset };
    app.smoove_radius = radius;
    begin(app);
    let draft = app.smoove.as_mut().unwrap();
    draft.radius = radius;
    draft.last_offset = last_offset;
    draft.correction = correction;
    Ok(())
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.smoove.as_ref().is_some_and(|d| d.valid(app)) {
        app.smoove = None;
        return;
    }
    if matches!(event, Pointer::Down { .. }) {
        app.smoove.as_mut().unwrap().correction = None;
    }
    if matches!(event, Pointer::Down { clicks: 2.. })
        && app.smoove.as_ref().unwrap().origin.is_some()
        && let Some(offset) = app.smoove.as_ref().unwrap().last_offset
    {
        app.smoove.as_mut().unwrap().offset = offset;
        app.message = update(app).and_then(|_| finish(app)).err();
        return;
    }
    let ray = app.local_view().ray(at);
    let normal = app.editor.active().axes.z();
    let origin = app.smoove.as_ref().unwrap().origin;
    if let Some(origin) = origin {
        let free = app.modifiers().shift && !app.smoove.as_ref().unwrap().preselected;
        let offset = if free {
            let view = app.local_view();
            let plane = Plane::new(view.view.transpose().transform_vector3(DVec3::Z).normalize(), origin);
            plane.intersect_ray(&ray).map(|t| ray.at(t) - origin)
        } else {
            closest_line_ray(origin, normal, &ray).map(|(distance, _)| normal * distance)
        };
        if let Some(offset) = offset {
            app.smoove.as_mut().unwrap().offset = offset;
            app.message = update(app).err();
        }
        if matches!(event, Pointer::Down { .. }) {
            app.message = finish(app).err();
        }
    } else {
        let hit = app.editor.mesh().pick_face(&ray).and_then(|(face, distance)| {
            let face = app.editor.mesh().face(face)?;
            if face.outer.len() != 3 || !face.holes.is_empty() {
                return None;
            }
            let point = ray.at(distance);
            face.outer.iter().copied().min_by(|a, b| {
                app.editor
                    .mesh()
                    .pos(*a)
                    .distance_squared(point)
                    .total_cmp(&app.editor.mesh().pos(*b).distance_squared(point))
            })
        });
        let hover = hit.map(|id| app.editor.mesh().pos(id));
        app.smoove.as_mut().unwrap().hover = hover;
        if matches!(event, Pointer::Down { .. }) {
            if let Some(vertex) = hit {
                let vertices = eligible(app.editor.mesh(), &BTreeSet::from([vertex]));
                let centre = app.editor.mesh().pos(vertex);
                let d = app.smoove.as_mut().unwrap();
                d.vertices = vertices;
                d.centres = vec![centre];
                d.origin = Some(centre);
                d.offset = DVec3::ZERO;
                app.message = update(app).err();
            } else {
                app.message = Some("Edit or explode the terrain group, then click a triangle.".into());
            }
        }
    }
}
pub fn value(app: &mut App3d, text: &str) -> Result<(), String> {
    if !app.smoove.as_ref().is_some_and(|d| d.valid(app)) {
        app.smoove = None;
        return Err("Smoove cancelled because the model changed.".into());
    }
    let free = app.modifiers().shift;
    let radius = text.trim().ends_with('r');
    let value = rysmo_model::parse_length(
        if radius { text.trim().trim_end_matches('r') } else { text },
        app.editor.doc().units,
    )
    .map_err(|e| e.to_string())?;
    if let Some(mut command) = app.smoove.as_ref().unwrap().correction.clone() {
        let Command::Smoove { radius: r, normal, offset, .. } = &mut command else { unreachable!() };
        if radius {
            if value < 1e-5 {
                return Err("Enter a positive radius.".into());
            }
            *r = value;
        } else {
            let direction = offset.try_normalize().unwrap_or(*normal);
            *offset = direction * value.abs() * if value < 0. { -1. } else { 1. };
            // Signed vertical values refer to blue, not the previous offset sign.
            if direction.cross(*normal).length() < 1e-7 {
                *offset = *normal * value;
            }
        }
        let (radius, offset) = match &command {
            Command::Smoove { radius, offset, .. } => (*radius, *offset),
            _ => unreachable!(),
        };
        app.editor.amend_last(command.clone()).map_err(|e| e.to_string())?;
        app.smoove_radius = radius;
        begin(app);
        let d = app.smoove.as_mut().unwrap();
        d.radius = radius;
        d.last_offset = (offset.length() >= 1e-10).then_some(offset);
        d.correction = (offset.length() >= 1e-10).then_some(command);
        app.smoove_radius = radius;
        return Ok(());
    }
    let d = app.smoove.as_mut().unwrap();
    if radius || d.origin.is_none() {
        if value < 1e-5 {
            return Err("Enter a positive radius.".into());
        }
        d.radius = value;
        app.smoove_radius = value;
        if d.origin.is_some() {
            update(app)?;
        }
        Ok(())
    } else {
        let direction = if free && !d.preselected { d.offset.try_normalize().unwrap_or(d.normal) } else { d.normal };
        d.offset = direction * value;
        update(app)?;
        finish(app)
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(d) = &app.smoove else { return };
    if !d.valid(app) {
        return;
    }
    let view = app.local_view();
    let axes = app.editor.active().axes;
    let line = |a: DVec3, b: DVec3, color| {
        if let (Some(a), Some(b)) = (view.project(a), view.project(b)) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(1.3, color),
            );
        }
    };
    if let Some(mesh) = &d.preview {
        for (_, edge) in mesh.edges().filter(|(_, e)| {
            e.visible() && e.v.iter().any(|id| mesh.pos(*id).distance(app.editor.mesh().pos(*id)) > 1e-8)
        }) {
            line(mesh.pos(edge.v[0]), mesh.pos(edge.v[1]), egui::Color32::MAGENTA);
        }
    }
    if d.origin.is_some() {
        for (id, seed) in &d.affected {
            let point = d.preview.as_ref().map(|m| m.pos(*id)).unwrap_or_else(|| app.editor.mesh().pos(*id));
            if let Some(p) = view.project(point) {
                painter.circle_filled(
                    egui::pos2(p.x as f32, p.y as f32),
                    if *seed { 5. } else { 2.5 },
                    egui::Color32::MAGENTA,
                );
            }
        }
    }
    if let Some(centre) = d.origin.or(d.hover) {
        for i in 0..64 {
            let point = |i: usize| {
                let angle = i as f64 * std::f64::consts::TAU / 64.;
                centre + (axes.x * angle.cos() + axes.y * angle.sin()) * d.radius
            };
            line(point(i), point(i + 1), egui::Color32::DARK_RED);
        }
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
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., spacing: 1. })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        app.editor.enter(id).unwrap();
        app
    }
    fn click(app: &mut App3d, p: DVec3) {
        let at = app.view().project(p).unwrap();
        crate::input::tool_pointer(app, at, Pointer::Down { clicks: 1 });
    }
    #[test]
    fn zero_correction_restores_the_original_terrain_and_removes_the_step() {
        let mut app = app();
        let before = app.editor.doc().clone();
        let label = app.editor.undo_label().map(str::to_owned);
        begin(&mut app);
        click(&mut app, DVec3::new(2., 2., 0.));
        value(&mut app, "1m").unwrap();
        value(&mut app, "0m").unwrap();
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_label(), label.as_deref());
        assert!(app.smoove.as_ref().unwrap().correction.is_none());
    }
    #[test]
    fn correction_replaces_offset_and_radius_in_one_undo() {
        let mut app = app();
        let before = app.editor.doc().clone();
        begin(&mut app);
        click(&mut app, DVec3::new(2., 2., 0.));
        value(&mut app, "1m").unwrap();
        value(&mut app, "2m").unwrap();
        value(&mut app, "1mr").unwrap();
        let vertex = app.editor.mesh().find_vertex(DVec3::new(2., 2., 2.)).unwrap();
        assert_eq!(app.editor.mesh().pos(vertex).z, 2.);
        let rim = app.editor.mesh().find_vertex(DVec3::new(3., 2., 0.)).unwrap();
        assert_eq!(app.editor.mesh().pos(rim).z, 0.);
        let corrected = app.editor.doc().clone();
        let revision = app.editor.revision();
        for input in ["bad", "0r"] {
            assert!(value(&mut app, input).is_err());
        }
        assert_eq!(app.editor.doc(), &corrected);
        assert_eq!(app.editor.revision(), revision);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn double_click_repeats_the_last_offset() {
        let mut app = app();
        begin(&mut app);
        click(&mut app, DVec3::new(2., 2., 0.));
        value(&mut app, "50cm").unwrap();
        let centre = DVec3::new(2., 2., 0.5);
        let vertex = app.editor.mesh().find_vertex(centre).unwrap();
        click(&mut app, centre);
        let revision = app.editor.revision();
        value(&mut app, "0m").unwrap();
        assert_eq!(app.editor.revision(), revision);
        click(&mut app, centre);
        let at = app.view().project(centre).unwrap();
        pointer(&mut app, at, Pointer::Down { clicks: 2 });
        assert!((app.editor.mesh().pos(vertex).z - 1.).abs() < 1e-9);
        app.editor.undo();
        assert!((app.editor.mesh().pos(vertex).z - 0.5).abs() < 1e-9);
    }
    #[test]
    fn shift_free_move_and_revision_change_cancel_safely() {
        let mut app = app();
        begin(&mut app);
        click(&mut app, DVec3::new(2., 2., 0.));
        app.held.shift = true;
        let at = app.view().project(DVec3::new(2.5, 2., 0.25)).unwrap();
        pointer(&mut app, at, Pointer::Move);
        assert!(app.smoove.as_ref().unwrap().offset.cross(DVec3::Z).length() > 0.1);
        let before = app.editor.doc().clone();
        app.editor.apply(Command::AddGuidePoint { point: DVec3::X }).unwrap();
        let changed = app.editor.doc().clone();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.smoove.is_none());
        assert_eq!(app.editor.doc(), &changed);
        assert_ne!(app.editor.doc(), &before);
    }
    #[test]
    fn mouse_preview_and_commit_have_one_undo() {
        let mut app = app();
        let before = app.editor.doc().clone();
        begin(&mut app);
        value(&mut app, "2mr").unwrap();
        click(&mut app, DVec3::new(2., 2., 0.));
        let centre = app.smoove.as_ref().unwrap().origin.unwrap();
        let at = app.view().project(centre + DVec3::Z * 0.75).unwrap();
        pointer(&mut app, at, Pointer::Move);
        assert_eq!(app.editor.doc(), &before);
        let vertex = app.editor.mesh().find_vertex(centre).unwrap();
        assert!((app.smoove.as_ref().unwrap().preview.as_ref().unwrap().pos(vertex).z - 0.75).abs() < 1e-8);
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert_eq!(app.editor.undo_label(), Some("Smoove"));
        assert!((app.editor.mesh().pos(vertex).z - 0.75).abs() < 1e-8);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn typed_radius_offset_preselection_and_escape() {
        let mut app = app();
        let before = app.editor.doc().clone();
        let vertex = app.editor.mesh().find_vertex(DVec3::new(2., 2., 0.)).unwrap();
        let edge = app.editor.mesh().edges().find(|(_, edge)| edge.v.contains(&vertex)).unwrap().0;
        app.editor.set_selection([Element::Edge(edge)]);
        begin(&mut app);
        assert!(app.smoove.as_ref().unwrap().preselected);
        assert!(!app.smoove.as_ref().unwrap().affected.is_empty());
        value(&mut app, "150cmr").unwrap();
        assert_eq!(app.smoove.as_ref().unwrap().radius, 1.5);
        assert!(value(&mut app, "0r").is_err());
        assert!(value(&mut app, "bad").is_err());
        assert_eq!(app.editor.doc(), &before);
        value(&mut app, "-50cm").unwrap();
        assert_eq!(app.editor.mesh().pos(vertex).z, -0.5);
        value(&mut app, "2mr").unwrap();
        let d = app.smoove.as_ref().unwrap();
        let influence = rysmo_model::terrain::SmooveInfluence::new(&d.centres, d.normal).unwrap();
        let expected = d.vertices.iter().filter(|id| influence.weight(app.editor.mesh().pos(**id), 2.) > 1e-12).count();
        assert_eq!(d.affected.len(), expected);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.smoove.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        app.set_tool(ToolKind::Line);
        assert!(app.smoove.is_none());
    }
}
