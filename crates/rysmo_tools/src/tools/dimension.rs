//! Dimension creation uses the same associated data and layout as saved entities.
use super::*;
use rysmo_model::dimensions::{
    Dimension, DimensionAnchor, DimensionMeasure, PointEntity, PointReference, RadialReference,
};

pub(crate) fn origin(dimension: &Dimension) -> DVec3 {
    match &dimension.measure {
        DimensionMeasure::Linear { from, .. } => from.point,
        DimensionMeasure::Radial { reference, .. } => reference.centre,
    }
}

pub(super) fn anchor(editor: &Editor, inf: Inference) -> DimensionAnchor {
    let mut result = DimensionAnchor { point: inf.point, reference: None };
    if inf.kind == Kind::Centre
        && let Some(curve) = inf.curve
    {
        for occurrence in editor.doc().occurrences(editor.context()).into_iter().filter(|o| o.in_context) {
            if inf.instance.is_some_and(|instance| occurrence.path.get(editor.context().len()) != Some(&instance)) {
                continue;
            }
            let mesh = &editor.doc().def(occurrence.definition).unwrap().mesh;
            let Some(member) = mesh.curve(curve) else { continue };
            let transform = editor.context_transform().inverse() * occurrence.transform;
            if transform.transform_point3(member.centre).distance(inf.point) > EPS {
                continue;
            }
            result.reference = Some(PointReference {
                path: occurrence.path[editor.context().len()..].to_vec(),
                entity: PointEntity::CurveCentre { curve },
            });
            return result;
        }
    }
    let Some(edge) = inf.edge else { return result };
    for occurrence in editor.doc().occurrences(editor.context()).into_iter().filter(|o| o.in_context) {
        let mesh = &editor.doc().def(occurrence.definition).unwrap().mesh;
        let Some(member) = mesh.edge(edge) else { continue };
        let transform = editor.context_transform().inverse() * occurrence.transform;
        let a = transform.transform_point3(mesh.pos(member.v[0]));
        let b = transform.transform_point3(mesh.pos(member.v[1]));
        let entity = if inf.point.distance(a) < EPS {
            PointEntity::Vertex { vertex: member.v[0] }
        } else if inf.point.distance(b) < EPS {
            PointEntity::Vertex { vertex: member.v[1] }
        } else {
            PointEntity::Edge { edge, parameter: ((inf.point - a).dot(b - a) / (b - a).length_squared()).clamp(0., 1.) }
        };
        result.reference = Some(PointReference { path: occurrence.path[editor.context().len()..].to_vec(), entity });
        break;
    }
    result
}

impl ToolBox {
    pub(crate) fn dimension_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        if self.dimension_draft.is_some() {
            self.dimension_move(editor, ctx);
            let _ = self.finish_dimension(editor);
            return;
        }
        let end = anchor(editor, inf);
        if let Some(start) = self.dimension_start.take() {
            if start.point.distance(end.point) < EPS {
                self.dimension_start = Some(start);
                return;
            }
            let direction = (end.point - start.point).normalize();
            let candidate = inf.plane.map(|p| p.normal).unwrap_or(self.axes.z());
            let perpendicular = candidate - direction * candidate.dot(direction);
            let normal = if perpendicular.length() > EPS {
                perpendicular.normalize()
            } else {
                Plane::new(direction, DVec3::ZERO).basis().0
            };
            self.dimension_draft = Some(Dimension {
                measure: DimensionMeasure::Linear { from: start, to: end, axis: None },
                normal,
                offset: DVec3::ZERO,
                text: String::new(),
                alignment: editor.doc().style.dimension_defaults.alignment,
                font: editor.doc().style.dimension_defaults.font,
                text_position: Default::default(),
                endpoints: editor.doc().style.dimension_defaults.endpoints,
            });
            return;
        }
        if !ctx.modifiers.ctrl
            && inf.kind != Kind::Endpoint
            && let Some(edge) = inf.edge
        {
            for occurrence in editor.doc().occurrences(editor.context()).into_iter().filter(|o| o.in_context) {
                let mesh = &editor.doc().def(occurrence.definition).unwrap().mesh;
                let Some(member) = mesh.edge(edge) else { continue };
                let transform = editor.context_transform().inverse() * occurrence.transform;
                let path = occurrence.path[editor.context().len()..].to_vec();
                if let Some((id, curve)) = member.curve.and_then(|id| mesh.curve(id).map(|c| (id, c)))
                    && matches!(curve.kind, rysmo_mesh::CurveKind::Circle | rysmo_mesh::CurveKind::Arc)
                {
                    let u = transform.transform_vector3(curve.start.normalize() * curve.radius);
                    let v = transform
                        .transform_vector3(curve.normal.normalize().cross(curve.start.normalize()) * curve.radius);
                    if (u.length() - v.length()).abs() < EPS * u.length().max(1.)
                        && u.normalize().dot(v.normalize()).abs() < 1e-6
                    {
                        let normal = u.cross(v).normalize();
                        self.dimension_draft = Some(Dimension {
                            measure: DimensionMeasure::Radial {
                                reference: RadialReference {
                                    radial_axes: None,
                                    arc_sweep: (curve.kind == rysmo_mesh::CurveKind::Arc).then_some(curve.sweep),
                                    path,
                                    curve: Some(id),
                                    centre: transform.transform_point3(curve.centre),
                                    normal,
                                    start: u.normalize(),
                                    radius: u.length(),
                                },
                                diameter: curve.kind == rysmo_mesh::CurveKind::Circle,
                            },
                            normal,
                            offset: DVec3::ZERO,
                            text: String::new(),
                            alignment: editor.doc().style.dimension_defaults.alignment,
                            font: editor.doc().style.dimension_defaults.font,
                            text_position: Default::default(),
                            endpoints: editor.doc().style.dimension_defaults.endpoints,
                        });
                        return;
                    }
                }
                if inf.kind == Kind::OnEdge {
                    let point = |vertex| DimensionAnchor {
                        point: transform.transform_point3(mesh.pos(vertex)),
                        reference: Some(PointReference { path: path.clone(), entity: PointEntity::Vertex { vertex } }),
                    };
                    let from = point(member.v[0]);
                    let to = point(member.v[1]);
                    let direction = (to.point - from.point).normalize();
                    let candidate = inf.plane.map(|p| p.normal).unwrap_or(self.axes.z());
                    let perpendicular = candidate - direction * candidate.dot(direction);
                    let normal = if perpendicular.length() > EPS {
                        perpendicular.normalize()
                    } else {
                        Plane::new(direction, DVec3::ZERO).basis().0
                    };
                    self.dimension_draft = Some(Dimension {
                        measure: DimensionMeasure::Linear { from, to, axis: None },
                        normal,
                        offset: DVec3::ZERO,
                        text: String::new(),
                        alignment: editor.doc().style.dimension_defaults.alignment,
                        font: editor.doc().style.dimension_defaults.font,
                        text_position: Default::default(),
                        endpoints: editor.doc().style.dimension_defaults.endpoints,
                    });
                    return;
                }
            }
        }
        self.dimension_start = Some(end);
    }

    pub(crate) fn dimension_move(&mut self, _editor: &Editor, ctx: &Ctx) {
        let Some(dimension) = &mut self.dimension_draft else { return };
        let from = origin(dimension);
        let plane = Plane::new(dimension.normal, from);
        let point = self
            .inference
            .map(|i| i.point)
            .or_else(|| plane.intersect_ray(&ctx.view.ray(ctx.cursor)).map(|t| ctx.view.ray(ctx.cursor).at(t)));
        if let Some(point) = point {
            let mut offset = plane.project(point) - from;
            if let DimensionMeasure::Linear { from, to, .. } = &dimension.measure {
                let direction = (to.point - from.point).normalize();
                offset -= direction * offset.dot(direction);
            }
            dimension.offset = offset;
        }
    }

    fn finish_dimension(&mut self, editor: &mut Editor) -> Result<(), String> {
        let dimension = self.dimension_draft.clone().ok_or("Select the measured geometry first.")?;
        if let Some(outcome) = self.run(editor, Command::AddDimension { dimension }) {
            editor.set_selection(outcome.created);
            self.reset();
            Ok(())
        } else {
            Err(self.message.clone().unwrap_or_else(|| "Could not create the dimension.".into()))
        }
    }

    pub(crate) fn dimension_vcb(&mut self, editor: &mut Editor, text: &str) -> Result<(), String> {
        let value = rysmo_model::parse_length(text, editor.doc().units).map_err(|e| e.to_string())?;
        if !value.is_finite() {
            return Err("Enter a finite offset.".into());
        }
        let dimension = self.dimension_draft.as_mut().ok_or("Select the measured geometry first.")?;
        let direction = if dimension.offset.length() > EPS {
            dimension.offset.normalize()
        } else {
            match &dimension.measure {
                DimensionMeasure::Linear { from, to, .. } => {
                    dimension.normal.cross((to.point - from.point).normalize()).normalize()
                }
                DimensionMeasure::Radial { reference, .. } => reference.start.normalize(),
            }
        };
        let scale = editor.context_transform().transform_vector3(direction).length();
        if !scale.is_finite() || scale < EPS {
            return Err("The editing context has no usable scale.".into());
        }
        let offset = direction * (value / scale);
        if !offset.is_finite() {
            return Err("The offset is too large for the editing context.".into());
        }
        dimension.offset = offset;
        self.finish_dimension(editor)
    }
}
