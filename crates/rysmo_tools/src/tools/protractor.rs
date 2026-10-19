//! Angle measurement and construction guides.
use super::{Ctx, State, ToolBox};
use rysmo_infer::Inference;
use rysmo_math::{DQuat, EPS, Plane};
use rysmo_model::{Command, Editor};

impl ToolBox {
    pub(crate) fn protractor_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        match self.state.clone() {
            State::Idle => {
                let plane = self
                    .axis_lock
                    .map(|a| Plane::new(self.axes.dir(a), inf.point))
                    .unwrap_or_else(|| super::draw::drawing_plane(&inf, editor.mesh(), self.axes));
                self.state = State::AngleBase { centre: inf.point, plane };
            }
            State::AngleBase { centre, plane } => {
                let from = plane.project(inf.point) - centre;
                if from.length() > EPS {
                    self.state = State::AngleSweep { centre, plane, from: from.normalize(), angle: 0.0 };
                }
            }
            State::AngleSweep { angle, .. } => {
                self.finish_protractor(editor, ctx, angle);
            }
            _ => {}
        }
    }

    pub(crate) fn protractor_move(&mut self, editor: &Editor) {
        if let State::AngleSweep { centre, plane, from, angle } = &mut self.state
            && let Some(inf) = self.inference
        {
            let to = plane.project(inf.point) - *centre;
            if to.length() > EPS {
                let to = to.normalize();
                *angle = from.cross(to).dot(plane.normal).atan2(from.dot(to)).to_degrees();
                if !matches!(
                    inf.kind,
                    rysmo_infer::Kind::Endpoint
                        | rysmo_infer::Kind::Midpoint
                        | rysmo_infer::Kind::Centre
                        | rysmo_infer::Kind::Intersection
                        | rysmo_infer::Kind::OnEdge
                        | rysmo_infer::Kind::GuidePoint
                        | rysmo_infer::Kind::OnGuide
                ) {
                    *angle = editor.doc().measurement.snap_angle(*angle, 1.);
                }
            }
        }
    }

    fn finish_protractor(&mut self, editor: &mut Editor, ctx: &Ctx, angle: f64) {
        let State::AngleSweep { centre, plane, from, .. } = self.state else { return };
        if ctx.modifiers.ctrl {
            let dir = DQuat::from_axis_angle(plane.normal, angle.to_radians()) * from;
            if self.run(editor, Command::AddGuideLine { origin: centre, dir }).is_none() {
                return;
            }
        }
        self.reset();
        self.last = super::Last::None;
        self.message = Some(format!(
            "Angle: {}{}",
            editor.doc().measurement.format_angle(angle.abs()),
            if ctx.modifiers.ctrl { ". Guide created." } else { "." }
        ));
    }

    pub(crate) fn protractor_vcb(&mut self, editor: &mut Editor, ctx: &Ctx, text: &str) -> Result<(), String> {
        if !matches!(self.state, State::AngleSweep { .. }) {
            return Err("Pick the vertex and the start of the angle first.".into());
        }
        let angle = text
            .trim()
            .trim_end_matches('°')
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|a| a.is_finite())
            .ok_or("Enter a finite angle in degrees.")?;
        self.finish_protractor(editor, ctx, angle);
        self.update_status();
        Ok(())
    }
}

pub(crate) fn append_ticks(
    pv: &mut super::Preview,
    centre: rysmo_math::DVec3,
    axis: rysmo_math::DVec3,
    reference: rysmo_math::DVec3,
    radius: f64,
    settings: rysmo_model::MeasurementSettings,
) {
    let Some(step) = settings.angle_snap else { return };
    let step = step * (5. / step).ceil().max(1.);
    let side = axis.normalize().cross(reference.normalize());
    for tick in 0..(360. / step).ceil() as usize {
        let angle = (tick as f64 * step).to_radians();
        let direction = reference.normalize() * angle.cos() + side * angle.sin();
        pv.lines.push(super::PreviewLine {
            a: centre + direction * radius * 0.88,
            b: centre + direction * radius,
            color: [0.35; 3],
            dashed: false,
            thick: false,
        });
    }
}
