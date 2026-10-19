//! Text annotations: screen positions, view-based and model-space leaders.
use crate::{
    Document,
    dimensions::{DimensionAnchor, DimensionFont, PointReference},
};
use rysmo_math::{DMat4, DVec2, DVec3, View};
use rysmo_mesh::{DefinitionId, FaceId, InstanceId, VertexId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextDefaults {
    pub screen_font: TextFont,
    pub leader_font: TextFont,
    pub endpoint: TextEndpoint,
    pub leader: TextLeaderKind,
}
impl Default for TextDefaults {
    fn default() -> Self {
        Self {
            screen_font: Default::default(),
            leader_font: Default::default(),
            endpoint: TextEndpoint::ClosedArrow,
            leader: TextLeaderKind::ViewBased,
        }
    }
}
impl TextDefaults {
    pub fn validate(&self) -> bool {
        [self.screen_font, self.leader_font].iter().all(|f| f.size.is_finite() && (1.0..=512.0).contains(&f.size))
    }
    /// Starts a screen-text draft at a logical pixel, snapshotting model defaults.
    pub fn screen(&self, content: String, position: DVec2, viewport: DVec2) -> Option<Text> {
        if !self.validate() || !position.is_finite() || !viewport.is_finite() || viewport.min_element() <= 0. {
            return None;
        }
        Some(Text {
            content,
            placement: TextPlacement::Screen { position: position / viewport },
            font: self.screen_font,
            endpoint: TextEndpoint::None,
        })
    }
    /// Starts an attached draft at a screen label position in the current occurrence.
    pub fn attached(
        &self,
        content: String,
        anchor: TextAnchor,
        position: DVec2,
        doc: &Document,
        owner: Option<DefinitionId>,
        transform: DMat4,
        view: &View,
    ) -> Option<Text> {
        if !self.validate()
            || !anchor.validate()
            || !position.is_finite()
            || !transform.is_finite()
            || !transform.inverse().is_finite()
        {
            return None;
        }
        let point = transform.transform_point3(anchor.resolve(doc, owner).0);
        if view.view.transform_point3(point).z > -1e-4 {
            return None;
        }
        let offset = position - view.project(point)?;
        let draft = Text {
            content,
            placement: TextPlacement::Attached { anchor, leader: TextLeader::ViewBased { offset } },
            font: self.leader_font,
            endpoint: self.endpoint,
        };
        draft.with_leader(doc, owner, transform, view, self.leader)
    }
    pub fn apply_appearance(&self, text: &mut Text) {
        text.font =
            if matches!(text.placement, TextPlacement::Screen { .. }) { self.screen_font } else { self.leader_font };
        text.endpoint = self.endpoint;
    }
}

pub type TextFont = DimensionFont;
fn zero_offset(offset: &DVec3) -> bool {
    *offset == DVec3::ZERO
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextEndpoint {
    None,
    Dot,
    #[default]
    ClosedArrow,
    OpenArrow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextReference {
    Point {
        reference: PointReference,
    },
    /// Barycentric attachment to three vertices on a face. Weights may be
    /// negative when the chosen triangle does not cover the entire polygon.
    Face {
        path: Vec<InstanceId>,
        face: FaceId,
        vertices: [VertexId; 3],
        weights: [f64; 3],
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextAnchor {
    pub point: DVec3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<TextReference>,
}

impl TextAnchor {
    /// Records a face hit using a non-degenerate vertex frame, including nested owners.
    pub fn on_face(
        doc: &Document,
        owner: Option<DefinitionId>,
        path: Vec<InstanceId>,
        face: FaceId,
        point: DVec3,
    ) -> Option<Self> {
        let mut definition = owner;
        let mut transform = DMat4::IDENTITY;
        for id in &path {
            let instance = doc.def(definition)?.instances.get(id)?;
            transform *= instance.transform;
            definition = Some(instance.definition);
        }
        let mesh = &doc.def(definition)?.mesh;
        let polygon = mesh.face(face)?;
        let local = transform.inverse().transform_point3(point);
        if !local.is_finite() || !mesh.face_plane(face)?.contains(local) || !mesh.face_contains(face, local) {
            return None;
        }
        let first = *polygon.outer.first()?;
        let origin = mesh.pos(first);
        for a in 1..polygon.outer.len() {
            for b in a + 1..polygon.outer.len() {
                let u = mesh.pos(polygon.outer[a]) - origin;
                let v = mesh.pos(polygon.outer[b]) - origin;
                let delta = local - origin;
                let determinant = u.length_squared() * v.length_squared() - u.dot(v).powi(2);
                if determinant.abs() <= u.length_squared() * v.length_squared() * 1e-12 {
                    continue;
                }
                let x = (delta.dot(u) * v.length_squared() - delta.dot(v) * u.dot(v)) / determinant;
                let y = (delta.dot(v) * u.length_squared() - delta.dot(u) * u.dot(v)) / determinant;
                let anchor = Self {
                    point,
                    reference: Some(TextReference::Face {
                        path: path.clone(),
                        face,
                        vertices: [first, polygon.outer[a], polygon.outer[b]],
                        weights: [1. - x - y, x, y],
                    }),
                };
                if anchor.validate() {
                    return Some(anchor);
                }
            }
        }
        None
    }
    pub fn validate(&self) -> bool {
        self.point.is_finite()
            && self.reference.as_ref().is_none_or(|r| match r {
                TextReference::Point { reference } => {
                    DimensionAnchor { point: self.point, reference: Some(reference.clone()) }.validate()
                }
                TextReference::Face { vertices, weights, .. } => {
                    vertices[0] != vertices[1]
                        && vertices[1] != vertices[2]
                        && vertices[0] != vertices[2]
                        && weights.iter().all(|w| w.is_finite())
                        && (weights.iter().sum::<f64>() - 1.).abs() < 1e-8
                }
            })
    }
    pub fn resolve(&self, doc: &Document, owner: Option<DefinitionId>) -> (DVec3, bool) {
        let Some(reference) = &self.reference else { return (self.point, false) };
        match reference {
            TextReference::Point { reference } => {
                DimensionAnchor { point: self.point, reference: Some(reference.clone()) }.resolve(doc, owner)
            }
            TextReference::Face { path, face, vertices, weights } => {
                let live = (|| {
                    let mut definition = owner;
                    let mut transform = DMat4::IDENTITY;
                    for id in path {
                        let instance = doc.def(definition)?.instances.get(id)?;
                        transform *= instance.transform;
                        definition = Some(instance.definition);
                    }
                    let mesh = &doc.def(definition)?.mesh;
                    let polygon = mesh.face(*face)?;
                    if !vertices.iter().all(|v| polygon.loops().any(|l| l.contains(v))) {
                        return None;
                    }
                    let mut point = DVec3::ZERO;
                    for (vertex, weight) in vertices.iter().zip(weights) {
                        point += mesh.vertex(*vertex)?.pos * *weight;
                    }
                    Some(transform.transform_point3(point))
                })();
                live.filter(|p| p.is_finite()).map_or((self.point, false), |p| (p, true))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextLeaderKind {
    None,
    ViewBased,
    Pushpin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextLeader {
    None {
        #[serde(default, skip_serializing_if = "zero_offset")]
        offset: DVec3,
    },
    /// Logical pixels relative to the projected attachment point.
    ViewBased { offset: DVec2 },
    /// Owner-local displacement relative to the attachment point.
    Pushpin { offset: DVec3 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextPlacement {
    /// Fraction of viewport size; remains fixed when the camera moves.
    Screen {
        position: DVec2,
    },
    Attached {
        anchor: TextAnchor,
        leader: TextLeader,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub content: String,
    pub placement: TextPlacement,
    #[serde(default)]
    pub font: TextFont,
    #[serde(default)]
    pub endpoint: TextEndpoint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    pub centred: bool,
    pub position: DVec2,
    pub anchor: Option<DVec2>,
    pub associated: bool,
}

pub type TextKey = crate::dimensions::DimensionTextKey;

/// Screen geometry shared by rendering, pointer picking and rectangle selection.
#[derive(Clone, Debug)]
pub struct TextGeometry {
    pub rect: crate::dimensions::DimensionTextRect,
    pub segments: Vec<[DVec2; 2]>,
    pub markers: Vec<crate::dimensions::DimensionEndMarker>,
}
impl TextGeometry {
    pub fn hit_distance(&self, point: DVec2) -> f64 {
        self.segments
            .iter()
            .map(|[a, b]| rysmo_math::distance_point_segment_2d(point, *a, *b))
            .chain(self.markers.iter().map(|m| m.distance(point)))
            .fold(self.rect.distance(point), f64::min)
    }
    pub fn box_hit(&self, lo: DVec2, hi: DVec2, crossing: bool) -> bool {
        use crate::dimensions::DimensionEndMarker::*;
        let inside = |p: DVec2| p.cmpge(lo).all() && p.cmple(hi).all();
        let segment = |a: DVec2, b: DVec2| {
            let d = b - a;
            let (mut enter, mut leave): (f64, f64) = (0., 1.);
            for axis in 0..2 {
                if d[axis].abs() < 1e-12 {
                    if a[axis] < lo[axis] || a[axis] > hi[axis] {
                        return false;
                    }
                } else {
                    let x = (lo[axis] - a[axis]) / d[axis];
                    let y = (hi[axis] - a[axis]) / d[axis];
                    enter = enter.max(x.min(y));
                    leave = leave.min(x.max(y));
                    if enter > leave {
                        return false;
                    }
                }
            }
            true
        };
        if crossing {
            self.rect.crosses(lo, hi)
                || self.segments.iter().any(|[a, b]| segment(*a, *b))
                || self.markers.iter().any(|m| match m {
                    Line([a, b]) => segment(*a, *b),
                    Triangle(p) => (0..3).any(|i| segment(p[i], p[(i + 1) % 3])) || m.distance((lo + hi) * 0.5) == 0.,
                    Dot { centre, radius } => centre.distance(centre.clamp(lo, hi)) <= *radius,
                })
        } else {
            self.rect.corners().into_iter().all(inside)
                && self.segments.iter().flatten().copied().all(inside)
                && self.markers.iter().all(|m| match m {
                    Line(p) => p.iter().copied().all(inside),
                    Triangle(p) => p.iter().copied().all(inside),
                    Dot { centre, radius } => {
                        inside(*centre - DVec2::splat(*radius)) && inside(*centre + DVec2::splat(*radius))
                    }
                })
        }
    }
}

impl Text {
    /// View-based/pushpin conversion preserves projected label placement.
    /// Removing the leader retains its model-space label displacement.
    pub fn with_leader(
        &self,
        doc: &Document,
        owner: Option<DefinitionId>,
        transform: DMat4,
        view: &View,
        kind: TextLeaderKind,
    ) -> Option<Self> {
        let TextPlacement::Attached { anchor, leader } = &self.placement else { return None };
        let current = match leader {
            TextLeader::None { .. } => TextLeaderKind::None,
            TextLeader::ViewBased { .. } => TextLeaderKind::ViewBased,
            TextLeader::Pushpin { .. } => TextLeaderKind::Pushpin,
        };
        if current == kind {
            return Some(self.clone());
        }
        if kind == TextLeaderKind::None {
            let mut result = self.clone();
            if let TextPlacement::Attached { leader, .. } = &mut result.placement {
                let offset = match leader {
                    TextLeader::Pushpin { offset } | TextLeader::None { offset } => *offset,
                    TextLeader::ViewBased { .. } => {
                        let layout = self.layout(doc, owner, transform, view)?;
                        let point = anchor.resolve(doc, owner).0;
                        let ray = view.ray(layout.position);
                        let plane = rysmo_math::Plane::new(
                            view.view.transpose().transform_vector3(DVec3::Z),
                            transform.transform_point3(point),
                        );
                        transform.inverse().transform_point3(ray.at(plane.intersect_ray(&ray)?)) - point
                    }
                };
                *leader = TextLeader::None { offset };
            }
            return Some(result);
        }
        let layout = self.layout(doc, owner, transform, view)?;
        let point = anchor.resolve(doc, owner).0;
        let world = transform.transform_point3(point);
        let projected = view.project(world)?;
        let position =
            if matches!(leader, TextLeader::None {offset} if *offset==DVec3::ZERO) && kind != TextLeaderKind::None {
                projected + DVec2::new(80., -40.)
            } else {
                layout.position
            };
        let leader = match kind {
            TextLeaderKind::None => TextLeader::None { offset: DVec3::ZERO },
            TextLeaderKind::ViewBased => TextLeader::ViewBased { offset: position - projected },
            TextLeaderKind::Pushpin => {
                let ray = view.ray(position);
                let normal = view.view.transpose().transform_vector3(DVec3::Z);
                let plane = rysmo_math::Plane::new(normal, world);
                let depth = plane.intersect_ray(&ray)?;
                let offset = transform.inverse().transform_point3(ray.at(depth)) - point;
                if !offset.is_finite() {
                    return None;
                }
                TextLeader::Pushpin { offset }
            }
        };
        let mut result = self.clone();
        result.placement = TextPlacement::Attached { anchor: anchor.clone(), leader };
        Some(result)
    }
    /// Repositions a label without moving or detaching its geometry attachment.
    pub fn at_label_position(
        &self,
        doc: &Document,
        owner: Option<DefinitionId>,
        transform: DMat4,
        view: &View,
        position: DVec2,
    ) -> Option<Self> {
        if !position.is_finite()
            || !view.size.is_finite()
            || view.size.min_element() <= 0.
            || !transform.inverse().is_finite()
        {
            return None;
        }
        let mut result = self.clone();
        match &mut result.placement {
            TextPlacement::Screen { position: stored } => *stored = (position - view.offset) / view.size,
            TextPlacement::Attached { anchor, leader } => {
                let point = anchor.resolve(doc, owner).0;
                match leader {
                    TextLeader::ViewBased { offset } => {
                        *offset = position - view.project(transform.transform_point3(point))?
                    }
                    TextLeader::Pushpin { offset } | TextLeader::None { offset } => {
                        let world = transform.transform_point3(point + *offset);
                        let plane = rysmo_math::Plane::new(view.view.transpose().transform_vector3(DVec3::Z), world);
                        let ray = view.ray(position);
                        let depth = plane.intersect_ray(&ray)?;
                        *offset = transform.inverse().transform_point3(ray.at(depth)) - point;
                    }
                }
            }
        }
        result.validate().then_some(result)
    }
    pub fn estimated_size(&self) -> DVec2 {
        let lines = self.content.split('\n').collect::<Vec<_>>();
        DVec2::new(
            lines.iter().map(|line| line.chars().count()).max().unwrap_or(0) as f64 * self.font.size as f64 * 0.6,
            lines.len() as f64 * self.font.size as f64 * 1.3,
        )
    }
    pub fn text_key(&self) -> TextKey {
        (self.content.clone(), self.font.size.to_bits(), self.font.monospace)
    }
    pub fn geometry(&self, layout: &TextLayout, size: DVec2) -> Option<TextGeometry> {
        use crate::dimensions::{DimensionEndMarker, DimensionTextRect};
        if !size.is_finite() || size.min_element() < 0. || !layout.position.is_finite() {
            return None;
        }
        let centre = if layout.centred {
            layout.position
        } else if let Some(tip) = layout.anchor {
            layout.position + DVec2::X * size.x * 0.5 * if layout.position.x >= tip.x { 1. } else { -1. }
        } else {
            layout.position + size * 0.5
        };
        let rect = DimensionTextRect { centre, axis: DVec2::X, size };
        let mut result = TextGeometry { rect, segments: vec![], markers: vec![] };
        if let Some(tip) = layout.anchor {
            let sign = if layout.position.x >= tip.x { 1. } else { -1. };
            let end = centre - DVec2::X * size.x * 0.5 * sign;
            let elbow = end - DVec2::X * 12. * sign;
            result.segments.extend([[tip, elbow], [elbow, end]]);
            let direction = (elbow - tip).normalize_or_zero();
            if direction.length_squared() > 0. {
                let base = tip + direction * 7.;
                let side = DVec2::new(-direction.y, direction.x) * 2.5;
                match self.endpoint {
                    TextEndpoint::None => {}
                    TextEndpoint::Dot => result.markers.push(DimensionEndMarker::Dot { centre: tip, radius: 2.5 }),
                    TextEndpoint::ClosedArrow => {
                        result.markers.push(DimensionEndMarker::Triangle([tip, base + side, base - side]))
                    }
                    TextEndpoint::OpenArrow => result.markers.extend([
                        DimensionEndMarker::Line([tip, base + side]),
                        DimensionEndMarker::Line([tip, base - side]),
                    ]),
                }
            }
        }
        Some(result)
    }
    pub fn validate(&self) -> bool {
        (self.font.size.is_finite() && (1.0..=512.0).contains(&self.font.size))
            && match &self.placement {
                TextPlacement::Screen { position } => position.is_finite(),
                TextPlacement::Attached { anchor, leader } => {
                    anchor.validate()
                        && match leader {
                            TextLeader::None { offset } => offset.is_finite(),
                            TextLeader::ViewBased { offset } => offset.is_finite(),
                            TextLeader::Pushpin { offset } => offset.is_finite(),
                        }
                }
            }
    }
    pub fn layout(
        &self,
        doc: &Document,
        owner: Option<DefinitionId>,
        transform: DMat4,
        view: &View,
    ) -> Option<TextLayout> {
        match &self.placement {
            TextPlacement::Screen { position } => Some(TextLayout {
                centred: false,
                position: view.offset + *position * view.size,
                anchor: None,
                associated: false,
            }),
            TextPlacement::Attached { anchor, leader } => {
                let (point, associated) = anchor.resolve(doc, owner);
                let world = transform.transform_point3(point);
                if view.view.transform_point3(world).z > -1e-4 {
                    return None;
                }
                let projected = view.project(world)?;
                let (position, tip) = match leader {
                    TextLeader::None { offset } => {
                        let end = transform.transform_point3(point + *offset);
                        if view.view.transform_point3(end).z > -1e-4 {
                            return None;
                        }
                        (view.project(end)?, None)
                    }
                    TextLeader::ViewBased { offset } => (projected + *offset, Some(projected)),
                    TextLeader::Pushpin { offset } => {
                        let end = transform.transform_point3(point + *offset);
                        if view.view.transform_point3(end).z > -1e-4 {
                            return None;
                        }
                        (view.project(end)?, Some(projected))
                    }
                };
                Some(TextLayout {
                    centred: matches!(leader, TextLeader::None { .. }),
                    position,
                    anchor: tip,
                    associated,
                })
            }
        }
    }
    pub fn refreshed(&self, doc: &Document, owner: Option<DefinitionId>) -> Self {
        let mut result = self.clone();
        if let TextPlacement::Attached { anchor, .. } = &mut result.placement {
            anchor.point = anchor.resolve(doc, owner).0;
        }
        result
    }
    pub(crate) fn transformed_links(
        &self,
        source: &rysmo_mesh::Mesh,
        matrix: DMat4,
        vertices: &std::collections::BTreeSet<VertexId>,
        instances: &[InstanceId],
    ) -> Self {
        use crate::dimensions::PointEntity;
        if matrix.abs_diff_eq(DMat4::IDENTITY, 1e-10) {
            return self.clone();
        }
        let mut text = self.clone();
        if let TextPlacement::Attached { anchor, leader } = &mut text.placement {
            anchor.point = matrix.transform_point3(anchor.point);
            if let TextLeader::Pushpin { offset } | TextLeader::None { offset } = leader {
                *offset = matrix.transform_vector3(*offset);
            }
            let moved = match &anchor.reference {
                Some(TextReference::Point { reference }) => {
                    reference.path.first().map(|id| instances.contains(id)).unwrap_or_else(|| match reference.entity {
                        PointEntity::Vertex { vertex } => vertices.contains(&vertex),
                        PointEntity::Edge { edge, .. } => {
                            source.edge(edge).is_some_and(|e| e.v.iter().all(|v| vertices.contains(v)))
                        }
                        PointEntity::CurveCentre { curve } => {
                            let edges = source.curve_edges(curve);
                            !edges.is_empty()
                                && edges.iter().all(|id| {
                                    source.edge(*id).is_some_and(|e| e.v.iter().all(|v| vertices.contains(v)))
                                })
                        }
                    })
                }
                Some(TextReference::Face { path, vertices: frame, .. }) => path
                    .first()
                    .map(|id| instances.contains(id))
                    .unwrap_or_else(|| frame.iter().all(|id| vertices.contains(id))),
                None => false,
            };
            if !moved {
                anchor.reference = None;
            }
        }
        text
    }
    pub(crate) fn copied_links(
        &self,
        source: &rysmo_mesh::Mesh,
        target: &rysmo_mesh::Mesh,
        matrix: DMat4,
        instances: &std::collections::BTreeMap<InstanceId, InstanceId>,
        elements: &[crate::Element],
    ) -> Self {
        use crate::dimensions::PointEntity;
        let vertices = source.vertices_of(elements);
        let edges = elements
            .iter()
            .flat_map(|e| match e {
                crate::Element::Edge(id) => vec![*id],
                crate::Element::Face(id) => source.face_edges(*id),
                _ => vec![],
            })
            .collect::<std::collections::BTreeSet<_>>();
        let copied = match &self.placement {
            TextPlacement::Screen { .. } => false,
            TextPlacement::Attached { anchor, .. } => match &anchor.reference {
                Some(TextReference::Point { reference }) => {
                    reference.path.first().map(|id| instances.contains_key(id)).unwrap_or_else(|| {
                        match reference.entity {
                            PointEntity::Vertex { vertex } => vertices.contains(&vertex),
                            PointEntity::Edge { edge, .. } => edges.contains(&edge),
                            PointEntity::CurveCentre { curve } => {
                                let members = source.curve_edges(curve);
                                !members.is_empty() && members.iter().all(|id| edges.contains(id))
                            }
                        }
                    })
                }
                Some(TextReference::Face { path, face, .. }) => {
                    path.first().map(|id| instances.contains_key(id)).unwrap_or_else(|| {
                        let members = source.face_edges(*face);
                        !members.is_empty() && members.iter().all(|id| edges.contains(id))
                    })
                }
                None => false,
            },
        };
        if copied {
            self.remapped(source, target, matrix, instances)
        } else if [DVec3::X, DVec3::Y, DVec3::Z]
            .iter()
            .any(|axis| matrix.transform_vector3(*axis).distance(*axis) > 1e-10)
        {
            self.transformed_links(source, matrix, &std::collections::BTreeSet::new(), &[])
        } else {
            self.clone()
        }
    }
    pub(crate) fn remapped(
        &self,
        source: &rysmo_mesh::Mesh,
        target: &rysmo_mesh::Mesh,
        transform: DMat4,
        instances: &std::collections::BTreeMap<InstanceId, InstanceId>,
    ) -> Self {
        let mut result = self.clone();
        if let TextPlacement::Attached { anchor, leader } = &mut result.placement {
            anchor.point = transform.transform_point3(anchor.point);
            if let TextLeader::Pushpin { offset } | TextLeader::None { offset } = leader {
                *offset = transform.transform_vector3(*offset);
            }
            anchor.reference = anchor.reference.take().and_then(|reference| match reference {
                TextReference::Point { mut reference } => {
                    if let Some(first) = reference.path.first_mut() {
                        *first = *instances.get(first)?;
                    } else {
                        reference.entity =
                            crate::dimensions::mapped_entity(source, target, transform, reference.entity)?;
                    }
                    Some(TextReference::Point { reference })
                }
                TextReference::Face { mut path, face, vertices, weights } => {
                    if let Some(first) = path.first_mut() {
                        *first = *instances.get(first)?;
                        return Some(TextReference::Face { path, face, vertices, weights });
                    }
                    let mut mapped = vertices;
                    for (new, old) in mapped.iter_mut().zip(vertices) {
                        *new = target.find_vertex(transform.transform_point3(source.vertex(old)?.pos))?;
                    }
                    let original = source.face(face)?;
                    let original_vertices = original
                        .outer
                        .iter()
                        .map(|v| target.find_vertex(transform.transform_point3(source.pos(*v))))
                        .collect::<Option<Vec<_>>>()?;
                    let face = target
                        .faces()
                        .find(|(_, f)| {
                            f.outer.len() == original_vertices.len()
                                && original_vertices.iter().all(|v| f.outer.contains(v))
                        })?
                        .0;
                    Some(TextReference::Face { path, face, vertices: mapped, weights })
                }
            });
        }
        result
    }
    /// Retargets external attachments after exploding or uniquifying an instance.
    pub(crate) fn remap_instance_reference(
        &mut self,
        instance: InstanceId,
        source: &rysmo_mesh::Mesh,
        target: &rysmo_mesh::Mesh,
        instances: &std::collections::BTreeMap<InstanceId, InstanceId>,
        transform: DMat4,
        remove: bool,
    ) {
        let TextPlacement::Attached { anchor, .. } = &self.placement else { return };
        let Some(reference) = &anchor.reference else { return };
        let path = match reference {
            TextReference::Point { reference } => &reference.path,
            TextReference::Face { path, .. } => path,
        };
        let Some(index) = path.iter().position(|id| *id == instance) else { return };
        let mut prefix = path[..index].to_vec();
        if !remove {
            prefix.push(instance);
        }
        let suffix = path[index + 1..].to_vec();
        let mut probe = self.clone();
        if let TextPlacement::Attached { anchor, .. } = &mut probe.placement {
            match anchor.reference.as_mut().unwrap() {
                TextReference::Point { reference } => reference.path = suffix,
                TextReference::Face { path, .. } => *path = suffix,
            }
        }
        let mapped = probe.remapped(source, target, transform, instances);
        if let TextPlacement::Attached { anchor: mapped, .. } = mapped.placement {
            let mut reference = mapped.reference;
            if let Some(reference) = &mut reference {
                let path = match reference {
                    TextReference::Point { reference } => &mut reference.path,
                    TextReference::Face { path, .. } => path,
                };
                prefix.append(path);
                *path = prefix;
            }
            if let TextPlacement::Attached { anchor, .. } = &mut self.placement {
                anchor.reference = reference;
            }
        }
    }

    /// Keeps annotations outside a new group linked to geometry moved into it.
    pub(crate) fn reparented(
        &self,
        doc: &Document,
        owner: Option<DefinitionId>,
        parent: Option<DefinitionId>,
        instance: InstanceId,
        source: &rysmo_mesh::Mesh,
        target: &rysmo_mesh::Mesh,
        transform: DMat4,
        children: &[InstanceId],
    ) -> Self {
        let mut result = self.clone();
        let TextPlacement::Attached { anchor, .. } = &mut result.placement else { return result };
        let Some(reference) = &mut anchor.reference else { return result };
        let path = match reference {
            TextReference::Point { reference } => &mut reference.path,
            TextReference::Face { path, .. } => path,
        };
        for index in 0..path.len() {
            if children.contains(&path[index])
                && crate::dimensions::context(doc, owner, &path[..index]).is_some_and(|(d, _)| d == parent)
            {
                path.insert(index, instance);
                return result;
            }
        }
        if !crate::dimensions::context(doc, owner, path).is_some_and(|(d, _)| d == parent) {
            return result;
        }
        let mesh = &doc.def(parent).unwrap().mesh;
        let retained = match reference {
            TextReference::Point { reference } => match reference.entity {
                crate::dimensions::PointEntity::Vertex { vertex } => mesh.vertex(vertex).is_some(),
                crate::dimensions::PointEntity::Edge { edge, .. } => mesh.edge(edge).is_some(),
                crate::dimensions::PointEntity::CurveCentre { curve } => mesh.curve(curve).is_some(),
            },
            TextReference::Face { face, .. } => mesh.face(*face).is_some(),
        };
        if retained {
            return result;
        }
        let mut local = reference.clone();
        let path = match &mut local {
            TextReference::Point { reference } => std::mem::take(&mut reference.path),
            TextReference::Face { path, .. } => std::mem::take(path),
        };
        let mut probe = self.clone();
        if let TextPlacement::Attached { anchor, .. } = &mut probe.placement {
            anchor.reference = Some(local);
        }
        let mapped = probe.remapped(source, target, transform, &Default::default());
        if let TextPlacement::Attached { anchor: TextAnchor { reference: Some(mut mapped), .. }, .. } = mapped.placement
        {
            let mapped_path = match &mut mapped {
                TextReference::Point { reference } => &mut reference.path,
                TextReference::Face { path, .. } => path,
            };
            *mapped_path = path;
            mapped_path.push(instance);
            anchor.reference = Some(mapped);
        }
        result
    }
    pub fn detached(&self, doc: &Document, owner: Option<DefinitionId>) -> Self {
        let mut text = self.refreshed(doc, owner);
        if let TextPlacement::Attached { anchor, .. } = &mut text.placement {
            anchor.reference = None;
        }
        text
    }
    pub fn scaled(mut self, scale: f64) -> Self {
        if let TextPlacement::Attached { anchor, leader } = &mut self.placement {
            anchor.point *= scale;
            if let TextLeader::Pushpin { offset } | TextLeader::None { offset } = leader {
                *offset *= scale;
            }
        }
        self
    }
}

impl Document {
    pub(crate) fn refresh_texts(&mut self) {
        let values: Vec<_> = std::iter::once((None, &self.root))
            .chain(self.definitions.iter().map(|(id, d)| (Some(*id), d)))
            .flat_map(|(owner, d)| d.texts.iter().map(move |(id, text)| (owner, *id, text)))
            .map(|(owner, id, text)| (owner, id, text.refreshed(self, owner)))
            .collect();
        for (owner, id, text) in values {
            self.def_mut(owner).unwrap().texts.insert(id, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimensions::{PointEntity, PointReference};
    use crate::{Command, Editor, Element};
    fn note(placement: TextPlacement) -> Text {
        Text {
            content: "Přízemí\nArea 2 m²".into(),
            placement,
            font: Default::default(),
            endpoint: TextEndpoint::OpenArrow,
        }
    }
    #[test]
    fn screen_text_placement_respects_docked_canvas_origin() {
        let document = Document::default();
        let mut view = rysmo_math::OrbitCamera::default().view(DVec2::new(800., 500.));
        view.offset = DVec2::new(30., 80.);
        let text = TextDefaults::default().screen("Canvas".into(), DVec2::new(200., 100.), view.size).unwrap();
        let layout = text.layout(&document, None, DMat4::IDENTITY, &view).unwrap();
        assert_eq!(layout.position, DVec2::new(230., 180.));
        let moved = text.at_label_position(&document, None, DMat4::IDENTITY, &view, DVec2::new(430., 330.)).unwrap();
        assert_eq!(moved.layout(&document, None, DMat4::IDENTITY, &view).unwrap().position, DVec2::new(430., 330.));
    }
    #[test]
    fn nested_external_attachment_retargets_child_paths() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let anchor = TextAnchor {
            point: DVec3::X * 0.5,
            reference: Some(TextReference::Point {
                reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
            }),
        };
        let id = editor
            .apply(Command::AddText {
                text: note(TextPlacement::Attached { anchor, leader: TextLeader::None { offset: DVec3::Y } }),
            })
            .unwrap()
            .texts[0];
        let result = editor.apply(Command::MakeGroup { elements: vec![Element::Edge(edge)] }).unwrap();
        let Element::Instance(first) = result.created[0] else { panic!() };
        let result = editor.apply(Command::MakeGroup { elements: vec![Element::Instance(first)] }).unwrap();
        let Element::Instance(outer) = result.created[0] else { panic!() };
        let before = editor.doc().clone();
        editor.apply(Command::MakeUnique { instance: outer }).unwrap();
        let result = editor.apply(Command::Explode { instance: outer }).unwrap();
        let child =
            result.created.iter().find_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None }).unwrap();
        assert_ne!(child, first);
        editor.apply(Command::Move { elements: vec![Element::Instance(child)], offset: DVec3::Z * 4. }).unwrap();
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        let Some(TextReference::Point { reference }) = &anchor.reference else { panic!() };
        assert_eq!(reference.path, vec![child]);
        let (position, linked) = anchor.resolve(editor.doc(), None);
        assert!(linked);
        assert!((position - DVec3::new(0.5, 0., 4.)).length() < 1e-9);
        editor.undo();
        editor.undo();
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn external_face_and_edge_links_survive_unique_and_explode() {
        for face_link in [false, true] {
            for explode in [false, true] {
                let mut editor = Editor::new();
                editor
                    .apply(Command::DrawRectangle {
                        corner: DVec3::new(3., 2., 0.),
                        u: DVec3::X * 2.,
                        v: DVec3::Y * 2.,
                        facing: None,
                    })
                    .unwrap();
                let face = editor.mesh().faces().next().unwrap().0;
                let edge = editor.mesh().edges().next().unwrap().0;
                let anchor = if face_link {
                    TextAnchor::on_face(editor.doc(), None, vec![], face, DVec3::new(4., 3., 0.)).unwrap()
                } else {
                    let line = editor.mesh().edge(edge).unwrap();
                    TextAnchor {
                        point: (editor.mesh().pos(line.v[0]) + editor.mesh().pos(line.v[1])) * 0.5,
                        reference: Some(TextReference::Point {
                            reference: PointReference {
                                path: vec![],
                                entity: PointEntity::Edge { edge, parameter: 0.5 },
                            },
                        }),
                    }
                };
                let original = anchor.point;
                let text_id = editor
                    .apply(Command::AddText {
                        text: note(TextPlacement::Attached {
                            anchor,
                            leader: TextLeader::Pushpin { offset: DVec3::Z },
                        }),
                    })
                    .unwrap()
                    .texts[0];
                let result = editor
                    .apply(Command::MakeComponent { elements: vec![Element::Face(face)], name: Some("Slab".into()) })
                    .unwrap();
                let Element::Instance(instance) = result.created[0] else { panic!() };
                editor
                    .apply(Command::Copy {
                        elements: vec![Element::Instance(instance)],
                        offset: DVec3::X * 10.,
                        copies: 1,
                    })
                    .unwrap();
                let before = editor.doc().clone();
                editor
                    .apply(if explode { Command::Explode { instance } } else { Command::MakeUnique { instance } })
                    .unwrap();
                if !explode {
                    editor.enter(instance).unwrap();
                }
                let new_face = editor.mesh().faces().next().unwrap().0;
                editor.apply(Command::Move { elements: vec![Element::Face(new_face)], offset: DVec3::Z * 3. }).unwrap();
                if !explode {
                    editor.exit();
                }
                let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&text_id].placement else {
                    panic!()
                };
                let (position, linked) = anchor.resolve(editor.doc(), None);
                assert!(linked, "face={face_link}, explode={explode}");
                assert!((position - original - DVec3::Z * 3.).length() < 1e-9);
                editor.undo();
                editor.undo();
                assert_eq!(editor.doc(), &before);
            }
        }
    }

    #[test]
    fn external_face_label_follows_new_group_and_nested_regrouping() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(3., 2., 0.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: None,
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let point = DVec3::new(4., 3., 0.);
        let anchor = TextAnchor::on_face(editor.doc(), None, vec![], face, point).unwrap();
        let id = editor
            .apply(Command::AddText {
                text: note(TextPlacement::Attached {
                    anchor,
                    leader: TextLeader::ViewBased { offset: DVec2::new(40., -20.) },
                }),
            })
            .unwrap()
            .texts[0];
        let before = editor.doc().clone();
        let result = editor.apply(Command::MakeGroup { elements: vec![Element::Face(face)] }).unwrap();
        let Element::Instance(first) = result.created[0] else { panic!() };
        let result = editor.apply(Command::MakeGroup { elements: vec![Element::Instance(first)] }).unwrap();
        let Element::Instance(second) = result.created[0] else { panic!() };
        editor.apply(Command::Move { elements: vec![Element::Instance(second)], offset: DVec3::Z * 2. }).unwrap();
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        let Some(TextReference::Face { path, .. }) = &anchor.reference else { panic!() };
        assert_eq!(path, &vec![second, first]);
        let (position, linked) = anchor.resolve(editor.doc(), None);
        assert!(linked);
        assert!((position - point - DVec3::Z * 2.).length() < 1e-9);
        editor.undo();
        editor.undo();
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn grouped_text_and_external_labels_keep_geometry_links_and_undo() {
        for selected in [false, true] {
            for component in [false, true] {
                let mut editor = Editor::new();
                let origin = DVec3::new(4., 3., 2.);
                editor
                    .apply(Command::DrawLines { lines: vec![[origin, origin + DVec3::X * 2.]], facing: None })
                    .unwrap();
                let edge = editor.mesh().edges().next().unwrap().0;
                let text = note(TextPlacement::Attached {
                    anchor: TextAnchor {
                        point: origin + DVec3::X,
                        reference: Some(TextReference::Point {
                            reference: PointReference {
                                path: vec![],
                                entity: PointEntity::Edge { edge, parameter: 0.5 },
                            },
                        }),
                    },
                    leader: TextLeader::Pushpin { offset: DVec3::Z },
                });
                let text_id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
                let before = editor.doc().clone();
                let mut elements = vec![Element::Edge(edge)];
                if selected {
                    elements.push(Element::Text(text_id));
                }
                let result = editor
                    .apply(if component {
                        Command::MakeComponent { elements, name: Some("Annotated".into()) }
                    } else {
                        Command::MakeGroup { elements }
                    })
                    .unwrap();
                let Element::Instance(instance) = result.created[0] else { panic!() };
                let inst = &editor.active().instances[&instance];
                let owner = selected.then_some(inst.definition);
                let transform = if selected { inst.transform } else { DMat4::IDENTITY };
                let grouped = &editor.doc().def(owner).unwrap().texts[&text_id];
                let TextPlacement::Attached { anchor, .. } = &grouped.placement else { panic!() };
                let (position, linked) = anchor.resolve(editor.doc(), owner);
                assert!(linked);
                assert!((transform.transform_point3(position) - origin - DVec3::X).length() < 1e-9);
                assert_eq!(grouped.font, text.font);
                editor
                    .apply(Command::Move { elements: vec![Element::Instance(instance)], offset: DVec3::Y * 5. })
                    .unwrap();
                let inst = &editor.active().instances[&instance];
                let transform = if selected { inst.transform } else { DMat4::IDENTITY };
                let TextPlacement::Attached { anchor, .. } =
                    &editor.doc().def(owner).unwrap().texts[&text_id].placement
                else {
                    panic!()
                };
                let (position, linked) = anchor.resolve(editor.doc(), owner);
                assert!(linked);
                assert!((transform.transform_point3(position) - origin - DVec3::X - DVec3::Y * 5.).length() < 1e-9);
                editor.undo();
                editor.undo();
                assert_eq!(editor.doc(), &before);
            }
        }
    }

    #[test]
    fn text_only_group_keeps_fallback_and_screen_position() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let text = note(TextPlacement::Attached {
            anchor: TextAnchor {
                point: DVec3::X * 0.5,
                reference: Some(TextReference::Point {
                    reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
                }),
            },
            leader: TextLeader::None { offset: DVec3::Y },
        });
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        let screen = note(TextPlacement::Screen { position: DVec2::new(0.3, 0.2) });
        let screen_id = editor.apply(Command::AddText { text: screen.clone() }).unwrap().texts[0];
        let before = editor.doc().clone();
        let result =
            editor.apply(Command::MakeGroup { elements: vec![Element::Text(id), Element::Text(screen_id)] }).unwrap();
        let Element::Instance(instance) = result.created[0] else { panic!() };
        let inst = &editor.active().instances[&instance];
        let owner = Some(inst.definition);
        let texts = &editor.doc().def(owner).unwrap().texts;
        assert_eq!(texts[&screen_id], screen);
        let TextPlacement::Attached { anchor, .. } = &texts[&id].placement else { panic!() };
        let (point, linked) = anchor.resolve(editor.doc(), owner);
        assert!(!linked);
        assert!((inst.transform.transform_point3(point) - DVec3::X * 0.5).length() < 1e-9);
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn screen_and_leader_types_follow_their_camera_contracts() {
        let doc = Document::new();
        let mut camera = rysmo_math::OrbitCamera::default();
        let size = DVec2::new(1200., 800.);
        let screen = note(TextPlacement::Screen { position: DVec2::new(0.2, 0.3) });
        let anchor = TextAnchor { point: DVec3::ZERO, reference: None };
        let view_based = note(TextPlacement::Attached {
            anchor: anchor.clone(),
            leader: TextLeader::ViewBased { offset: DVec2::new(80., -40.) },
        });
        let pushpin = note(TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset: DVec3::X } });
        let before = pushpin.layout(&doc, None, DMat4::IDENTITY, &camera.view(size)).unwrap();
        camera.yaw += 0.7;
        camera.zoom_at(0.8, size * 0.5, size);
        for camera in [rysmo_math::OrbitCamera::default(), camera] {
            let view = camera.view(size);
            assert_eq!(screen.layout(&doc, None, DMat4::IDENTITY, &view).unwrap().position, DVec2::new(240., 240.));
            let layout = view_based.layout(&doc, None, DMat4::IDENTITY, &view).unwrap();
            assert!((layout.position - layout.anchor.unwrap() - DVec2::new(80., -40.)).length() < 1e-9);
            let layout = pushpin.layout(&doc, None, DMat4::IDENTITY, &view).unwrap();
            assert!((layout.position - view.project(DVec3::X).unwrap()).length() < 1e-9);
        }
        let after = pushpin.layout(&doc, None, DMat4::IDENTITY, &camera.view(size)).unwrap();
        assert!(before.position.distance(after.position) > 1.);
        assert_eq!(screen.scaled(1000.).placement, TextPlacement::Screen { position: DVec2::new(0.2, 0.3) });
        assert_eq!(view_based.clone().scaled(1000.).font, view_based.font);
    }
    #[test]
    fn live_attachment_retains_last_position_after_erasure_and_undo() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let text = note(TextPlacement::Attached {
            anchor: TextAnchor {
                point: DVec3::X * 0.5,
                reference: Some(TextReference::Point {
                    reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
                }),
            },
            leader: TextLeader::ViewBased { offset: DVec2::new(50., -30.) },
        });
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        editor.apply(Command::Move { elements: vec![Element::Edge(edge)], offset: DVec3::Y * 2. }).unwrap();
        let anchor = |editor: &Editor| {
            let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
            anchor.resolve(editor.doc(), None)
        };
        assert_eq!(anchor(&editor), (DVec3::new(0.5, 2., 0.), true));
        editor.apply(Command::Erase { elements: vec![Element::Edge(edge)] }).unwrap();
        assert_eq!(anchor(&editor), (DVec3::new(0.5, 2., 0.), false));
        editor.undo();
        assert_eq!(anchor(&editor), (DVec3::new(0.5, 2., 0.), true));
        let revision = editor.revision();
        editor.apply(Command::SetText { id, text: editor.active().texts[&id].clone() }).unwrap();
        assert_eq!(editor.revision(), revision);
        let before = editor.doc().clone();
        assert!(editor.apply(Command::EraseTexts { ids: vec![id, crate::TextId(u64::MAX)] }).is_err());
        assert_eq!(editor.doc(), &before);
        editor.apply(Command::EraseTexts { ids: vec![id] }).unwrap();
        editor.undo();
        assert!(editor.active().texts.contains_key(&id));
    }
    #[test]
    fn face_attachment_follows_deformation_and_rejects_invalid_weights() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: None })
            .unwrap();
        let (face, polygon) = editor.mesh().faces().next().unwrap();
        let vertices = [polygon.outer[0], polygon.outer[1], polygon.outer[2]];
        let point = vertices.iter().map(|v| editor.mesh().pos(*v)).sum::<DVec3>() / 3.;
        let anchor = TextAnchor {
            point,
            reference: Some(TextReference::Face { path: vec![], face, vertices, weights: [1. / 3.; 3] }),
        };
        let id = editor
            .apply(Command::AddText {
                text: note(TextPlacement::Attached { anchor, leader: TextLeader::None { offset: DVec3::ZERO } }),
            })
            .unwrap()
            .texts[0];
        editor.apply(Command::Move { elements: vec![Element::Face(face)], offset: DVec3::Z }).unwrap();
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), None), (point + DVec3::Z, true));
        let invalid = TextAnchor {
            point,
            reference: Some(TextReference::Face { path: vec![], face, vertices, weights: [0.1; 3] }),
        };
        assert!(!invalid.validate());
    }
    #[test]
    fn group_unique_and_explode_preserve_text_with_fresh_references() {
        let mut editor = Editor::new();
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
        let (face, polygon) = editor.mesh().faces().next().unwrap();
        let vertices = [polygon.outer[0], polygon.outer[1], polygon.outer[2]];
        let point = vertices.iter().map(|v| editor.mesh().pos(*v)).sum::<DVec3>() / 3.;
        let id = editor
            .apply(Command::AddText {
                text: note(TextPlacement::Attached {
                    anchor: TextAnchor {
                        point,
                        reference: Some(TextReference::Face { path: vec![], face, vertices, weights: [1. / 3.; 3] }),
                    },
                    leader: TextLeader::Pushpin { offset: DVec3::Y },
                }),
            })
            .unwrap()
            .texts[0];
        editor.exit();
        let Element::Instance(copy) = editor
            .apply(Command::Copy { elements: vec![Element::Instance(group)], offset: DVec3::X * 4., copies: 1 })
            .unwrap()
            .created[0]
        else {
            panic!()
        };
        editor.enter(copy).unwrap(); // group copy becomes unique
        let (new_id, text) = editor.active().texts.iter().next().unwrap();
        assert_ne!(*new_id, id);
        let TextPlacement::Attached { anchor, .. } = &text.placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (point, true));
        editor.exit();
        editor.apply(Command::Explode { instance: copy }).unwrap();
        let text = editor.active().texts.values().next().unwrap();
        let TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset } } = &text.placement else {
            panic!()
        };
        assert!((anchor.resolve(editor.doc(), None).0 - point - DVec3::X * 4.).length() < 1e-9);
        assert!(anchor.resolve(editor.doc(), None).1);
        assert_eq!(*offset, DVec3::Y);
        assert!(
            editor.doc().def(Some(editor.doc().root.instances[&group].definition)).unwrap().texts.contains_key(&id)
        );
    }
    #[test]
    fn text_geometry_shares_visible_labels_leaders_and_endpoints_with_selection() {
        let doc = Document::new();
        let size = DVec2::new(120., 40.);
        for endpoint in [TextEndpoint::None, TextEndpoint::Dot, TextEndpoint::OpenArrow, TextEndpoint::ClosedArrow] {
            for direction in [-1., 1.] {
                let text = Text { endpoint, ..note(TextPlacement::Screen { position: DVec2::ZERO }) };
                let layout = TextLayout {
                    position: DVec2::new(300., 200.),
                    anchor: Some(DVec2::new(300. - direction * 150., 300.)),
                    associated: true,
                    centred: false,
                };
                let geometry = text.geometry(&layout, size).unwrap();
                assert_eq!(geometry.rect.centre.x, 300. + direction * 60.);
                assert_eq!(geometry.hit_distance(geometry.rect.centre), 0.);
                let [tip, elbow] = geometry.segments[0];
                let middle = (tip + elbow) * 0.5;
                assert_eq!(geometry.hit_distance(middle), 0.);
                assert!(geometry.box_hit(middle - DVec2::splat(2.), middle + DVec2::splat(2.), true));
                assert!(!geometry.box_hit(middle - DVec2::splat(2.), middle + DVec2::splat(2.), false));
                assert!(geometry.box_hit(DVec2::ZERO, DVec2::splat(1000.), false));
                assert!(!geometry.box_hit(DVec2::ZERO, DVec2::splat(10.), true));
                for marker in &geometry.markers {
                    use crate::dimensions::DimensionEndMarker::*;
                    let point = match marker {
                        Line([a, b]) => (*a + *b) * 0.5,
                        Triangle(p) => (p[0] + p[1] + p[2]) / 3.,
                        Dot { centre, .. } => *centre,
                    };
                    assert_eq!(geometry.hit_distance(point), 0.);
                    assert!(geometry.box_hit(point - DVec2::splat(0.1), point + DVec2::splat(0.1), true));
                }
            }
        }
        let text = note(TextPlacement::Screen { position: DVec2::new(0.2, 0.3) });
        let layout = text
            .layout(&doc, None, DMat4::IDENTITY, &rysmo_math::OrbitCamera::default().view(DVec2::new(1000., 800.)))
            .unwrap();
        let geometry = text.geometry(&layout, size).unwrap();
        assert_eq!(geometry.rect.bounds().0, DVec2::new(200., 240.));
        assert!(geometry.segments.is_empty());
        assert!(geometry.box_hit(DVec2::new(200., 240.), DVec2::new(320., 280.), false));
    }
    #[test]
    fn attached_text_behind_the_eye_is_hidden_in_both_projections() {
        let doc = Document::new();
        for parallel in [false, true] {
            let camera = rysmo_math::OrbitCamera { parallel, ..Default::default() };
            let view = camera.view(DVec2::new(1000., 800.));
            let behind = camera.eye() + (camera.eye() - camera.target).normalize();
            let text = note(TextPlacement::Attached {
                anchor: TextAnchor { point: behind, reference: None },
                leader: TextLeader::None { offset: DVec3::ZERO },
            });
            assert!(text.layout(&doc, None, DMat4::IDENTITY, &view).is_none());
            let text = note(TextPlacement::Attached {
                anchor: TextAnchor { point: camera.target, reference: None },
                leader: TextLeader::Pushpin { offset: behind - camera.target },
            });
            assert!(text.layout(&doc, None, DMat4::IDENTITY, &view).is_none());
        }
    }
    #[test]
    fn leader_conversion_preserves_projection_in_scaled_context_and_undo() {
        let mut editor = Editor::new();
        let text = note(TextPlacement::Attached {
            anchor: TextAnchor { point: DVec3::ZERO, reference: None },
            leader: TextLeader::ViewBased { offset: DVec2::new(100., -70.) },
        });
        let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
        for parallel in [false, true] {
            let camera = rysmo_math::OrbitCamera { parallel, ..Default::default() };
            let view = camera.view(DVec2::new(1200., 800.));
            let transform = DMat4::from_rotation_z(0.4) * DMat4::from_scale(DVec3::new(2., 0.5, 3.));
            let before = text.layout(editor.doc(), None, transform, &view).unwrap();
            let pushpin = text.with_leader(editor.doc(), None, transform, &view, TextLeaderKind::Pushpin).unwrap();
            assert!(pushpin.validate());
            assert!(
                before.position.distance(pushpin.layout(editor.doc(), None, transform, &view).unwrap().position) < 1e-8
            );
            let back = pushpin.with_leader(editor.doc(), None, transform, &view, TextLeaderKind::ViewBased).unwrap();
            assert!(
                before.position.distance(back.layout(editor.doc(), None, transform, &view).unwrap().position) < 1e-8
            );
            assert_eq!(
                pushpin.with_leader(editor.doc(), None, transform, &view, TextLeaderKind::Pushpin).unwrap(),
                pushpin
            );
            editor.apply(Command::SetText { id, text: pushpin }).unwrap();
            editor.undo();
            assert_eq!(editor.active().texts[&id], text);
        }
    }
    #[test]
    fn appearance_defaults_update_selected_types_atomically_and_undo() {
        let mut editor = Editor::new();
        let screen = note(TextPlacement::Screen { position: DVec2::new(0.2, 0.3) });
        let attached = note(TextPlacement::Attached {
            anchor: TextAnchor { point: DVec3::ZERO, reference: None },
            leader: TextLeader::ViewBased { offset: DVec2::new(80., -40.) },
        });
        let a = editor.apply(Command::AddText { text: screen.clone() }).unwrap().texts[0];
        let b = editor.apply(Command::AddText { text: attached.clone() }).unwrap().texts[0];
        let mut style = editor.doc().style;
        style.text_defaults.screen_font.size = 28.;
        style.text_defaults.leader_font.size = 18.;
        style.text_defaults.endpoint = TextEndpoint::Dot;
        editor.apply(Command::SetStyle { style }).unwrap();
        let before = editor.doc().clone();
        assert!(editor.apply(Command::ApplyTextAppearance { ids: vec![a, crate::TextId(u64::MAX)] }).is_err());
        assert_eq!(editor.doc(), &before);
        editor.apply(Command::ApplyTextAppearance { ids: vec![a, b] }).unwrap();
        assert_eq!(editor.active().texts[&a].font.size, 28.);
        assert_eq!(editor.active().texts[&b].font.size, 18.);
        assert_eq!(editor.active().texts[&b].placement, attached.placement);
        assert_eq!(editor.active().texts[&a].content, screen.content);
        let revision = editor.revision();
        editor.apply(Command::ApplyTextAppearance { ids: vec![a, b] }).unwrap();
        assert_eq!(editor.revision(), revision);
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn bulk_defaults_convert_leaders_atomically() {
        let mut editor = Editor::new();
        let text = note(TextPlacement::Attached {
            anchor: TextAnchor { point: DVec3::ZERO, reference: None },
            leader: TextLeader::ViewBased { offset: DVec2::new(100., -50.) },
        });
        let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
        let mut style = editor.doc().style;
        style.text_defaults.leader = TextLeaderKind::Pushpin;
        editor.apply(Command::SetStyle { style }).unwrap();
        let camera = rysmo_math::OrbitCamera::default();
        let viewport = DVec2::new(1200., 800.);
        let transform = DMat4::IDENTITY;
        let before = editor.doc().clone();
        assert!(
            editor
                .apply(Command::ApplyTextDefaults {
                    ids: vec![id, crate::TextId(u64::MAX)],
                    camera,
                    viewport,
                    transform
                })
                .is_err()
        );
        assert_eq!(editor.doc(), &before);
        editor.apply(Command::ApplyTextDefaults { ids: vec![id], camera, viewport, transform }).unwrap();
        let view = camera.view(viewport);
        assert!(
            text.layout(editor.doc(), None, transform, &view)
                .unwrap()
                .position
                .distance(editor.active().texts[&id].layout(editor.doc(), None, transform, &view).unwrap().position)
                < 1e-8
        );
        assert!(matches!(
            editor.active().texts[&id].placement,
            TextPlacement::Attached { leader: TextLeader::Pushpin { .. }, .. }
        ));
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn creation_drafts_snapshot_defaults_and_preserve_clicked_positions() {
        let doc = Document::new();
        let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
        let transform = DMat4::from_rotation_z(0.3) * DMat4::from_scale(DVec3::new(2., 0.5, 3.));
        let mut defaults = TextDefaults::default();
        defaults.screen_font.size = 28.;
        defaults.leader_font.size = 20.;
        defaults.endpoint = TextEndpoint::OpenArrow;
        let screen = defaults.screen("Screen".into(), DVec2::new(240., 160.), view.size).unwrap();
        assert_eq!(screen.layout(&doc, None, transform, &view).unwrap().position, DVec2::new(240., 160.));
        assert_eq!(screen.font.size, 28.);
        let position = view.project(DVec3::ZERO).unwrap() + DVec2::new(90., -60.);
        for kind in [TextLeaderKind::ViewBased, TextLeaderKind::Pushpin] {
            defaults.leader = kind;
            let text = defaults
                .attached(
                    "Note".into(),
                    TextAnchor { point: DVec3::ZERO, reference: None },
                    position,
                    &doc,
                    None,
                    transform,
                    &view,
                )
                .unwrap();
            assert_eq!(text.font.size, 20.);
            assert_eq!(text.endpoint, TextEndpoint::OpenArrow);
            assert!(text.layout(&doc, None, transform, &view).unwrap().position.distance(position) < 1e-8);
        }
        defaults.screen_font.size = 10.;
        assert_eq!(screen.font.size, 28.);
        assert!(defaults.screen("Invalid".into(), DVec2::ZERO, DVec2::ZERO).is_none());
    }
    #[test]
    fn face_hit_constructs_an_anchor_that_follows_motion() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: None })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let point = DVec3::new(0.3, 0.7, 0.);
        let anchor = TextAnchor::on_face(editor.doc(), None, vec![], face, point).unwrap();
        assert!(anchor.resolve(editor.doc(), None).0.distance(point) < 1e-9);
        editor.apply(Command::Move { elements: vec![Element::Face(face)], offset: DVec3::Z }).unwrap();
        assert!(anchor.resolve(editor.doc(), None).0.distance(point + DVec3::Z) < 1e-9);
        assert!(TextAnchor::on_face(editor.doc(), None, vec![], face, DVec3::new(4., 4., 1.)).is_none());
    }
    #[test]
    fn repositioning_labels_keeps_geometry_anchors_for_every_leader_kind() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let anchor = TextAnchor {
            point: DVec3::X * 0.5,
            reference: Some(TextReference::Point {
                reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
            }),
        };
        let camera = rysmo_math::OrbitCamera::default();
        let view = camera.view(DVec2::new(1200., 800.));
        let transform = DMat4::from_scale(DVec3::new(2., 0.5, 3.));
        let position = DVec2::new(500., 300.);
        for leader in [
            TextLeader::None { offset: DVec3::ZERO },
            TextLeader::ViewBased { offset: DVec2::new(90., -50.) },
            TextLeader::Pushpin { offset: DVec3::Y },
        ] {
            let original = note(TextPlacement::Attached { anchor: anchor.clone(), leader });
            let moved = original.at_label_position(editor.doc(), None, transform, &view, position).unwrap();
            assert!(moved.layout(editor.doc(), None, transform, &view).unwrap().position.distance(position) < 1e-8);
            let TextPlacement::Attached { anchor: actual, .. } = &moved.placement else { panic!() };
            assert_eq!(actual, &anchor);
            assert!(actual.resolve(editor.doc(), None).1);
            assert_eq!(moved.font, original.font);
            assert_eq!(moved.content, original.content);
        }
        let screen = note(TextPlacement::Screen { position: DVec2::ZERO })
            .at_label_position(editor.doc(), None, transform, &view, position)
            .unwrap();
        assert_eq!(screen.layout(editor.doc(), None, transform, &view).unwrap().position, position);
    }
    #[test]
    fn move_with_text_preserves_attachments_and_moves_geometry_only_once() {
        for together in [false, true] {
            for leader in [
                TextLeader::None { offset: DVec3::Y },
                TextLeader::Pushpin { offset: DVec3::Y },
                TextLeader::ViewBased { offset: DVec2::new(80., -40.) },
            ] {
                let mut editor = Editor::new();
                editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
                let edge = editor.mesh().edges().next().unwrap().0;
                let anchor = TextAnchor {
                    point: DVec3::X * 0.5,
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
                    }),
                };
                let text = note(TextPlacement::Attached { anchor, leader: leader.clone() });
                let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
                let before = editor.doc().clone();
                let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
                let offset = DVec3::new(0.3, 0.2, 0.4);
                let pivot = DVec3::ZERO;
                let mut elements = vec![Element::Text(id)];
                if together {
                    elements.push(Element::Edge(edge));
                }
                let preview = editor.preview_text_move(&elements, offset, pivot, view).unwrap();
                assert_eq!(editor.doc(), &before);
                editor.apply(Command::MoveWithText { elements, offset, pivot, view }).unwrap();
                let text = &editor.active().texts[&id];
                let layout = text.layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap();
                let TextPlacement::Attached { anchor, leader: actual } = &text.placement else { panic!() };
                assert!(anchor.resolve(editor.doc(), None).1);
                match leader {
                    TextLeader::ViewBased { offset: old } => {
                        if together {
                            assert_eq!(actual, &TextLeader::ViewBased { offset: old });
                        }
                    }
                    TextLeader::Pushpin { offset: old } | TextLeader::None { offset: old } => {
                        let expected = view.project(DVec3::X * 0.5 + old + offset).unwrap();
                        assert!(layout.position.distance(expected) < 1e-8);
                        if together {
                            assert_eq!(actual, &leader);
                        } else {
                            assert_eq!(anchor.resolve(editor.doc(), None).0, DVec3::X * 0.5);
                        }
                    }
                }
                assert!(
                    preview[0]
                        .layout(editor.doc(), None, DMat4::IDENTITY, &view)
                        .unwrap()
                        .position
                        .distance(layout.position)
                        < 1e-8
                );
                editor.undo();
                assert_eq!(editor.doc(), &before);
            }
        }
    }
    #[test]
    fn screen_text_move_is_serializable_atomic_and_zero_move_is_not_history() {
        let mut editor = Editor::new();
        let text = note(TextPlacement::Screen { position: DVec2::new(0.2, 0.3) });
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
        let pivot = DVec3::ZERO;
        let offset = DVec3::X * 0.2;
        let command = Command::MoveWithText { elements: vec![Element::Text(id)], offset, pivot, view };
        let encoded = serde_json::to_string(&command).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&encoded).unwrap(), command);
        let before = editor.doc().clone();
        assert!(
            editor
                .apply(Command::MoveWithText {
                    elements: vec![Element::Text(id), Element::Text(crate::TextId(u64::MAX))],
                    offset,
                    pivot,
                    view
                })
                .is_err()
        );
        assert_eq!(editor.doc(), &before);
        let revision = editor.revision();
        editor
            .apply(Command::MoveWithText { elements: vec![Element::Text(id)], offset: DVec3::ZERO, pivot, view })
            .unwrap();
        assert_eq!(editor.revision(), revision);
        editor.apply(command).unwrap();
        let expected = DVec2::new(240., 240.) + view.project(offset).unwrap() - view.project(pivot).unwrap();
        let layout = editor.active().texts[&id].layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap();
        assert!(layout.position.distance(expected) < 1e-8);
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn label_position_in_local_affine_view_preserves_camera_depth() {
        let doc = Document::new();
        let world = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
        let view = world.local(DMat4::from_rotation_z(0.4) * DMat4::from_scale(DVec3::new(2., 0.5, 3.)));
        let text = note(TextPlacement::Attached {
            anchor: TextAnchor { point: DVec3::ZERO, reference: None },
            leader: TextLeader::Pushpin { offset: DVec3::Y },
        });
        let position = text.layout(&doc, None, DMat4::IDENTITY, &view).unwrap().position + DVec2::new(100., -70.);
        let moved = text.at_label_position(&doc, None, DMat4::IDENTITY, &view, position).unwrap();
        let TextPlacement::Attached { leader: TextLeader::Pushpin { offset }, .. } = &moved.placement else { panic!() };
        assert!((view.view.transform_point3(*offset).z - view.view.transform_point3(DVec3::Y).z).abs() < 1e-8);
        assert!(moved.layout(&doc, None, DMat4::IDENTITY, &view).unwrap().position.distance(position) < 1e-8);
    }
    #[test]
    fn group_owned_text_previews_match_committed_instance_transforms() {
        for operation in 0..3 {
            let mut editor = Editor::new();
            editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
            let elements = editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect();
            let Element::Instance(group) = editor.apply(Command::MakeGroup { elements }).unwrap().created[0] else {
                panic!()
            };
            editor.enter(group).unwrap();
            editor
                .apply(Command::AddText {
                    text: note(TextPlacement::Attached {
                        anchor: TextAnchor { point: DVec3::ZERO, reference: None },
                        leader: TextLeader::Pushpin { offset: DVec3::Y },
                    }),
                })
                .unwrap();
            editor.exit();
            let owner = Some(editor.doc().root.instances[&group].definition);
            let elements = vec![Element::Instance(group)];
            let (matrix, command) = match operation {
                0 => {
                    (DMat4::from_translation(DVec3::Y), Command::Move { elements: elements.clone(), offset: DVec3::Y })
                }
                1 => (
                    DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2),
                    Command::Rotate {
                        elements: elements.clone(),
                        centre: DVec3::ZERO,
                        axis: DVec3::Z,
                        angle: 90.,
                        copies: 0,
                    },
                ),
                _ => (
                    DMat4::from_scale(DVec3::new(2., 0.5, 3.)),
                    Command::Scale {
                        elements: elements.clone(),
                        origin: DVec3::ZERO,
                        factors: DVec3::new(2., 0.5, 3.),
                    },
                ),
            };
            let before = editor.doc().clone();
            let preview = editor.preview_instance_texts(&elements, matrix).unwrap();
            assert_eq!(preview.len(), 1);
            assert_eq!(editor.doc(), &before);
            let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
            let predicted = preview[0].0.layout(editor.doc(), owner, preview[0].1, &view).unwrap();
            editor.apply(command).unwrap();
            let occurrence = editor.doc().occurrences(&[]).into_iter().find(|o| o.definition == owner).unwrap();
            let text = editor.doc().def(owner).unwrap().texts.values().next().unwrap();
            let actual = text.layout(editor.doc(), owner, occurrence.transform, &view).unwrap();
            assert!(predicted.position.distance(actual.position) < 1e-8);
            assert!(predicted.anchor.unwrap().distance(actual.anchor.unwrap()) < 1e-8);
            editor.undo();
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn text_copy_arrays_keep_or_retarget_links_and_undo_atomically() {
        for together in [false, true] {
            let mut editor = Editor::new();
            editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
            let edge = editor.mesh().edges().next().unwrap().0;
            let anchor = TextAnchor {
                point: DVec3::X * 0.5,
                reference: Some(TextReference::Point {
                    reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
                }),
            };
            let text = note(TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset: DVec3::Z } });
            let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
            let before = editor.doc().clone();
            let mut elements = vec![Element::Text(id)];
            if together {
                elements.push(Element::Edge(edge));
            }
            let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
            let offset = DVec3::Y;
            let pivot = DVec3::ZERO;
            let preview = editor.preview_text_copy(&elements, offset, pivot, view).unwrap();
            assert_eq!(editor.doc(), &before);
            let out = editor.apply(Command::CopyWithText { elements, offset, copies: 2, pivot, view }).unwrap();
            assert_eq!(out.texts.len(), 2);
            assert_eq!(editor.active().texts[&id], text);
            for (index, new_id) in out.texts.iter().enumerate() {
                let copied = &editor.active().texts[new_id];
                let TextPlacement::Attached { anchor, .. } = &copied.placement else { panic!() };
                assert!(anchor.resolve(editor.doc(), None).1);
                let expected = if together { DVec3::X * 0.5 + offset * (index + 1) as f64 } else { DVec3::X * 0.5 };
                assert!(anchor.resolve(editor.doc(), None).0.distance(expected) < 1e-8);
                let layout = copied.layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap();
                assert!(
                    layout
                        .position
                        .distance(view.project(DVec3::X * 0.5 + DVec3::Z + offset * (index + 1) as f64).unwrap())
                        < 1e-8
                );
                if index == 0 {
                    assert!(
                        preview[0]
                            .layout(editor.doc(), None, DMat4::IDENTITY, &view)
                            .unwrap()
                            .position
                            .distance(layout.position)
                            < 1e-8
                    );
                }
            }
            editor.undo();
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn copied_face_and_instance_text_follow_only_their_copied_geometry() {
        for nested in [false, true] {
            let mut editor = Editor::new();
            editor
                .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: None })
                .unwrap();
            let face = editor.mesh().faces().next().unwrap().0;
            let (path, face, geometry) = if nested {
                let elements =
                    editor.mesh().edges().map(|(id, _)| Element::Edge(id)).chain([Element::Face(face)]).collect();
                let Element::Instance(group) = editor.apply(Command::MakeGroup { elements }).unwrap().created[0] else {
                    panic!()
                };
                let definition = editor.doc().root.instances[&group].definition;
                let face = editor.doc().def(Some(definition)).unwrap().mesh.faces().next().unwrap().0;
                (vec![group], face, Element::Instance(group))
            } else {
                (vec![], face, Element::Face(face))
            };
            let point = DVec3::new(0.3, 0.6, 0.);
            let anchor = TextAnchor::on_face(editor.doc(), None, path, face, point).unwrap();
            let text = note(TextPlacement::Attached {
                anchor,
                leader: TextLeader::ViewBased { offset: DVec2::new(80., -40.) },
            });
            let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
            let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
            let offset = DVec3::Y * 2.;
            let out = editor
                .apply(Command::CopyWithText {
                    elements: vec![geometry, Element::Text(id)],
                    offset,
                    copies: 1,
                    pivot: DVec3::ZERO,
                    view,
                })
                .unwrap();
            let copied_id = out.texts[0];
            let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&copied_id].placement else { panic!() };
            assert!(anchor.resolve(editor.doc(), None).1);
            assert!(anchor.resolve(editor.doc(), None).0.distance(point + offset) < 1e-8);
            let copied_geometry = out
                .created
                .iter()
                .copied()
                .find(|e| {
                    matches!(
                        (geometry, e),
                        (Element::Face(_), Element::Face(_)) | (Element::Instance(_), Element::Instance(_))
                    )
                })
                .unwrap();
            editor.apply(Command::Move { elements: vec![copied_geometry], offset: DVec3::Z }).unwrap();
            let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&copied_id].placement else { panic!() };
            assert!(anchor.resolve(editor.doc(), None).0.distance(point + offset + DVec3::Z) < 1e-8);
            let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
            assert!(anchor.resolve(editor.doc(), None).0.distance(point) < 1e-8);
        }
    }
    #[test]
    fn screen_and_no_leader_copy_preserve_originals_and_reject_bad_selections() {
        let mut editor = Editor::new();
        let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
        let offset = DVec3::X * 0.2;
        let pivot = DVec3::ZERO;
        for placement in [
            TextPlacement::Screen { position: DVec2::new(0.2, 0.3) },
            TextPlacement::Attached {
                anchor: TextAnchor { point: DVec3::ZERO, reference: None },
                leader: TextLeader::None { offset: DVec3::Y },
            },
        ] {
            let original = note(placement);
            let id = editor.apply(Command::AddText { text: original.clone() }).unwrap().texts[0];
            let before = editor.doc().clone();
            assert!(
                editor
                    .apply(Command::CopyWithText {
                        elements: vec![Element::Text(id), Element::Text(crate::TextId(u64::MAX))],
                        offset,
                        copies: 1,
                        pivot,
                        view
                    })
                    .is_err()
            );
            assert_eq!(editor.doc(), &before);
            let out = editor
                .apply(Command::CopyWithText { elements: vec![Element::Text(id)], offset, copies: 1, pivot, view })
                .unwrap();
            let copied = &editor.active().texts[&out.texts[0]];
            let expected = match original.placement {
                TextPlacement::Screen { .. } => {
                    original.layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap().position
                        + view.project(offset).unwrap()
                        - view.project(pivot).unwrap()
                }
                _ => view.project(DVec3::Y + offset).unwrap(),
            };
            assert!(
                copied.layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap().position.distance(expected) < 1e-8
            );
            assert_eq!(editor.active().texts[&id], original);
            editor.undo();
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn rotate_text_alone_detaches_anchor_and_together_preserves_geometry_links() {
        for together in [false, true] {
            let mut editor = Editor::new();
            editor.apply(Command::DrawLines { lines: vec![[DVec3::X, DVec3::X * 2.]], facing: None }).unwrap();
            let edge = editor.mesh().edges().next().unwrap().0;
            let anchor = TextAnchor {
                point: DVec3::X * 1.5,
                reference: Some(TextReference::Point {
                    reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
                }),
            };
            let text = note(TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset: DVec3::Y } });
            let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
            let before = editor.doc().clone();
            let mut elements = vec![Element::Text(id)];
            if together {
                elements.push(Element::Edge(edge));
            }
            let matrix = DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2);
            let preview = editor.preview_text_transform(&elements, matrix).unwrap();
            assert_eq!(editor.doc(), &before);
            editor
                .apply(Command::Rotate { elements, centre: DVec3::ZERO, axis: DVec3::Z, angle: 90., copies: 0 })
                .unwrap();
            let transformed = &editor.active().texts[&id];
            let TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset } } = &transformed.placement
            else {
                panic!()
            };
            assert_eq!(anchor.resolve(editor.doc(), None).1, together);
            assert!(anchor.resolve(editor.doc(), None).0.distance(DVec3::Y * 1.5) < 1e-8);
            assert!(offset.distance(-DVec3::X) < 1e-8);
            let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
            assert!(
                preview[0]
                    .layout(editor.doc(), None, DMat4::IDENTITY, &view)
                    .unwrap()
                    .position
                    .distance(transformed.layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap().position)
                    < 1e-8
            );
            assert_eq!(transformed.font, text.font);
            editor.undo();
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn rotated_text_arrays_retarget_copied_edges_and_detach_uncopied_ones() {
        for together in [false, true] {
            let mut editor = Editor::new();
            editor.apply(Command::DrawLines { lines: vec![[DVec3::X, DVec3::X * 2.]], facing: None }).unwrap();
            let edge = editor.mesh().edges().next().unwrap().0;
            let text = note(TextPlacement::Attached {
                anchor: TextAnchor {
                    point: DVec3::X * 1.5,
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
                    }),
                },
                leader: TextLeader::Pushpin { offset: DVec3::Y },
            });
            let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
            let before = editor.doc().clone();
            let mut elements = vec![Element::Text(id)];
            if together {
                elements.push(Element::Edge(edge));
            }
            let preview = editor.preview_text_rotation(&elements, DVec3::ZERO, DVec3::Z, 90., true).unwrap();
            assert_eq!(editor.doc(), &before);
            let out = editor
                .apply(Command::Rotate { elements, centre: DVec3::ZERO, axis: DVec3::Z, angle: 90., copies: 2 })
                .unwrap();
            let ids = out
                .created
                .iter()
                .filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None })
                .collect::<Vec<_>>();
            assert_eq!(ids.len(), 2);
            assert_eq!(editor.active().texts[&id], text);
            for (index, id) in ids.iter().enumerate() {
                let copied = &editor.active().texts[id];
                let TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset } } = &copied.placement
                else {
                    panic!()
                };
                let matrix = DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2 * (index + 1) as f64);
                assert_eq!(anchor.resolve(editor.doc(), None).1, together);
                assert!(anchor.resolve(editor.doc(), None).0.distance(matrix.transform_point3(DVec3::X * 1.5)) < 1e-8);
                assert!(offset.distance(matrix.transform_vector3(DVec3::Y)) < 1e-8);
            }
            let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
            assert!(
                preview[0].layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap().position.distance(
                    editor.active().texts[&ids[0]].layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap().position
                ) < 1e-8
            );
            editor.undo();
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn scaling_text_preserves_fonts_and_only_keeps_participating_links() {
        for together in [false, true] {
            let mut editor = Editor::new();
            editor.apply(Command::DrawLines { lines: vec![[DVec3::X, DVec3::X * 2.]], facing: None }).unwrap();
            let edge = editor.mesh().edges().next().unwrap().0;
            let text = note(TextPlacement::Attached {
                anchor: TextAnchor {
                    point: DVec3::X * 1.5,
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } },
                    }),
                },
                leader: TextLeader::Pushpin { offset: DVec3::Y },
            });
            let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
            let before = editor.doc().clone();
            let factors = DVec3::new(-2., 0.5, 3.);
            let matrix = DMat4::from_scale(factors);
            let mut elements = vec![Element::Text(id)];
            if together {
                elements.push(Element::Edge(edge));
            }
            let preview = editor.preview_text_transform(&elements, matrix).unwrap();
            assert_eq!(editor.doc(), &before);
            editor.apply(Command::Scale { elements, origin: DVec3::ZERO, factors }).unwrap();
            let scaled = &editor.active().texts[&id];
            let TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset } } = &scaled.placement else {
                panic!()
            };
            assert_eq!(anchor.resolve(editor.doc(), None).1, together);
            assert!(anchor.resolve(editor.doc(), None).0.distance(-DVec3::X * 3.) < 1e-8);
            assert_eq!(*offset, DVec3::Y * 0.5);
            assert_eq!(scaled.font, text.font);
            let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1200., 800.));
            assert!(
                preview[0]
                    .layout(editor.doc(), None, DMat4::IDENTITY, &view)
                    .unwrap()
                    .position
                    .distance(scaled.layout(editor.doc(), None, DMat4::IDENTITY, &view).unwrap().position)
                    < 1e-8
            );
            editor.undo();
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn screen_scale_is_noop_and_view_based_scale_keeps_pixel_offset() {
        let mut editor = Editor::new();
        let screen = note(TextPlacement::Screen { position: DVec2::new(0.2, 0.3) });
        let id = editor.apply(Command::AddText { text: screen.clone() }).unwrap().texts[0];
        let revision = editor.revision();
        editor
            .apply(Command::Scale { elements: vec![Element::Text(id)], origin: DVec3::ZERO, factors: DVec3::splat(2.) })
            .unwrap();
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.active().texts[&id], screen);
        let text = note(TextPlacement::Attached {
            anchor: TextAnchor { point: DVec3::X, reference: None },
            leader: TextLeader::ViewBased { offset: DVec2::new(80., -40.) },
        });
        let id = editor.apply(Command::AddText { text: text.clone() }).unwrap().texts[0];
        editor
            .apply(Command::ScaleInAxes {
                elements: vec![Element::Text(id)],
                origin: DVec3::ZERO,
                factors: DVec3::new(2., 0.5, 3.),
                x: DVec3::Y,
                y: -DVec3::X,
            })
            .unwrap();
        let scaled = &editor.active().texts[&id];
        assert_eq!(scaled.font, text.font);
        let TextPlacement::Attached { anchor, leader: TextLeader::ViewBased { offset } } = &scaled.placement else {
            panic!()
        };
        assert!(anchor.point.distance(DVec3::X * 0.5) < 1e-8);
        assert_eq!(*offset, DVec2::new(80., -40.));
    }
}
