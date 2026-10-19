//! Geometry associations and measurements for dimension entities.
use crate::{Document, Units};
use rysmo_math::{DMat4, DVec3, EPS};
use rysmo_mesh::{CurveId, CurveKind, DefinitionId, EdgeId, InstanceId, VertexId};
use serde::{Deserialize, Serialize};

/// An association is relative to the definition that owns the dimension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointReference {
    #[serde(default)]
    pub path: Vec<InstanceId>,
    pub entity: PointEntity,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointEntity {
    Vertex { vertex: VertexId },
    Edge { edge: EdgeId, parameter: f64 },
    CurveCentre { curve: CurveId },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimensionAnchor {
    /// Retained when the referenced entity is erased or becomes unavailable.
    pub point: DVec3,
    pub reference: Option<PointReference>,
}

pub(crate) fn context(
    doc: &Document,
    owner: Option<DefinitionId>,
    path: &[InstanceId],
) -> Option<(Option<DefinitionId>, DMat4)> {
    let mut definition = owner;
    let mut transform = DMat4::IDENTITY;
    for id in path {
        let instance = doc.def(definition)?.instances.get(id)?;
        transform *= instance.transform;
        definition = Some(instance.definition);
    }
    Some((definition, transform))
}

fn mapped_curve(
    source: &rysmo_mesh::Mesh,
    target: &rysmo_mesh::Mesh,
    transform: DMat4,
    id: CurveId,
) -> Option<CurveId> {
    source.curve_edges(id).into_iter().find_map(|edge| {
        let (a, b) = source.edge_points(edge);
        let a = target.find_vertex(transform.transform_point3(a))?;
        let b = target.find_vertex(transform.transform_point3(b))?;
        target.edge(target.edge_between(a, b)?)?.curve
    })
}
pub(crate) fn mapped_entity(
    source: &rysmo_mesh::Mesh,
    target: &rysmo_mesh::Mesh,
    transform: DMat4,
    entity: PointEntity,
) -> Option<PointEntity> {
    match entity {
        PointEntity::Vertex { vertex } => Some(PointEntity::Vertex {
            vertex: target.find_vertex(transform.transform_point3(source.vertex(vertex)?.pos))?,
        }),
        PointEntity::Edge { edge, parameter } => {
            source.edge(edge)?;
            let (a, b) = source.edge_points(edge);
            let point = transform.transform_point3(a.lerp(b, parameter));
            if let Some(vertex) = target.find_vertex(point) {
                return Some(PointEntity::Vertex { vertex });
            }
            target.edges().find_map(|(edge, _)| {
                let (a, b) = target.edge_points(edge);
                let delta = b - a;
                let parameter = (point - a).dot(delta) / delta.length_squared();
                (parameter >= 0.0 && parameter <= 1.0 && point.distance(a + delta * parameter) < EPS)
                    .then_some(PointEntity::Edge { edge, parameter })
            })
        }
        PointEntity::CurveCentre { curve } => {
            Some(PointEntity::CurveCentre { curve: mapped_curve(source, target, transform, curve)? })
        }
    }
}

impl DimensionAnchor {
    pub fn validate(&self) -> bool {
        self.point.is_finite()
            && self.reference.as_ref().is_none_or(|reference| match reference.entity {
                PointEntity::Edge { parameter, .. } => parameter.is_finite() && (0.0..=1.0).contains(&parameter),
                _ => true,
            })
    }

    /// Returns owner-local position and whether it still follows live geometry.
    pub fn resolve(&self, doc: &Document, owner: Option<DefinitionId>) -> (DVec3, bool) {
        let live = (|| {
            let reference = self.reference.as_ref()?;
            let (definition, transform) = context(doc, owner, &reference.path)?;
            let mesh = &doc.def(definition)?.mesh;
            let point = match reference.entity {
                PointEntity::Vertex { vertex } => mesh.vertex(vertex)?.pos,
                PointEntity::Edge { edge, parameter } => {
                    if !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
                        return None;
                    }
                    mesh.edge(edge)?;
                    let (a, b) = mesh.edge_points(edge);
                    a.lerp(b, parameter)
                }
                PointEntity::CurveCentre { curve } => {
                    let curve = mesh.curve(curve)?;
                    if curve.kind == CurveKind::Polyline {
                        return None;
                    }
                    curve.centre
                }
            };
            let point = transform.transform_point3(point);
            point.is_finite().then_some(point)
        })();
        live.map(|p| (p, true)).unwrap_or((self.point, false))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadialReference {
    #[serde(default)]
    pub path: Vec<InstanceId>,
    #[serde(default)]
    pub curve: Option<CurveId>,
    pub centre: DVec3,
    pub normal: DVec3,
    pub start: DVec3,
    pub radius: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_sweep: Option<f64>,
    /// Retained affine image of the original circular frame, in owner coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_axes: Option<[DVec3; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DimensionMeasure {
    Linear { from: DimensionAnchor, to: DimensionAnchor, axis: Option<DVec3> },
    Radial { reference: RadialReference, diameter: bool },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionTextPosition {
    OutsideStart,
    #[default]
    Centered,
    OutsideEnd,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionEndpoints {
    None,
    Slash,
    Dot,
    #[default]
    ClosedArrow,
    OpenArrow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DimensionEndMarker {
    Line([rysmo_math::DVec2; 2]),
    Triangle([rysmo_math::DVec2; 3]),
    Dot { centre: rysmo_math::DVec2, radius: f64 },
}
impl DimensionEndMarker {
    pub fn distance(&self, point: rysmo_math::DVec2) -> f64 {
        use rysmo_math::distance_point_segment_2d;
        match self {
            Self::Line([a, b]) => distance_point_segment_2d(point, *a, *b),
            Self::Dot { centre, radius } => (point.distance(*centre) - radius).max(0.),
            Self::Triangle(triangle) => {
                let signs = (0..3)
                    .map(|i| (triangle[(i + 1) % 3] - triangle[i]).perp_dot(point - triangle[i]))
                    .collect::<Vec<_>>();
                if signs.iter().all(|s| *s >= 0.) || signs.iter().all(|s| *s <= 0.) {
                    0.
                } else {
                    (0..3)
                        .map(|i| distance_point_segment_2d(point, triangle[i], triangle[(i + 1) % 3]))
                        .fold(f64::INFINITY, f64::min)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionTextAlignment {
    #[default]
    Screen,
    DimensionLine,
}

/// Oriented screen rectangle shared by text drawing and selection.
#[derive(Clone, Copy, Debug)]
pub struct DimensionTextRect {
    pub centre: rysmo_math::DVec2,
    pub axis: rysmo_math::DVec2,
    pub size: rysmo_math::DVec2,
}
impl DimensionTextRect {
    pub fn corners(&self) -> [rysmo_math::DVec2; 4] {
        let along = self.axis * self.size.x * 0.5;
        let side = rysmo_math::DVec2::new(-self.axis.y, self.axis.x) * self.size.y * 0.5;
        [self.centre - along - side, self.centre + along - side, self.centre + along + side, self.centre - along + side]
    }
    pub fn bounds(&self) -> (rysmo_math::DVec2, rysmo_math::DVec2) {
        self.corners().into_iter().fold(
            (rysmo_math::DVec2::splat(f64::INFINITY), rysmo_math::DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        )
    }
    pub fn expanded(self, padding: f64) -> Self {
        Self { size: self.size + rysmo_math::DVec2::splat(padding * 2.), ..self }
    }
    pub fn distance(&self, point: rysmo_math::DVec2) -> f64 {
        let delta = point - self.centre;
        let local =
            rysmo_math::DVec2::new(delta.dot(self.axis), delta.dot(rysmo_math::DVec2::new(-self.axis.y, self.axis.x)));
        local.distance(local.clamp(-self.size * 0.5, self.size * 0.5))
    }
    pub fn crosses(&self, lo: rysmo_math::DVec2, hi: rysmo_math::DVec2) -> bool {
        let rect = [lo, rysmo_math::DVec2::new(hi.x, lo.y), hi, rysmo_math::DVec2::new(lo.x, hi.y)];
        let corners = self.corners();
        [rysmo_math::DVec2::X, rysmo_math::DVec2::Y, self.axis, rysmo_math::DVec2::new(-self.axis.y, self.axis.x)]
            .into_iter()
            .all(|axis| {
                let interval = |points: &[rysmo_math::DVec2; 4]| {
                    points
                        .iter()
                        .map(|p| p.dot(axis))
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
                };
                let (a, b) = interval(&rect);
                let (c, d) = interval(&corners);
                a <= d && c <= b
            })
    }
}

/// Text styling uses logical screen pixels, independent of model scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionFont {
    pub size: f32,
    pub monospace: bool,
    pub color: [u8; 3],
}
impl Default for DimensionFont {
    fn default() -> Self {
        Self { size: 14., monospace: false, color: [30, 30, 30] }
    }
}
pub type DimensionTextKey = (String, u32, bool);
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionDefaults {
    pub font: DimensionFont,
    pub alignment: DimensionTextAlignment,
    pub endpoints: DimensionEndpoints,
}
impl DimensionDefaults {
    pub fn validate(&self) -> bool {
        self.font.size.is_finite() && (1.0..=512.0).contains(&self.font.size)
    }
    pub fn apply(&self, dimension: &mut Dimension) {
        dimension.font = self.font;
        dimension.alignment = self.alignment;
        dimension.endpoints = self.endpoints;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub measure: DimensionMeasure,
    /// Fixed placement plane, in the owning definition's coordinates.
    pub normal: DVec3,
    pub offset: DVec3,
    /// Empty uses the automatic value. `<>` inserts the current measurement.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub alignment: DimensionTextAlignment,
    #[serde(default)]
    pub font: DimensionFont,
    #[serde(default)]
    pub text_position: DimensionTextPosition,
    #[serde(default)]
    pub endpoints: DimensionEndpoints,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedDimension {
    pub from: DVec3,
    pub to: DVec3,
    pub length: f64,
    pub normal: DVec3,
    pub associated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionVisibility {
    pub hide_foreshortened: bool,
    pub angle_degrees: f64,
    pub hide_small: bool,
    pub minimum_pixels: f64,
    pub highlight_non_associated: bool,
    pub highlight_color: [u8; 3],
}
impl Default for DimensionVisibility {
    fn default() -> Self {
        Self {
            hide_foreshortened: false,
            angle_degrees: 20.,
            hide_small: false,
            minimum_pixels: 40.,
            highlight_non_associated: false,
            highlight_color: [220, 40, 40],
        }
    }
}
impl DimensionVisibility {
    pub fn validate(&self) -> bool {
        self.angle_degrees.is_finite()
            && (0.0..=90.0).contains(&self.angle_degrees)
            && self.minimum_pixels.is_finite()
            && (0.0..=4096.0).contains(&self.minimum_pixels)
    }
}

/// World-space drawing geometry shared by the live preview and committed entities.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionLayout {
    pub segments: Vec<[DVec3; 2]>,
    /// Arrow tip and the direction toward the inside of its dimension line.
    pub arrows: Vec<(DVec3, DVec3)>,
    pub endpoints: DimensionEndpoints,
    pub text_position: DVec3,
    pub text_direction: DVec3,
    pub measurement: ResolvedDimension,
}

impl DimensionLayout {
    /// Visibility is evaluated from the actual dimension string in the current view.
    pub fn visible(&self, view: &rysmo_math::View, settings: DimensionVisibility) -> bool {
        if !settings.hide_small && !settings.hide_foreshortened {
            return true;
        }
        let Some([a, b]) = self.segments.last().copied() else {
            return false;
        };
        if settings.hide_small {
            let Some((a, b)) = view.project_segment(a, b) else {
                return false;
            };
            if a.distance(b) < settings.minimum_pixels {
                return false;
            }
        }
        if settings.hide_foreshortened {
            let Some(at) = view.project((a + b) * 0.5) else {
                return false;
            };
            let direction = (b - a).normalize_or_zero();
            if direction.length_squared() == 0. {
                return false;
            }
            let angle = direction.dot(view.ray(at).dir).abs().clamp(0., 1.).acos().to_degrees();
            if angle < settings.angle_degrees {
                return false;
            }
        }
        true
    }

    pub fn text_on_screen(&self, view: &rysmo_math::View) -> Option<rysmo_math::DVec2> {
        (view.view.transform_point3(self.text_position).z <= -1e-4).then(|| view.project(self.text_position)).flatten()
    }
    /// Screen arrow geometry is shared by drawing and hit testing.
    pub fn screen_arrows(&self, view: &rysmo_math::View, length: f64, width: f64) -> Vec<[rysmo_math::DVec2; 3]> {
        if !length.is_finite() || !width.is_finite() || length <= 0.0 || width <= 0.0 {
            return Vec::new();
        }
        self.arrows
            .iter()
            .filter_map(|(tip, direction)| {
                if view.view.transform_point3(*tip).z > -1e-4 {
                    return None;
                }
                let a = view.project(*tip)?;
                let b = view.project(*tip + *direction * view.pixel_size_at(*tip) * 8.0)?;
                let delta = b - a;
                if delta.length_squared() < 0.01 {
                    return None;
                }
                let direction = delta.normalize();
                let side = rysmo_math::DVec2::new(-direction.y, direction.x);
                Some([a, a + direction * length + side * width, a + direction * length - side * width])
            })
            .collect()
    }

    pub fn screen_endpoints(&self, view: &rysmo_math::View) -> Vec<DimensionEndMarker> {
        use DimensionEndMarker::*;
        if self.endpoints == DimensionEndpoints::None {
            return Vec::new();
        }
        if self.endpoints == DimensionEndpoints::Dot {
            return self
                .arrows
                .iter()
                .filter_map(|(tip, _)| {
                    if view.view.transform_point3(*tip).z > -1e-4 {
                        return None;
                    }
                    Some(Dot { centre: view.project(*tip)?, radius: 2.5 })
                })
                .collect();
        }
        self.screen_arrows(view, 7., 2.5)
            .into_iter()
            .flat_map(|[tip, a, b]| match self.endpoints {
                DimensionEndpoints::ClosedArrow => vec![Triangle([tip, a, b])],
                DimensionEndpoints::OpenArrow => vec![Line([tip, a]), Line([tip, b])],
                DimensionEndpoints::Slash => {
                    let along = ((a + b) * 0.5 - tip).normalize();
                    let side = (a - b).normalize();
                    let delta = (along + side).normalize() * 4.;
                    vec![Line([tip - delta, tip + delta])]
                }
                _ => Vec::new(),
            })
            .collect()
    }

    pub fn box_hit(
        &self,
        view: &rysmo_math::View,
        lo: rysmo_math::DVec2,
        hi: rysmo_math::DVec2,
        crossing: bool,
        text_bounds: Option<(rysmo_math::DVec2, rysmo_math::DVec2)>,
    ) -> bool {
        let inside = |p: rysmo_math::DVec2| p.cmpge(lo).all() && p.cmple(hi).all();
        let segment = |a: rysmo_math::DVec2, b: rysmo_math::DVec2| {
            let delta = b - a;
            let mut enter: f64 = 0.;
            let mut leave: f64 = 1.;
            for axis in 0..2 {
                if delta[axis].abs() < 1e-12 {
                    if a[axis] < lo[axis] || a[axis] > hi[axis] {
                        return false;
                    }
                } else {
                    let x = (lo[axis] - a[axis]) / delta[axis];
                    let y = (hi[axis] - a[axis]) / delta[axis];
                    enter = enter.max(x.min(y));
                    leave = leave.min(x.max(y));
                    if enter > leave {
                        return false;
                    }
                }
            }
            true
        };
        let markers = self.screen_endpoints(view);
        let text = self.text_on_screen(view).and(text_bounds);
        if crossing {
            self.segments.iter().filter_map(|[a, b]| view.project_segment(*a, *b)).any(|(a, b)| segment(a, b))
                || markers.iter().any(|marker| match marker {
                    DimensionEndMarker::Line([a, b]) => segment(*a, *b),
                    DimensionEndMarker::Triangle(triangle) => {
                        (0..3).any(|i| segment(triangle[i], triangle[(i + 1) % 3]))
                            || marker.distance((lo + hi) * 0.5) == 0.
                    }
                    DimensionEndMarker::Dot { centre, radius } => centre.distance(centre.clamp(lo, hi)) <= *radius,
                })
                || text.is_some_and(|(a, b)| a.min(b).cmple(hi).all() && a.max(b).cmpge(lo).all())
        } else {
            !self.segments.is_empty()
                && self.segments.iter().all(|[a, b]| {
                    view.view.transform_point3(*a).z <= -1e-4
                        && view.view.transform_point3(*b).z <= -1e-4
                        && view.project(*a).zip(view.project(*b)).is_some_and(|(a, b)| inside(a) && inside(b))
                })
                && markers.iter().all(|marker| match marker {
                    DimensionEndMarker::Line(points) => points.iter().all(|p| inside(*p)),
                    DimensionEndMarker::Triangle(points) => points.iter().all(|p| inside(*p)),
                    DimensionEndMarker::Dot { centre, radius } => {
                        inside(*centre - rysmo_math::DVec2::splat(*radius))
                            && inside(*centre + rysmo_math::DVec2::splat(*radius))
                    }
                })
                && text.is_some_and(|(a, b)| inside(a) && inside(b))
        }
    }

    /// The caller supplies the actual text rectangle, measured by its font engine.
    /// Distances are in screen pixels. Invisible geometry cannot be picked.
    pub fn hit_distance(
        &self,
        view: &rysmo_math::View,
        cursor: rysmo_math::DVec2,
        text_bounds: Option<(rysmo_math::DVec2, rysmo_math::DVec2)>,
    ) -> Option<f64> {
        if !cursor.is_finite() {
            return None;
        }
        let mut distances: Vec<f64> = self
            .segments
            .iter()
            .filter_map(|[a, b]| {
                let (a, b) = view.project_segment(*a, *b)?;
                Some(rysmo_math::distance_point_segment_2d(cursor, a, b))
            })
            .collect();
        distances.extend(self.screen_endpoints(view).iter().map(|marker| marker.distance(cursor)));
        if let Some((a, b)) = text_bounds
            && self.text_on_screen(view).is_some()
            && a.is_finite()
            && b.is_finite()
        {
            let lo = a.min(b);
            let hi = a.max(b);
            distances.push(cursor.distance(cursor.clamp(lo, hi)));
        }
        distances.into_iter().filter(|d| d.is_finite()).min_by(f64::total_cmp)
    }
}

impl Dimension {
    pub fn text_key(&self, label: String) -> DimensionTextKey {
        (label, self.font.size.to_bits(), self.font.monospace)
    }
    pub fn text_rect(
        &self,
        layout: &DimensionLayout,
        view: &rysmo_math::View,
        size: rysmo_math::DVec2,
    ) -> Option<DimensionTextRect> {
        if !size.is_finite() || size.min_element() < 0. {
            return None;
        }
        let mut centre = layout.text_on_screen(view)?;
        let projected = view
            .project(layout.text_position + layout.text_direction * view.pixel_size_at(layout.text_position) * 20.)
            .map(|end| (end - centre).normalize_or_zero())
            .unwrap_or(rysmo_math::DVec2::ZERO);
        let mut axis = if self.alignment == DimensionTextAlignment::DimensionLine && projected.length_squared() > 0. {
            projected
        } else {
            rysmo_math::DVec2::X
        };
        // Keep text upright as the camera moves around the model.
        if axis.x < 0. {
            axis = -axis;
        }
        if matches!(self.measure, DimensionMeasure::Linear { .. })
            && self.text_position != DimensionTextPosition::Centered
        {
            let side = rysmo_math::DVec2::new(-axis.y, axis.x);
            let extent = (projected.dot(axis).abs() * size.x + projected.dot(side).abs() * size.y) * 0.5 + 4.;
            centre +=
                projected * extent * if self.text_position == DimensionTextPosition::OutsideStart { -1. } else { 1. };
        }
        Some(DimensionTextRect { centre, axis, size })
    }
    pub fn text_bounds(
        &self,
        layout: &DimensionLayout,
        view: &rysmo_math::View,
        size: rysmo_math::DVec2,
    ) -> Option<(rysmo_math::DVec2, rysmo_math::DVec2)> {
        self.text_rect(layout, view, size).map(|r| r.bounds())
    }
    pub fn hit_distance(
        &self,
        layout: &DimensionLayout,
        view: &rysmo_math::View,
        cursor: rysmo_math::DVec2,
        size: Option<rysmo_math::DVec2>,
    ) -> Option<f64> {
        let geometry = layout.hit_distance(view, cursor, None);
        let text = size.and_then(|size| self.text_rect(layout, view, size)).map(|r| r.expanded(2.).distance(cursor));
        geometry.into_iter().chain(text).filter(|d| d.is_finite()).min_by(f64::total_cmp)
    }
    pub fn box_hit(
        &self,
        layout: &DimensionLayout,
        view: &rysmo_math::View,
        lo: rysmo_math::DVec2,
        hi: rysmo_math::DVec2,
        crossing: bool,
        size: Option<rysmo_math::DVec2>,
    ) -> bool {
        let text = size.and_then(|size| self.text_rect(layout, view, size)).map(|r| r.expanded(2.));
        if crossing {
            layout.box_hit(view, lo, hi, true, None) || text.is_some_and(|r| r.crosses(lo, hi))
        } else {
            layout.box_hit(view, lo, hi, false, text.map(|r| r.bounds()))
        }
    }

    pub fn validate(&self) -> bool {
        if !self.font.size.is_finite() || !(1.0..=512.0).contains(&self.font.size) {
            return false;
        }
        if !self.normal.is_finite()
            || self.normal.length() < EPS
            || !self.offset.is_finite()
            || self.normal.normalize().dot(self.offset).abs() > EPS
        {
            return false;
        }
        match &self.measure {
            DimensionMeasure::Linear { from, to, axis } => {
                from.validate()
                    && to.validate()
                    && axis.is_none_or(|axis| {
                        axis.is_finite()
                            && axis.length() > EPS
                            && axis.normalize().dot(self.normal.normalize()).abs() < 1e-6
                    })
            }
            DimensionMeasure::Radial { reference, .. } => {
                reference.centre.is_finite()
                    && reference.normal.is_finite()
                    && reference.start.is_finite()
                    && reference.radius.is_finite()
                    && reference.radius > EPS
                    && reference.radial_axes.is_none_or(|axes| {
                        let cross = axes[0].cross(axes[1]);
                        axes.iter().all(|a| a.is_finite() && a.length().is_finite() && a.length() > EPS)
                            && cross.length().is_finite()
                            && cross.length() > EPS * EPS
                            && cross.normalize().dot(reference.normal.normalize()) > 1. - 1e-6
                            && axes[0].normalize().dot(reference.start.normalize()) > 1. - 1e-6
                            && (axes[0].length() - reference.radius).abs() < EPS * reference.radius.max(1.)
                    })
                    && reference.arc_sweep.is_none_or(|s| s.is_finite() && s.abs() > 1e-9 && s.abs() <= 360.0 + 1e-6)
                    && reference.normal.length() > EPS
                    && reference.start.length() > EPS
                    && reference.normal.normalize().dot(reference.start.normalize()).abs() < 1e-6
                    && reference.normal.normalize().dot(self.normal.normalize()).abs() > 1.0 - 1e-6
            }
        }
    }

    pub fn resolve(&self, doc: &Document, owner: Option<DefinitionId>) -> ResolvedDimension {
        match &self.measure {
            DimensionMeasure::Linear { from, to, axis } => {
                let (from, a) = from.resolve(doc, owner);
                let (to, b) = to.resolve(doc, owner);
                let delta = to - from;
                let length = axis.map(|axis| delta.dot(axis.normalize()).abs()).unwrap_or(delta.length());
                ResolvedDimension { from, to, length, normal: self.normal.normalize(), associated: a && b }
            }
            DimensionMeasure::Radial { reference, diameter } => {
                let live = (|| {
                    let (definition, transform) = context(doc, owner, &reference.path)?;
                    let curve = doc.def(definition)?.mesh.curve(reference.curve?)?;
                    if curve.kind == CurveKind::Polyline {
                        return None;
                    }
                    let u = curve.start.normalize() * curve.radius;
                    let v = curve.normal.normalize().cross(curve.start.normalize()) * curve.radius;
                    let a = transform.transform_vector3(u);
                    let b = transform.transform_vector3(v);
                    // A nonuniformly transformed circle has no single radius.
                    if !a.is_finite()
                        || !b.is_finite()
                        || a.length() < EPS
                        || b.length() < EPS
                        || (a.length() - b.length()).abs() > EPS * a.length().max(1.0)
                        || a.normalize().dot(b.normalize()).abs() > 1e-6
                    {
                        return None;
                    }
                    Some((transform.transform_point3(curve.centre), a, a.cross(b).normalize()))
                })();
                let associated = live.is_some();
                let (centre, radial, normal) = live.unwrap_or((
                    reference.centre,
                    reference.start.normalize() * reference.radius,
                    reference.normal.normalize(),
                ));
                let from = if *diameter { centre - radial } else { centre };
                let to = centre + radial;
                ResolvedDimension {
                    from,
                    to,
                    length: radial.length() * if *diameter { 2.0 } else { 1.0 },
                    normal,
                    associated,
                }
            }
        }
    }

    pub(crate) fn refreshed(&self, doc: &Document, owner: Option<DefinitionId>) -> Self {
        let mut result = self.clone();
        match &mut result.measure {
            DimensionMeasure::Linear { from, to, .. } => {
                let (a, attached) = from.resolve(doc, owner);
                if attached {
                    from.point = a;
                }
                let (b, attached) = to.resolve(doc, owner);
                if attached {
                    to.point = b;
                }
            }
            DimensionMeasure::Radial { reference, diameter } => {
                let resolved = self.resolve(doc, owner);
                if resolved.associated {
                    let centre = if *diameter { (resolved.from + resolved.to) * 0.5 } else { resolved.from };
                    let start = (resolved.to - centre).normalize();
                    let old_u = reference.start.normalize();
                    let old_v = reference.normal.normalize().cross(old_u);
                    result.offset =
                        start * result.offset.dot(old_u) + resolved.normal.cross(start) * result.offset.dot(old_v);
                    result.normal = resolved.normal;
                    reference.centre = centre;
                    reference.start = start;
                    reference.normal = resolved.normal;
                    reference.radius = resolved.length / if *diameter { 2.0 } else { 1.0 };
                    reference.radial_axes = None;
                    reference.arc_sweep = reference.curve.and_then(|id| {
                        let (definition, _) = context(doc, owner, &reference.path)?;
                        let curve = doc.def(definition)?.mesh.curve(id)?;
                        (curve.kind == CurveKind::Arc).then_some(curve.sweep)
                    });
                }
            }
        }
        result
    }

    pub(crate) fn remapped(
        &self,
        source: &rysmo_mesh::Mesh,
        target: &rysmo_mesh::Mesh,
        transform: DMat4,
        instances: &std::collections::BTreeMap<InstanceId, InstanceId>,
    ) -> Self {
        let mut result = self.clone().mapped(transform);
        let remap = |reference: &mut Option<PointReference>| {
            if let Some(r) = reference {
                if let Some(first) = r.path.first_mut() {
                    if let Some(id) = instances.get(first) {
                        *first = *id;
                    } else {
                        *reference = None;
                    }
                } else if let Some(entity) = mapped_entity(source, target, transform, r.entity) {
                    r.entity = entity;
                } else {
                    *reference = None;
                }
            }
        };
        match &mut result.measure {
            DimensionMeasure::Linear { from, to, .. } => {
                remap(&mut from.reference);
                remap(&mut to.reference);
            }
            DimensionMeasure::Radial { reference, .. } => {
                if let Some(first) = reference.path.first_mut() {
                    if let Some(id) = instances.get(first) {
                        *first = *id;
                    } else {
                        reference.curve = None;
                        reference.path.clear();
                    }
                } else {
                    reference.curve = reference.curve.and_then(|curve| mapped_curve(source, target, transform, curve));
                }
            }
        }
        result
    }

    /// Transform annotation geometry, retaining links only to geometry moved with it.
    pub(crate) fn transformed_links(
        &self,
        source: &rysmo_mesh::Mesh,
        transform: DMat4,
        vertices: &std::collections::BTreeSet<VertexId>,
        instances: &[InstanceId],
    ) -> Self {
        let curve_moved = |curve| {
            let edges = source.curve_edges(curve);
            !edges.is_empty()
                && edges.iter().all(|id| source.edge(*id).is_some_and(|e| e.v.iter().all(|v| vertices.contains(v))))
        };
        let mut result = self.clone().mapped(transform);
        let retain = |reference: &mut Option<PointReference>| {
            if let Some(r) = reference {
                let moved = if let Some(first) = r.path.first() {
                    instances.contains(first)
                } else {
                    match r.entity {
                        PointEntity::Vertex { vertex } => vertices.contains(&vertex),
                        PointEntity::Edge { edge, .. } => {
                            source.edge(edge).is_some_and(|e| e.v.iter().all(|v| vertices.contains(v)))
                        }
                        PointEntity::CurveCentre { curve } => curve_moved(curve),
                    }
                };
                if !moved {
                    *reference = None;
                }
            }
        };
        match &mut result.measure {
            DimensionMeasure::Linear { from, to, .. } => {
                retain(&mut from.reference);
                retain(&mut to.reference);
            }
            DimensionMeasure::Radial { reference, .. } => {
                let moved = reference
                    .path
                    .first()
                    .map(|id| instances.contains(id))
                    .unwrap_or_else(|| reference.curve.is_some_and(curve_moved));
                if !moved {
                    reference.curve = None;
                    reference.path.clear();
                }
            }
        }
        result
    }

    /// Retarget only associations whose geometry participates in this copy.
    pub(crate) fn copied_links(
        &self,
        source: &rysmo_mesh::Mesh,
        target: &rysmo_mesh::Mesh,
        transform: DMat4,
        instances: &std::collections::BTreeMap<InstanceId, InstanceId>,
        edges: &std::collections::BTreeSet<EdgeId>,
    ) -> Self {
        let mapped = self.remapped(source, target, transform, instances);
        let copied_curve = |curve| {
            !source.curve_edges(curve).is_empty() && source.curve_edges(curve).iter().all(|e| edges.contains(e))
        };
        let copied_point = |reference: &Option<PointReference>| {
            reference.as_ref().is_some_and(|r| {
                if let Some(first) = r.path.first() {
                    return instances.contains_key(first);
                }
                match r.entity {
                    PointEntity::Vertex { vertex } => {
                        edges.iter().any(|e| source.edge(*e).is_some_and(|e| e.v.contains(&vertex)))
                    }
                    PointEntity::Edge { edge, .. } => edges.contains(&edge),
                    PointEntity::CurveCentre { curve } => copied_curve(curve),
                }
            })
        };
        let transformed = [DVec3::X, DVec3::Y, DVec3::Z]
            .iter()
            .any(|axis| transform.transform_vector3(*axis).distance(*axis) > 1e-10);
        let mut result = if transformed { self.clone().mapped(transform) } else { self.clone() };
        match (&mut result.measure, mapped.measure) {
            (DimensionMeasure::Linear { from, to, .. }, DimensionMeasure::Linear { from: ma, to: mb, .. }) => {
                if copied_point(&from.reference) {
                    *from = ma;
                } else if transformed {
                    from.reference = None;
                }
                if copied_point(&to.reference) {
                    *to = mb;
                } else if transformed {
                    to.reference = None;
                }
            }
            (DimensionMeasure::Radial { reference, .. }, DimensionMeasure::Radial { reference: mapped, .. }) => {
                let copied = reference
                    .path
                    .first()
                    .map(|id| instances.contains_key(id))
                    .unwrap_or_else(|| reference.curve.is_some_and(copied_curve));
                if copied {
                    *reference = mapped;
                } else if transformed {
                    reference.path.clear();
                    reference.curve = None;
                }
            }
            _ => unreachable!(),
        }
        result
    }

    pub(crate) fn remap_instance_reference(
        &mut self,
        instance: InstanceId,
        source: &rysmo_mesh::Mesh,
        target: &rysmo_mesh::Mesh,
        instances: &std::collections::BTreeMap<InstanceId, InstanceId>,
        transform: DMat4,
        remove: bool,
    ) {
        let point = |reference: &mut Option<PointReference>| {
            let Some(r) = reference else { return };
            let Some(index) = r.path.iter().position(|id| *id == instance) else { return };
            if let Some(child) = r.path.get_mut(index + 1) {
                if let Some(id) = instances.get(child) {
                    *child = *id;
                } else {
                    *reference = None;
                    return;
                }
            } else if let Some(entity) = mapped_entity(source, target, transform, r.entity) {
                r.entity = entity;
            } else {
                *reference = None;
                return;
            }
            if remove {
                r.path.remove(index);
            }
        };
        match &mut self.measure {
            DimensionMeasure::Linear { from, to, .. } => {
                point(&mut from.reference);
                point(&mut to.reference);
            }
            DimensionMeasure::Radial { reference, .. } => {
                if let Some(index) = reference.path.iter().position(|id| *id == instance) {
                    if let Some(child) = reference.path.get_mut(index + 1) {
                        if let Some(id) = instances.get(child) {
                            *child = *id;
                        } else {
                            reference.curve = None;
                        }
                    } else {
                        reference.curve = reference.curve.and_then(|id| mapped_curve(source, target, transform, id));
                    }
                    if remove {
                        reference.path.remove(index);
                    }
                }
            }
        }
    }

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
        let path = |path: &mut Vec<InstanceId>| {
            for index in 0..path.len() {
                if children.contains(&path[index])
                    && context(doc, owner, &path[..index]).is_some_and(|(definition, _)| definition == parent)
                {
                    path.insert(index, instance);
                    return false;
                }
            }
            context(doc, owner, path).is_some_and(|(definition, _)| definition == parent)
        };
        let remap = |reference: &mut Option<PointReference>| {
            if let Some(reference) = reference
                && path(&mut reference.path)
            {
                let mesh = &doc.def(parent).unwrap().mesh;
                let retained = match reference.entity {
                    PointEntity::Vertex { vertex } => mesh.vertex(vertex).is_some(),
                    PointEntity::Edge { edge, .. } => mesh.edge(edge).is_some(),
                    PointEntity::CurveCentre { curve } => mesh.curve(curve).is_some(),
                };
                if !retained && let Some(entity) = mapped_entity(source, target, transform, reference.entity) {
                    reference.entity = entity;
                    reference.path.push(instance);
                }
            }
        };
        match &mut result.measure {
            DimensionMeasure::Linear { from, to, .. } => {
                remap(&mut from.reference);
                remap(&mut to.reference);
            }
            DimensionMeasure::Radial { reference, .. } => {
                if path(&mut reference.path)
                    && let Some(curve) = reference.curve
                    && doc.def(parent).unwrap().mesh.curve(curve).is_none()
                    && let Some(curve) = mapped_curve(source, target, transform, curve)
                {
                    reference.curve = Some(curve);
                    reference.path.push(instance);
                }
            }
        }
        result
    }

    /// Retains current values while deliberately removing geometry associations.
    pub fn detached(&self, doc: &Document, owner: Option<DefinitionId>) -> Self {
        let resolved = self.resolve(doc, owner);
        let mut result = self.refreshed(doc, owner);
        result.normal = resolved.normal;
        result.offset -= resolved.normal * result.offset.dot(resolved.normal);
        match &mut result.measure {
            DimensionMeasure::Linear { from, to, .. } => {
                from.point = resolved.from;
                from.reference = None;
                to.point = resolved.to;
                to.reference = None;
            }
            DimensionMeasure::Radial { reference, diameter } => {
                reference.path.clear();
                reference.curve = None;
                reference.centre = if *diameter { (resolved.from + resolved.to) * 0.5 } else { resolved.from };
                reference.radius = resolved.length / if *diameter { 2.0 } else { 1.0 };
                reference.start = (resolved.to - reference.centre).normalize();
                reference.normal = resolved.normal;
            }
        }
        result
    }

    /// Maps placement and fallback values. Linked geometry retains its references.
    pub fn mapped(mut self, transform: DMat4) -> Self {
        let normal = |normal: DVec3| transform.inverse().transpose().transform_vector3(normal).normalize();
        self.normal = normal(self.normal);
        self.offset = transform.transform_vector3(self.offset);
        match &mut self.measure {
            DimensionMeasure::Linear { from, to, axis } => {
                from.point = transform.transform_point3(from.point);
                to.point = transform.transform_point3(to.point);
                if let Some(axis) = axis {
                    *axis = transform.transform_vector3(*axis).normalize();
                }
            }
            DimensionMeasure::Radial { reference, .. } => {
                reference.centre = transform.transform_point3(reference.centre);
                let axes = reference.radial_axes.unwrap_or([
                    reference.start.normalize() * reference.radius,
                    reference.normal.normalize().cross(reference.start.normalize()) * reference.radius,
                ]);
                let radial = transform.transform_vector3(axes[0]);
                let perpendicular = transform.transform_vector3(axes[1]);
                reference.normal = radial.cross(perpendicular).normalize();
                reference.radius = radial.length();
                reference.start = radial.normalize();
                let circular = (radial.length() - perpendicular.length()).abs() < EPS * radial.length().max(1.)
                    && radial.normalize().dot(perpendicular.normalize()).abs() < 1e-6;
                reference.radial_axes = (!circular).then_some([radial, perpendicular]);
            }
        }
        self
    }

    pub fn scaled(mut self, factor: f64) -> Self {
        self.offset *= factor;
        match &mut self.measure {
            DimensionMeasure::Linear { from, to, .. } => {
                from.point *= factor;
                to.point *= factor;
            }
            DimensionMeasure::Radial { reference, .. } => {
                reference.centre *= factor;
                reference.radius *= factor;
                if let Some(axes) = &mut reference.radial_axes {
                    for axis in axes {
                        *axis *= factor;
                    }
                }
            }
        }
        self
    }

    pub fn layout(
        &self,
        doc: &Document,
        owner: Option<DefinitionId>,
        transform: DMat4,
        extension: f64,
    ) -> Option<DimensionLayout> {
        if !self.validate()
            || !transform.is_finite()
            || !transform.inverse().is_finite()
            || !extension.is_finite()
            || extension < 0.0
        {
            return None;
        }
        let mut measurement = self.resolve(doc, owner);
        let mut segments = Vec::new();
        let mut arrows = Vec::new();
        let (from, to, text_position) = match &self.measure {
            DimensionMeasure::Linear { axis, .. } => {
                let delta = measurement.to - measurement.from;
                let direction = axis.map(|axis| axis.normalize()).unwrap_or_else(|| delta.normalize_or_zero());
                let offset = self.offset - direction * self.offset.dot(direction);
                let a = transform.transform_point3(measurement.from + offset);
                let b = transform.transform_point3(measurement.from + offset + direction * delta.dot(direction));
                for (anchor, end) in [(measurement.from, a), (measurement.to, b)] {
                    let anchor = transform.transform_point3(anchor);
                    let leader = (end - anchor).normalize_or_zero();
                    if leader.length_squared() > 0.0 {
                        segments.push([anchor, end + leader * extension]);
                    }
                }
                measurement.length = a.distance(b);
                let text = match self.text_position {
                    DimensionTextPosition::OutsideStart => a,
                    DimensionTextPosition::Centered => (a + b) * 0.5,
                    DimensionTextPosition::OutsideEnd => b,
                };
                (a, b, text)
            }
            DimensionMeasure::Radial { reference, diameter } => {
                let centre = if *diameter { (measurement.from + measurement.to) * 0.5 } else { measurement.from };
                let normal = measurement.normal.normalize();
                let radius = measurement.length / if *diameter { 2.0 } else { 1.0 };
                let offset = self.offset - normal * self.offset.dot(normal);
                let mut direction =
                    if offset.length() > EPS { offset.normalize() } else { (measurement.to - centre).normalize() };
                let live_sweep = if measurement.associated {
                    reference.curve.and_then(|id| {
                        let (definition, _) = context(doc, owner, &reference.path)?;
                        let curve = doc.def(definition)?.mesh.curve(id)?;
                        (curve.kind == CurveKind::Arc).then_some(curve.sweep)
                    })
                } else {
                    None
                };
                if let Some(sweep) = live_sweep.or(reference.arc_sweep) {
                    let start = (measurement.to - centre).normalize();
                    let sweep = sweep.to_radians();
                    let mut angle = normal.dot(start.cross(direction)).atan2(start.dot(direction));
                    if sweep > 0.0 && angle < 0.0 {
                        angle += std::f64::consts::TAU;
                    }
                    if sweep < 0.0 && angle > 0.0 {
                        angle -= std::f64::consts::TAU;
                    }
                    if angle < sweep.min(0.0) - 1e-9 || angle > sweep.max(0.0) + 1e-9 {
                        let end = rysmo_math::DQuat::from_axis_angle(normal, sweep) * start;
                        direction = if start.dot(direction) >= end.dot(direction) { start } else { end };
                    }
                }
                let (radial, perpendicular) = if !measurement.associated
                    && let Some([u, v]) = reference.radial_axes
                {
                    let cross = u.cross(v);
                    let area = cross.length();
                    let frame_normal = cross / area;
                    let x = offset.cross(v).dot(frame_normal) / area;
                    let y = u.cross(offset).dot(frame_normal) / area;
                    let mut angle = if offset.length() > EPS { y.atan2(x) } else { 0. };
                    if let Some(sweep) = reference.arc_sweep {
                        let sweep = sweep.to_radians();
                        if sweep > 0. && angle < 0. {
                            angle += std::f64::consts::TAU;
                        }
                        if sweep < 0. && angle > 0. {
                            angle -= std::f64::consts::TAU;
                        }
                        if angle < sweep.min(0.) - 1e-9 || angle > sweep.max(0.) + 1e-9 {
                            angle = if angle.cos() >= (angle - sweep).cos() { 0. } else { sweep };
                        }
                    }
                    (
                        transform.transform_vector3(u * angle.cos() + v * angle.sin()),
                        transform.transform_vector3(-u * angle.sin() + v * angle.cos()),
                    )
                } else {
                    (
                        transform.transform_vector3(direction * radius),
                        transform.transform_vector3(normal.cross(direction) * radius),
                    )
                };
                if (radial.length() - perpendicular.length()).abs() > EPS * radial.length().max(1.0)
                    || radial.normalize().dot(perpendicular.normalize()).abs() > 1e-6
                {
                    measurement.associated = false;
                }
                let centre = transform.transform_point3(centre);
                let a = if *diameter { centre - radial } else { centre };
                let b = centre + radial;
                let text = if offset.length() > EPS {
                    centre + transform.transform_vector3(offset)
                } else {
                    centre + radial * 1.3
                };
                if b.distance_squared(text) > EPS * EPS {
                    segments.push([b, text]);
                }
                measurement.length = radial.length() * if *diameter { 2.0 } else { 1.0 };
                (a, b, text)
            }
        };
        let direction = (to - from).normalize_or_zero();
        if from.distance_squared(to) > EPS * EPS {
            segments.push([from, to]);
            if !matches!(self.measure, DimensionMeasure::Radial { diameter: false, .. }) {
                arrows.push((from, direction));
            }
            arrows.push((to, -direction));
        }
        measurement.from = transform.transform_point3(measurement.from);
        measurement.to = transform.transform_point3(measurement.to);
        measurement.normal = transform.inverse().transpose().transform_vector3(measurement.normal).normalize();
        Some(DimensionLayout {
            segments,
            arrows,
            endpoints: self.endpoints,
            text_position,
            text_direction: direction,
            measurement,
        })
    }

    pub fn label(&self, resolved: &ResolvedDimension, units: Units) -> String {
        self.label_with_precision(resolved, units, None)
    }

    pub fn label_in_document(&self, resolved: &ResolvedDimension, doc: &Document) -> String {
        self.label_text(doc.format_length(resolved.length))
    }
    pub fn label_with_precision(&self, resolved: &ResolvedDimension, units: Units, precision: Option<u8>) -> String {
        self.label_text(crate::format_length_with_precision(resolved.length, units, precision))
    }

    fn label_text(&self, length: String) -> String {
        let prefix = match &self.measure {
            DimensionMeasure::Radial { diameter: true, .. } => "Ø",
            DimensionMeasure::Radial { diameter: false, .. } => "R",
            _ => "",
        };
        let automatic = format!("{prefix}{length}");
        if self.text.is_empty() { automatic } else { self.text.replace("<>", &automatic) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Element, format_length};
    #[test]
    fn edge_anchor_follows_move_and_reports_deleted_association() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let anchor = DimensionAnchor {
            point: DVec3::X,
            reference: Some(PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.5 } }),
        };
        assert_eq!(anchor.resolve(editor.doc(), None), (DVec3::X, true));
        editor.apply(Command::Move { elements: vec![Element::Edge(edge)], offset: DVec3::Y }).unwrap();
        assert_eq!(anchor.resolve(editor.doc(), None), (DVec3::X + DVec3::Y, true));
        editor.apply(Command::Erase { elements: vec![Element::Edge(edge)] }).unwrap();
        assert_eq!(anchor.resolve(editor.doc(), None), (DVec3::X, false));
    }
    #[test]
    fn measurement_text_updates_and_exact_override_does_not_fake_association() {
        let doc = Document::new();
        let dimension = Dimension {
            measure: DimensionMeasure::Linear {
                from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                to: DimensionAnchor { point: DVec3::X * 2., reference: None },
                axis: None,
            },
            normal: DVec3::Z,
            offset: DVec3::Y,
            text: "Length <>".into(),
            alignment: Default::default(),
            font: Default::default(),
            text_position: Default::default(),
            endpoints: Default::default(),
        };
        assert!(dimension.validate());
        let resolved = dimension.resolve(&doc, None);
        assert_eq!(resolved.length, 2.);
        assert!(!resolved.associated);
        assert_eq!(dimension.label(&resolved, Units::Metres), format!("Length {}", format_length(2., Units::Metres)));
        let mut fraction = resolved.clone();
        fraction.length = 1. / 3.;
        assert_eq!(dimension.label_with_precision(&fraction, Units::Metres, Some(2)), "Length ~ 0.33 m");
        let mut invalid = dimension.clone();
        invalid.offset = DVec3::Z;
        assert!(!invalid.validate());
    }
    #[test]
    fn radial_dimensions_follow_group_scaling_and_detect_ellipses() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawCircle {
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                radius: 1.,
                segments: 12,
                start: Some(DVec3::X),
            })
            .unwrap();
        let elements = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        let out = editor.apply(Command::MakeGroup { elements }).unwrap();
        let Element::Instance(group) = out.created[0] else {
            panic!("group");
        };
        let definition = editor.doc().root.instances[&group].definition;
        let curve = editor.doc().def(Some(definition)).unwrap().mesh.curves().next().unwrap().0;
        let reference = RadialReference {
            radial_axes: None,
            arc_sweep: None,
            path: vec![group],
            curve: Some(curve),
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 1.,
        };
        let dimension = Dimension {
            measure: DimensionMeasure::Radial { reference, diameter: true },
            normal: DVec3::Z,
            offset: DVec3::Y,
            text: String::new(),
            alignment: Default::default(),
            font: Default::default(),
            text_position: Default::default(),
            endpoints: Default::default(),
        };
        assert!(dimension.validate());
        let resolved = dimension.resolve(editor.doc(), None);
        assert!(resolved.associated);
        assert!((resolved.length - 2.).abs() < 1e-8);
        editor
            .apply(Command::Scale {
                elements: vec![Element::Instance(group)],
                origin: DVec3::ZERO,
                factors: DVec3::splat(2.),
            })
            .unwrap();
        let resolved = dimension.resolve(editor.doc(), None);
        assert!(resolved.associated);
        assert!((resolved.length - 4.).abs() < 1e-8);
        editor
            .apply(Command::Scale {
                elements: vec![Element::Instance(group)],
                origin: DVec3::ZERO,
                factors: DVec3::new(2., 1., 1.),
            })
            .unwrap();
        let resolved = dimension.resolve(editor.doc(), None);
        assert!(!resolved.associated);
        assert_eq!(resolved.length, 2.);
    }
    #[test]
    fn point_anchor_resolves_relative_to_its_owner_and_nested_instance() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
        let elements = editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect();
        let out = editor.apply(Command::MakeGroup { elements }).unwrap();
        let Element::Instance(group) = out.created[0] else {
            panic!("group");
        };
        let owner = editor.doc().root.instances[&group].definition;
        let edge = editor.doc().def(Some(owner)).unwrap().mesh.edges().next().unwrap().0;
        let mut anchor = DimensionAnchor {
            point: DVec3::X,
            reference: Some(PointReference { path: vec![group], entity: PointEntity::Edge { edge, parameter: 0.5 } }),
        };
        editor
            .apply(Command::Move { elements: vec![Element::Instance(group)], offset: DVec3::new(2., 3., 4.) })
            .unwrap();
        assert_eq!(anchor.resolve(editor.doc(), None), (DVec3::new(3., 3., 4.), true));
        anchor.reference.as_mut().unwrap().path.clear();
        assert_eq!(anchor.resolve(editor.doc(), Some(owner)), (DVec3::X, true));
        let text = serde_json::to_string(&anchor).unwrap();
        assert_eq!(serde_json::from_str::<DimensionAnchor>(&text).unwrap(), anchor);
    }
    #[test]
    fn linear_layout_projects_axis_and_measures_scaled_occurrence() {
        let doc = Document::new();
        let dimension = Dimension {
            measure: DimensionMeasure::Linear {
                from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                to: DimensionAnchor { point: DVec3::new(2., 1., 0.), reference: None },
                axis: Some(DVec3::X),
            },
            normal: DVec3::Z,
            offset: DVec3::Y * 2.,
            text: String::new(),
            alignment: Default::default(),
            font: Default::default(),
            text_position: Default::default(),
            endpoints: Default::default(),
        };
        let transform = DMat4::from_translation(DVec3::new(3., 4., 0.)) * DMat4::from_scale(DVec3::splat(2.));
        let layout = dimension.layout(&doc, None, transform, 0.1).unwrap();
        assert_eq!(layout.measurement.length, 4.);
        assert_eq!(layout.text_position, DVec3::new(5., 8., 0.));
        assert_eq!(layout.arrows.len(), 2);
        assert_eq!(layout.segments.len(), 3);
        assert!(layout.segments.iter().flatten().all(|point| point.is_finite()));
        assert!(dimension.layout(&doc, None, DMat4::ZERO, 0.1).is_none());
    }
    #[test]
    fn outside_text_positions_clear_the_endpoint_and_remain_pickable() {
        let doc = Document::new();
        let view = rysmo_math::OrbitCamera::default().view(rysmo_math::DVec2::new(1000., 800.));
        let mut dimension = Dimension {
            measure: DimensionMeasure::Linear {
                from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                to: DimensionAnchor { point: DVec3::X * 2., reference: None },
                axis: None,
            },
            normal: DVec3::Z,
            offset: DVec3::Y,
            text: String::new(),
            alignment: Default::default(),
            font: Default::default(),
            text_position: DimensionTextPosition::Centered,
            endpoints: Default::default(),
        };
        for (position, expected, sign) in [
            (DimensionTextPosition::OutsideStart, DVec3::Y, -1.),
            (DimensionTextPosition::OutsideEnd, DVec3::Y + DVec3::X * 2., 1.),
        ] {
            dimension.text_position = position;
            let layout = dimension.layout(&doc, None, DMat4::IDENTITY, 0.1).unwrap();
            assert_eq!(layout.text_position, expected);
            let size = rysmo_math::DVec2::new(120., 20.);
            let (lo, hi) = dimension.text_bounds(&layout, &view, size).unwrap();
            let centre = (lo + hi) * 0.5;
            let endpoint = view.project(expected).unwrap();
            let direction = (view.project(expected + DVec3::X).unwrap() - endpoint).normalize();
            assert!((centre - endpoint).dot(direction) * sign > direction.abs().dot(size) * 0.5);
            assert_eq!(layout.hit_distance(&view, centre, Some((lo, hi))), Some(0.));
            assert!(layout.box_hit(
                &view,
                centre - rysmo_math::DVec2::ONE,
                centre + rysmo_math::DVec2::ONE,
                true,
                Some((lo, hi))
            ));
        }
        let mut legacy = serde_json::to_value(&dimension).unwrap();
        legacy.as_object_mut().unwrap().remove("text_position");
        legacy.as_object_mut().unwrap().remove("endpoints");
        legacy.as_object_mut().unwrap().remove("font");
        legacy.as_object_mut().unwrap().remove("alignment");
        assert_eq!(
            serde_json::from_value::<Dimension>(legacy.clone()).unwrap().alignment,
            DimensionTextAlignment::Screen
        );
        let mut loaded = serde_json::from_value::<Dimension>(legacy.clone()).unwrap();
        assert_eq!(loaded.font, DimensionFont::default());
        for size in [f32::NAN, f32::INFINITY, 0., 513.] {
            loaded.font.size = size;
            assert!(!loaded.validate());
        }
        assert_eq!(
            serde_json::from_value::<Dimension>(legacy.clone()).unwrap().endpoints,
            DimensionEndpoints::ClosedArrow
        );
        assert_eq!(serde_json::from_value::<Dimension>(legacy).unwrap().text_position, DimensionTextPosition::Centered);
    }

    #[test]
    fn radial_layout_places_tips_on_the_circle_toward_the_label() {
        let doc = Document::new();
        for diameter in [false, true] {
            let dimension = Dimension {
                measure: DimensionMeasure::Radial {
                    reference: RadialReference {
                        radial_axes: None,
                        arc_sweep: None,
                        path: vec![],
                        curve: None,
                        centre: DVec3::ZERO,
                        normal: DVec3::Z,
                        start: DVec3::X,
                        radius: 2.,
                    },
                    diameter,
                },
                normal: DVec3::Z,
                offset: DVec3::Y * 3.,
                text: String::new(),
                alignment: Default::default(),
                font: Default::default(),
                text_position: Default::default(),
                endpoints: Default::default(),
            };
            let layout = dimension.layout(&doc, None, DMat4::IDENTITY, 0.1).unwrap();
            assert_eq!(layout.text_position, DVec3::Y * 3.);
            assert_eq!(layout.measurement.length, if diameter { 4. } else { 2. });
            assert_eq!(layout.arrows.len(), if diameter { 2 } else { 1 });
            assert!(layout.arrows.iter().all(|(point, _)| (point.length() - 2.).abs() < 1e-8));
        }
    }
    #[test]
    fn arc_radius_tip_stays_on_the_arc_when_label_is_outside_its_sweep() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawArc {
                start: DVec3::ZERO,
                end: DVec3::X * 2.,
                normal: DVec3::Z,
                bulge: 1.,
                segments: 12,
            })
            .unwrap();
        let curve = editor.mesh().curves().next().unwrap().0;
        let dimension = Dimension {
            measure: DimensionMeasure::Radial {
                reference: RadialReference {
                    radial_axes: None,
                    arc_sweep: None,
                    path: vec![],
                    curve: Some(curve),
                    centre: DVec3::X,
                    normal: DVec3::Z,
                    start: -DVec3::X,
                    radius: 1.,
                },
                diameter: false,
            },
            normal: DVec3::Z,
            offset: DVec3::new(-1., -2., 0.),
            text: String::new(),
            alignment: Default::default(),
            font: Default::default(),
            text_position: Default::default(),
            endpoints: Default::default(),
        };
        let layout = dimension.layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        assert!(layout.arrows[0].0.distance(DVec3::ZERO) < 1e-8);
        assert!(layout.measurement.associated);
        assert!((layout.measurement.length - 1.).abs() < 1e-8);
        let detached = dimension.detached(editor.doc(), None);
        let fallback = detached.layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        assert_eq!(fallback.segments, layout.segments);
        assert_eq!(fallback.arrows, layout.arrows);
        let mirror = DMat4::from_scale(DVec3::new(-1., 1., 1.));
        let mapped = detached.mapped(mirror).layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        for (actual, expected) in mapped.segments.iter().flatten().zip(layout.segments.iter().flatten()) {
            assert!(actual.distance(mirror.transform_point3(*expected)) < 1e-8);
        }
    }
    #[test]
    fn dimension_hit_testing_uses_visible_lines_arrows_and_exact_text_bounds() {
        let view = rysmo_math::OrbitCamera::default().view(rysmo_math::DVec2::new(1000., 800.));
        let layout = DimensionLayout {
            segments: vec![[DVec3::ZERO, DVec3::X]],
            arrows: vec![(DVec3::X, -DVec3::X)],
            endpoints: Default::default(),
            text_position: DVec3::ZERO,
            text_direction: DVec3::X,
            measurement: ResolvedDimension {
                from: DVec3::ZERO,
                to: DVec3::X,
                length: 1.,
                normal: DVec3::Z,
                associated: true,
            },
        };
        let midpoint = view.project(DVec3::X * 0.5).unwrap();
        assert!(layout.hit_distance(&view, midpoint, None).unwrap() < 1e-8);
        let triangle = layout.screen_arrows(&view, 7., 2.5)[0];
        let centre = triangle.iter().copied().sum::<rysmo_math::DVec2>() / 3.;
        assert_eq!(layout.hit_distance(&view, centre, None), Some(0.));
        let lo = rysmo_math::DVec2::new(10., 10.);
        let hi = lo + rysmo_math::DVec2::new(100., 20.);
        assert_eq!(layout.hit_distance(&view, (lo + hi) * 0.5, Some((lo, hi))), Some(0.));
        assert!(layout.hit_distance(&view, rysmo_math::DVec2::splat(f64::NAN), None).is_none());
    }
    #[test]
    fn dimensions_behind_the_eye_are_not_selectable_in_either_projection() {
        for parallel in [false, true] {
            let camera = rysmo_math::OrbitCamera { parallel, ..Default::default() };
            let view = camera.view(rysmo_math::DVec2::new(1000., 800.));
            let behind = view.eye - view.ray(view.size * 0.5).dir;
            let layout = DimensionLayout {
                segments: vec![[behind, behind + DVec3::X * 0.1]],
                arrows: vec![(behind, DVec3::X)],
                endpoints: Default::default(),
                text_position: behind,
                text_direction: DVec3::X,
                measurement: ResolvedDimension {
                    from: behind,
                    to: behind + DVec3::X,
                    length: 1.,
                    normal: DVec3::Z,
                    associated: true,
                },
            };
            assert!(layout.text_on_screen(&view).is_none());
            assert!(layout.screen_arrows(&view, 7., 2.5).is_empty());
            assert!(layout.hit_distance(&view, view.size * 0.5, Some((rysmo_math::DVec2::ZERO, view.size))).is_none());
        }
    }
    #[test]
    fn visibility_thresholds_follow_camera_direction_distance_and_projection() {
        for parallel in [false, true] {
            let mut camera = rysmo_math::OrbitCamera { parallel, ..Default::default() };
            let view = camera.view(rysmo_math::DVec2::new(1000., 800.));
            let forward = view.view.inverse().transform_vector3(DVec3::Z).normalize();
            let sideways = view.view.inverse().transform_vector3(DVec3::X).normalize();
            let mut layout = DimensionLayout {
                segments: vec![[-forward * 0.5, forward * 0.5]],
                arrows: vec![],
                endpoints: Default::default(),
                text_position: DVec3::ZERO,
                text_direction: forward,
                measurement: ResolvedDimension {
                    from: DVec3::ZERO,
                    to: forward,
                    length: 1.,
                    normal: DVec3::Z,
                    associated: true,
                },
            };
            let angle = DimensionVisibility { hide_foreshortened: true, ..Default::default() };
            assert!(!layout.visible(&view, angle));
            layout.segments = vec![[-sideways * 0.5, sideways * 0.5]];
            assert!(layout.visible(&view, angle));
            let (a, b) = view.project_segment(-sideways * 0.5, sideways * 0.5).unwrap();
            let settings =
                DimensionVisibility { hide_small: true, minimum_pixels: a.distance(b) * 0.5, ..Default::default() };
            assert!(layout.visible(&view, settings));
            camera.distance *= 5.;
            assert!(!layout.visible(&camera.view(view.size), settings));
            assert!(layout.visible(&camera.view(view.size), DimensionVisibility::default()));
        }
    }

    #[test]
    fn aligned_text_rotates_with_view_and_does_not_pick_empty_bounding_corners() {
        let doc = Document::new();
        let mut camera = rysmo_math::OrbitCamera::default();
        let dimension = Dimension {
            measure: DimensionMeasure::Linear {
                from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                to: DimensionAnchor { point: DVec3::X * 2., reference: None },
                axis: None,
            },
            normal: DVec3::Z,
            offset: DVec3::Y,
            text: String::new(),
            font: Default::default(),
            alignment: DimensionTextAlignment::DimensionLine,
            text_position: Default::default(),
            endpoints: Default::default(),
        };
        for yaw in [0.3, 1.2, -2.4] {
            camera.yaw = yaw;
            let view = camera.view(rysmo_math::DVec2::new(1000., 800.));
            let mut layout = dimension.layout(&doc, None, DMat4::IDENTITY, 0.1).unwrap();
            layout.segments.clear();
            layout.arrows.clear();
            let size = rysmo_math::DVec2::new(160., 20.);
            let rect = dimension.text_rect(&layout, &view, size).unwrap();
            assert!(rect.axis.x >= 0.);
            let projected = (view.project(layout.text_position + DVec3::X).unwrap()
                - view.project(layout.text_position).unwrap())
            .normalize();
            assert!(rect.axis.dot(projected).abs() > 0.9999);
            let on_text = rect.centre + rect.axis * 60.;
            assert_eq!(dimension.hit_distance(&layout, &view, on_text, Some(size)), Some(0.));
            let (lo, _) = rect.bounds();
            let empty = lo + rysmo_math::DVec2::ONE;
            assert!(dimension.hit_distance(&layout, &view, empty, Some(size)).unwrap() > 4.);
            assert!(!dimension.box_hit(
                &layout,
                &view,
                empty - rysmo_math::DVec2::ONE,
                empty + rysmo_math::DVec2::ONE,
                true,
                Some(size)
            ));
            assert!(dimension.box_hit(
                &layout,
                &view,
                on_text - rysmo_math::DVec2::ONE,
                on_text + rysmo_math::DVec2::ONE,
                true,
                Some(size)
            ));
        }
    }

    #[test]
    fn endpoint_styles_share_visible_geometry_with_picking_and_box_selection() {
        let view = rysmo_math::OrbitCamera::default().view(rysmo_math::DVec2::new(1000., 800.));
        let mut layout = DimensionLayout {
            segments: vec![],
            arrows: vec![(DVec3::ZERO, DVec3::X)],
            endpoints: DimensionEndpoints::ClosedArrow,
            text_position: DVec3::ZERO,
            text_direction: DVec3::X,
            measurement: ResolvedDimension {
                from: DVec3::ZERO,
                to: DVec3::X,
                length: 1.,
                normal: DVec3::Z,
                associated: true,
            },
        };
        let [tip, a, b] = layout.screen_arrows(&view, 7., 2.5)[0];
        let interior = (tip + a + b) / 3.;
        assert_eq!(layout.hit_distance(&view, interior, None), Some(0.));
        layout.endpoints = DimensionEndpoints::OpenArrow;
        assert_eq!(layout.screen_endpoints(&view).len(), 2);
        assert!(layout.hit_distance(&view, interior, None).unwrap() > 0.5);
        let on_arm = (tip + a) * 0.5;
        assert!(layout.hit_distance(&view, on_arm, None).unwrap() < 1e-9);
        assert!(layout.box_hit(
            &view,
            on_arm - rysmo_math::DVec2::splat(0.1),
            on_arm + rysmo_math::DVec2::splat(0.1),
            true,
            None
        ));
        layout.endpoints = DimensionEndpoints::Slash;
        assert!(matches!(layout.screen_endpoints(&view).as_slice(), [DimensionEndMarker::Line(_)]));
        assert_eq!(layout.hit_distance(&view, tip, None), Some(0.));
        layout.endpoints = DimensionEndpoints::Dot;
        assert!(matches!(layout.screen_endpoints(&view).as_slice(), [DimensionEndMarker::Dot { .. }]));
        assert_eq!(layout.hit_distance(&view, tip + rysmo_math::DVec2::X * 2., None), Some(0.));
        assert!((layout.hit_distance(&view, tip + rysmo_math::DVec2::X * 3., None).unwrap() - 0.5).abs() < 1e-9);
        let tangent = tip + rysmo_math::DVec2::X * 2.5;
        assert!(layout.box_hit(&view, tangent, tangent + rysmo_math::DVec2::ONE, true, None));
        layout.endpoints = DimensionEndpoints::None;
        assert!(layout.screen_endpoints(&view).is_empty());
        assert!(layout.hit_distance(&view, tip, None).is_none());
        assert!(!layout.box_hit(&view, tip - rysmo_math::DVec2::ONE, tip + rysmo_math::DVec2::ONE, true, None));
        layout.endpoints = DimensionEndpoints::Dot;
        layout.arrows[0].0 = view.view.inverse().transform_point3(DVec3::Z);
        assert!(layout.screen_endpoints(&view).is_empty());
    }

    #[test]
    fn dimension_box_selection_distinguishes_window_crossing_and_label_extent() {
        let view = rysmo_math::OrbitCamera::default().view(rysmo_math::DVec2::new(1000., 800.));
        let layout = DimensionLayout {
            segments: vec![[DVec3::ZERO, DVec3::X]],
            arrows: vec![],
            endpoints: Default::default(),
            text_position: DVec3::X * 0.5,
            text_direction: DVec3::X,
            measurement: ResolvedDimension {
                from: DVec3::ZERO,
                to: DVec3::X,
                length: 1.,
                normal: DVec3::Z,
                associated: true,
            },
        };
        let centre = view.project(layout.text_position).unwrap();
        let lo = centre - rysmo_math::DVec2::splat(2.);
        let hi = centre + rysmo_math::DVec2::splat(2.);
        let text = Some((centre - rysmo_math::DVec2::new(50., 10.), centre + rysmo_math::DVec2::new(50., 10.)));
        assert!(layout.box_hit(&view, lo, hi, true, text));
        assert!(!layout.box_hit(&view, lo, hi, false, text));
        assert!(layout.box_hit(&view, rysmo_math::DVec2::ZERO, view.size, false, text));
        assert!(!layout.box_hit(&view, rysmo_math::DVec2::ZERO, view.size, false, None));
    }
}

impl Document {
    pub(crate) fn refresh_dimensions(&mut self) {
        let owners: Vec<_> = std::iter::once(None).chain(self.definitions.keys().copied().map(Some)).collect();
        let document = &*self;
        let updates: Vec<_> = owners
            .into_iter()
            .flat_map(|owner| {
                document
                    .def(owner)
                    .unwrap()
                    .dimensions
                    .iter()
                    .map(move |(id, dimension)| (owner, *id, dimension.refreshed(document, owner)))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (owner, id, dimension) in updates {
            self.def_mut(owner).unwrap().dimensions.insert(id, dimension);
        }
    }
}
