//! The Select tool: clicks, double and triple clicks, boxes, and opening groups and
//! components for editing.

use std::collections::BTreeSet;

use rysmo_infer::Pick;
use rysmo_math::DVec2;
use rysmo_mesh::{Element, Mesh};
use rysmo_model::Editor;

use super::{Ctx, State, ToolBox};

impl ToolBox {
    pub(crate) fn select_click(&mut self, editor: &mut Editor, ctx: &Ctx, clicks: u8) {
        self.state = State::Selecting { from: ctx.cursor };
        let el = match self.pick_full(editor, ctx) {
            Pick::Element(e) => e,
            Pick::Outside if !editor.context().is_empty() => {
                // A click outside the group being edited closes it.
                editor.exit();
                return;
            }
            _ => {
                if !ctx.modifiers.ctrl && !ctx.modifiers.shift {
                    editor.clear_selection();
                }
                return;
            }
        };
        let mesh = editor.mesh();
        let set: Vec<Element> = match (clicks, el) {
            (1, Element::Edge(e)) if mesh.edge(e).and_then(|e| e.curve).is_some() => {
                mesh.curve_edges(mesh.edge(e).unwrap().curve.unwrap()).into_iter().map(Element::Edge).collect()
            }
            (1, Element::Face(face)) if !editor.doc().style.show_hidden_geometry => {
                soft_surface(mesh, face).into_iter().map(Element::Face).collect()
            }
            (1, _) => vec![el],
            // A double click opens a group or component.
            (_, Element::Instance(i)) => {
                if let Err(e) = editor.enter(i) {
                    self.message = Some(e.to_string());
                }
                self.state = State::Idle;
                return;
            }
            (2, Element::Face(f)) => {
                let faces = if editor.doc().style.show_hidden_geometry { vec![f] } else { soft_surface(mesh, f) };
                faces
                    .iter()
                    .copied()
                    .map(Element::Face)
                    .chain(
                        faces
                            .iter()
                            .flat_map(|f| mesh.face_edges(*f))
                            .filter(|id| {
                                editor.doc().style.show_hidden_geometry || mesh.edge(*id).is_some_and(|e| e.visible())
                            })
                            .map(Element::Edge),
                    )
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            }
            (2, Element::Edge(e)) => {
                std::iter::once(el).chain(mesh.faces_of_edge(e).into_iter().map(Element::Face)).collect()
            }
            (_, Element::Section(id)) => {
                if let Err(error) = editor.apply(rysmo_model::Command::ActivateSection { id: Some(id) }) {
                    self.message = Some(error.to_string());
                }
                vec![el]
            }
            (_, Element::Guide(_) | Element::Dimension(_) | Element::Text(_)) => vec![el],
            _ => connected(mesh, el),
        };
        let set: Vec<_> = set
            .into_iter()
            .filter(|element| match element {
                Element::Face(id) => {
                    editor.doc().style.show_hidden_geometry || !editor.mesh().face(*id).unwrap().hidden
                }
                Element::Edge(id) => {
                    editor.doc().style.show_hidden_geometry || !editor.mesh().edge(*id).unwrap().hidden
                }
                _ => true,
            })
            .collect();
        let m = ctx.modifiers;
        match (m.ctrl, m.shift) {
            (true, true) => editor.remove_from_selection(set),
            (true, false) => editor.add_to_selection(set),
            (false, true) if clicks == 1 => editor.toggle_selection(set),
            _ => editor.set_selection(set),
        }
    }

    pub(crate) fn box_select(&mut self, editor: &mut Editor, ctx: &Ctx, from: DVec2, to: DVec2) {
        let crossing = to.x < from.x;
        let lo = from.min(to);
        let hi = from.max(to);
        let inside = |p: DVec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
        let evaluated = editor.doc().effective_mesh(editor.active_definition());
        let mesh = &*evaluated;
        let seg_hits = |a: DVec2, b: DVec2| inside(a) || inside(b) || segment_hits_rect(a, b, lo, hi);
        let mut found = Vec::new();
        for (id, e) in mesh.edges() {
            if e.procedural || (!e.visible() && !editor.doc().style.show_hidden_geometry) {
                continue;
            }
            let (a, b) = mesh.edge_points(id);
            let Some((sa, sb)) = ctx.view.project_segment(a, b) else { continue };
            let hit = if crossing { seg_hits(sa, sb) } else { inside(sa) && inside(sb) };
            if hit {
                found.push(Element::Edge(id));
            }
        }
        for (id, f) in mesh.faces() {
            if f.hidden && !editor.doc().style.show_hidden_geometry {
                continue;
            }
            let pts: Option<Vec<DVec2>> = f.outer.iter().map(|v| ctx.view.project(mesh.pos(*v))).collect();
            let Some(pts) = pts else { continue };
            let hit = if crossing {
                let boundaries = f.loops().any(|vertices| {
                    (0..vertices.len()).any(|k| {
                        ctx.view
                            .project_segment(mesh.pos(vertices[k]), mesh.pos(vertices[(k + 1) % vertices.len()]))
                            .is_some_and(|(a, b)| seg_hits(a, b))
                    })
                });
                let ray = ctx.view.ray((lo + hi) * 0.5);
                boundaries
                    || mesh
                        .face_plane(id)
                        .and_then(|plane| plane.intersect_ray(&ray))
                        .is_some_and(|depth| mesh.face_contains(id, ray.at(depth)))
            } else {
                pts.iter().all(|p| inside(*p))
            };
            if hit {
                found.push(Element::Face(id));
            }
        }
        if editor.doc().style.show_guides {
            for (id, guide) in &editor.active().guides {
                if guide.hidden() && !editor.doc().style.show_hidden_geometry {
                    continue;
                }
                let hit = match guide {
                    rysmo_model::Guide::Point { p, origin: None, .. } => ctx.view.project(*p).is_some_and(inside),
                    rysmo_model::Guide::Point { p, origin: Some(a), .. } => ctx
                        .view
                        .project_segment(*a, *p)
                        .is_some_and(|(a, b)| if crossing { seg_hits(a, b) } else { inside(a) && inside(b) }),
                    rysmo_model::Guide::Line { origin, dir, .. } => {
                        crossing
                            && ctx
                                .view
                                .project_segment(*origin - *dir * 1e6, *origin + *dir * 1e6)
                                .is_some_and(|(a, b)| seg_hits(a, b))
                    }
                };
                if hit {
                    found.push(Element::Guide(*id));
                }
            }
        }
        let doc = editor.doc();
        for (id, inst) in &editor.active().instances {
            if !doc.tag_visible(inst.tag) || (inst.hidden && !doc.style.show_hidden_geometry) {
                continue;
            }
            let Some((blo, bhi)) = doc.definition_bounds(Some(inst.definition)) else { continue };
            let lines = super::box_lines(blo, bhi, &inst.transform);
            let hit = if crossing {
                lines.iter().filter_map(|(a, b)| ctx.view.project_segment(*a, *b)).any(|(a, b)| seg_hits(a, b))
                    || ray_hits_bounds(ctx.view.local(inst.transform).ray((lo + hi) * 0.5), blo, bhi)
            } else {
                lines.iter().all(|(a, b)| {
                    ctx.view.project(*a).zip(ctx.view.project(*b)).is_some_and(|(a, b)| inside(a) && inside(b))
                })
            };
            let hit = self
                .annotation_box
                .as_ref()
                .and_then(|hits| hits.instances.get(id))
                .map_or(hit, |(any, all)| if crossing { hit || *any } else { hit && *all });
            if hit {
                found.push(Element::Instance(*id));
            }
        }
        if !editor.doc().style.show_hidden_geometry {
            let initial: BTreeSet<_> =
                found.iter().filter_map(|e| if let Element::Face(id) = e { Some(*id) } else { None }).collect();
            let mut expanded = BTreeSet::new();
            for face in &initial {
                let surface = soft_surface(mesh, *face);
                if crossing || surface.iter().all(|f| initial.contains(f)) {
                    expanded.extend(surface);
                }
            }
            found.retain(|e| !matches!(e, Element::Face(_)));
            found.extend(expanded.into_iter().map(Element::Face));
        }
        if let Some(hits) = &self.annotation_box {
            found.extend(hits.elements.iter().copied());
        }
        let m = ctx.modifiers;
        match (m.ctrl, m.shift) {
            (true, true) => editor.remove_from_selection(found),
            (true, false) => editor.add_to_selection(found),
            (false, true) => editor.toggle_selection(found),
            _ => editor.set_selection(found),
        }
    }
}

/// Everything connected to an element through shared vertices.
pub(super) fn connected(mesh: &Mesh, start: Element) -> Vec<Element> {
    let mut verts: BTreeSet<_> = mesh.vertices_of(&[start]);
    loop {
        let before = verts.len();
        for (_, e) in mesh.edges() {
            if verts.contains(&e.v[0]) || verts.contains(&e.v[1]) {
                verts.extend(e.v);
            }
        }
        if verts.len() == before {
            break;
        }
    }
    let mut out: Vec<Element> =
        mesh.edges().filter(|(_, e)| verts.contains(&e.v[0])).map(|(id, _)| Element::Edge(id)).collect();
    out.extend(mesh.faces().filter(|(_, f)| verts.contains(&f.outer[0])).map(|(id, _)| Element::Face(id)));
    out
}

/// Group bounds also have a projected interior; a crossing box wholly inside
/// that silhouette must not depend on intersecting one of its twelve edges.
fn ray_hits_bounds(ray: rysmo_math::Ray, lo: rysmo_math::DVec3, hi: rysmo_math::DVec3) -> bool {
    let mut enter: f64 = 0.0;
    let mut leave = f64::INFINITY;
    for axis in 0..3 {
        if ray.dir[axis].abs() < 1e-12 {
            if ray.origin[axis] < lo[axis] - 1e-9 || ray.origin[axis] > hi[axis] + 1e-9 {
                return false;
            }
        } else {
            let a = (lo[axis] - ray.origin[axis]) / ray.dir[axis];
            let b = (hi[axis] - ray.origin[axis]) / ray.dir[axis];
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
            if enter > leave + 1e-9 {
                return false;
            }
        }
    }
    true
}

fn segment_hits_rect(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> bool {
    let delta = b - a;
    let mut enter: f64 = 0.0;
    let mut leave: f64 = 1.0;
    for axis in 0..2 {
        if delta[axis].abs() < 1e-12 {
            if a[axis] < lo[axis] || a[axis] > hi[axis] {
                return false;
            }
        } else {
            let near = (lo[axis] - a[axis]) / delta[axis];
            let far = (hi[axis] - a[axis]) / delta[axis];
            enter = enter.max(near.min(far));
            leave = leave.min(near.max(far));
            if enter > leave {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crossing_rectangle_includes_collinear_boundary_contact() {
        assert!(segment_hits_rect(DVec2::new(-1., 0.), DVec2::new(2., 0.), DVec2::ZERO, DVec2::ONE));
        assert!(!segment_hits_rect(DVec2::new(-1., -0.1), DVec2::new(2., -0.1), DVec2::ZERO, DVec2::ONE));
    }
}

/// Soft edges join visible facets into one selectable surface.
fn soft_surface(mesh: &Mesh, start: rysmo_mesh::FaceId) -> Vec<rysmo_mesh::FaceId> {
    let mut result = BTreeSet::from([start]);
    let mut pending = vec![start];
    while let Some(face) = pending.pop() {
        for edge in mesh.face_edges(face) {
            if !mesh.edge(edge).is_some_and(|e| e.soft && !e.hidden) {
                continue;
            }
            for neighbour in mesh.faces_of_edge(edge) {
                if mesh.face(neighbour).is_some_and(|f| !f.hidden) && result.insert(neighbour) {
                    pending.push(neighbour);
                }
            }
        }
    }
    result.into_iter().collect()
}
