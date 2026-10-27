use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{Command, DefinitionId, Element, parametric::Spec};

#[derive(Default)]
pub struct State {
    pub open: bool,
    spec: Spec,
    editing: Option<DefinitionId>,
    error: Option<String>,
    resize: Option<Resize>,
}

fn selected(app: &App3d) -> Option<DefinitionId> {
    let id = match app.editor.selected().as_slice() {
        [Element::Instance(id)] => *id,
        _ => return None,
    };
    let instance = app.editor.doc().def(app.editor.active_definition())?.instances.get(&id)?;
    app.editor.doc().definitions.get(&instance.definition)?.parametric.as_ref()?;
    Some(instance.definition)
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.parametric.open {
        return;
    }
    let mut state = std::mem::take(&mut app.features.parametric);
    let selected = selected(app);
    let mut create = false;
    let mut update = false;
    let mut fit = false;
    let fitting = {
        let mut instances =
            app.editor.selection().iter().filter_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None });
        let instance = instances.next();
        let face = app.editor.selection().iter().find_map(|e| if let Element::Face(id) = e { Some(*id) } else { None });
        instance.zip(face).filter(|_| instances.next().is_none() && app.editor.selection().len() == 2)
    };
    let response=crate::theme::window("Parametric components").vscroll(true).beside_model().open(&mut state.open).default_size(egui::vec2(420.,520.)).resizable(true).show(ctx,|ui| {
        ui.horizontal_wrapped(|ui| {
            for (label,spec) in [("Window",Spec::default()),("Door",Spec::Door{width:0.9,height:2.1,depth:0.12,frame:0.06,opening:0.}), ("Stairs",Spec::Stairs{width:1.,rise:0.18,tread:0.28,steps:12})] {
                if ui.selectable_label(state.spec.name()==label,label).clicked() {state.spec=spec;state.editing=None;state.error=None;}
            }
        });
        if let Some(id)=selected && ui.button("Edit selected component").clicked()
            && let Some(spec)=app.editor.doc().definitions.get(&id).and_then(|d| d.parametric.clone()) {
                state.spec=spec;state.editing=Some(id);state.error=None;
        }
        let length=|ui:&mut egui::Ui,label:&str,v:&mut f64| {
            ui.label(label);ui.add(crate::widgets::Number::new(v).speed(0.005).range(0.001..=1000.).suffix(" m"));ui.end_row();
        };
        // One grid keeps every value field aligned, whatever the label widths.
        egui::Grid::new("parametric-values").num_columns(2).spacing([12.,8.]).show(ui,|ui| match &mut state.spec {
            Spec::Window{width,height,depth,frame,columns}=>{length(ui,"Width",width);length(ui,"Height",height);length(ui,"Depth",depth);length(ui,"Frame",frame);ui.label("Columns");ui.add(crate::widgets::Number::new(columns).range(1..=16));ui.end_row();},
            Spec::Door{width,height,depth,frame,opening}=>{length(ui,"Width",width);length(ui,"Height",height);length(ui,"Depth",depth);length(ui,"Frame",frame);ui.label("Opening");ui.add(crate::widgets::Number::new(opening).range(0. ..=170.).suffix("°"));ui.end_row();},
            Spec::Stairs{width,rise,tread,steps}=>{length(ui,"Width",width);length(ui,"Rise",rise);length(ui,"Tread",tread);ui.label("Steps");ui.add(crate::widgets::Number::new(steps).range(1..=128));ui.end_row();},
        });
        fit=ui.add_enabled(fitting.is_some(),egui::Button::new("Fit selected component to selected rectangle")).clicked();
        ui.small("Fitting makes the selected occurrence unique. Select one window/door and one rectangular face. A face is used as a fitting target, not cut automatically.");
        let validation=state.spec.validate();
        if let Err(error)=&validation {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        if let Some(error)=&state.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        ui.weak("Dimensions use local metres. Panes use ordinary face materials. Regeneration retains paint on matching generated parts; instance placement remains. Make Unique to edit only one copy.");
        ui.horizontal_wrapped(|ui| {
            create=ui.add_enabled(validation.is_ok(),egui::Button::new("Place new component")).clicked();
            if let Some(id)=state.editing {
                let count=app.editor.doc().instance_count(id);
                update=ui.add_enabled(validation.is_ok() && app.editor.doc().definitions.contains_key(&id),egui::Button::new(if count==1 {"Update 1 instance".to_owned()} else {format!("Update {count} instances")})).clicked();
            }
        });
    });
    super::remember(app, &response);
    if create {
        match state
            .spec
            .document()
            .and_then(|doc| crate::object_clipboard::place_external(app, doc, state.spec.name().into(), true))
        {
            Ok(()) => state.open = false,
            Err(error) => state.error = Some(error),
        }
    }
    if fit && let Some((instance, face)) = fitting {
        match app.editor.apply(Command::FitParametric { instance, face, make_unique: true }) {
            Ok(_) => {
                state.editing = None;
                state.error = None;
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    if update && let Some(definition) = state.editing {
        match app.editor.apply(Command::SetParametric { definition, spec: state.spec.clone() }) {
            Ok(_) => state.error = None,
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    app.features.parametric = state;
}

struct Resize {
    definition: DefinitionId,
    instance: rysmo_model::InstanceId,
    revision: u64,
    axis: usize,
    original: Spec,
    draft: Spec,
    screen_per_metre: egui::Vec2,
}
fn dimensions(spec: &Spec) -> [f64; 3] {
    match *spec {
        Spec::Window { width, height, depth, .. } | Spec::Door { width, height, depth, .. } => [width, depth, height],
        Spec::Stairs { width, rise, tread, steps } => [width, tread * steps as f64, rise * steps as f64],
    }
}
fn resized(spec: &Spec, axis: usize, delta: f64) -> Spec {
    let mut spec = spec.clone();
    match &mut spec {
        Spec::Window { width, height, depth, .. } | Spec::Door { width, height, depth, .. } => {
            let value = match axis {
                0 => width,
                1 => depth,
                _ => height,
            };
            *value = (*value + delta).max(0.001);
        }
        Spec::Stairs { width, rise, tread, steps } => {
            let divisor = if axis == 0 { 1. } else { *steps as f64 };
            let value = match axis {
                0 => width,
                1 => tread,
                _ => rise,
            };
            *value = (*value + delta / divisor).max(0.001);
        }
    }
    spec
}
/// Screen handles preview dimensions and commit one regeneration on release.
pub fn grips(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.parametric.open {
        app.features.parametric.resize = None;
        return;
    }
    let Some(definition) = selected(app) else {
        app.features.parametric.resize = None;
        return;
    };
    let instance = match app.editor.selected().as_slice() {
        [Element::Instance(id)] => *id,
        _ => return,
    };
    let Some(spec) = app.editor.doc().definitions[&definition].parametric.clone() else {
        return;
    };
    let transform = app.editor.context_transform()
        * app.editor.doc().def(app.editor.active_definition()).unwrap().instances[&instance].transform;
    if app
        .features
        .parametric
        .resize
        .as_ref()
        .is_some_and(|r| r.definition != definition || r.instance != instance || r.revision != app.editor.revision())
        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
    {
        app.features.parametric.resize = None;
        return;
    }
    let draft = app.features.parametric.resize.as_ref().map(|r| r.draft.clone()).unwrap_or_else(|| spec.clone());
    let size = dimensions(&draft);
    let sign = if matches!(draft, Spec::Stairs { .. }) { 1. } else { -1. };
    let points = [
        rysmo_math::DVec3::new(size[0], sign * size[1] * 0.5, size[2] * 0.5),
        rysmo_math::DVec3::new(size[0] * 0.5, sign * size[1], size[2] * 0.5),
        rysmo_math::DVec3::new(size[0] * 0.5, sign * size[1] * 0.5, size[2]),
    ];
    let axes = [rysmo_math::DVec3::X, rysmo_math::DVec3::Y * sign, rysmo_math::DVec3::Z];
    if app.features.parametric.resize.is_some() {
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("parametric-preview")));
        let corners: Vec<_> = (0..8)
            .map(|i| {
                transform.transform_point3(rysmo_math::DVec3::new(
                    if i & 1 == 0 { 0. } else { size[0] },
                    if i & 2 == 0 { 0. } else { sign * size[1] },
                    if i & 4 == 0 { 0. } else { size[2] },
                ))
            })
            .collect();
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit == 0
                    && let (Some(a), Some(b)) = (app.view().project(corners[i]), app.view().project(corners[i | bit]))
                {
                    painter.line_segment(
                        [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(80, 190, 230)),
                    );
                }
            }
        }
    }
    let mut commit = None;
    for (axis, point) in points.into_iter().enumerate() {
        let world = transform.transform_point3(point);
        let Some(at) = app.view().project(world) else {
            continue;
        };
        if app.over_panel(at) && !app.features.parametric.resize.as_ref().is_some_and(|resize| resize.axis == axis) {
            continue;
        }
        let Some(step) = app.view().project(transform.transform_point3(point + axes[axis])) else {
            continue;
        };
        let screen = egui::vec2((step.x - at.x) as f32, (step.y - at.y) as f32);
        if screen.length_sq() < 4. {
            continue;
        }
        let response = egui::Area::new(egui::Id::unique(("parametric-grip", axis)))
            .movable(false)
            .fixed_pos(egui::pos2(at.x as f32 - 10., at.y as f32 - 10.))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::drag());
                ui.painter().rect_filled(
                    rect.shrink(4.),
                    2.,
                    if response.hovered() || response.dragged() {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::from_rgb(80, 190, 230)
                    },
                );
                response.on_hover_text(format!(
                    "Drag {} ({:.3} m). Shared copies update; Undo reverts.",
                    ["width", "depth / run", "height / rise"][axis],
                    size[axis]
                ))
            });
        let rect = response.response.rect;
        app.ui_rects.push((
            rysmo_math::DVec2::new(rect.min.x as f64, rect.min.y as f64),
            rysmo_math::DVec2::new(rect.max.x as f64, rect.max.y as f64),
        ));
        let response = response.inner;
        if response.drag_started() {
            app.features.parametric.resize = Some(Resize {
                definition,
                instance,
                revision: app.editor.revision(),
                axis,
                original: spec.clone(),
                draft: spec.clone(),
                screen_per_metre: screen,
            });
        }
        if let Some(resize) = &mut app.features.parametric.resize
            && resize.axis == axis
        {
            let delta = response.total_drag_delta().unwrap_or_default().dot(resize.screen_per_metre)
                / resize.screen_per_metre.length_sq();
            if response.dragged() {
                resize.draft = resized(&resize.original, axis, delta as f64);
            }
            if response.drag_stopped() {
                commit = Some(resize.draft.clone());
            }
        }
    }
    if let Some(next) = commit {
        app.features.parametric.resize = None;
        match app.editor.apply(Command::SetParametric { definition, spec: next.clone() }) {
            Ok(_) => {
                app.features.parametric.spec = next;
                app.features.parametric.error = None;
            }
            Err(error) => app.features.parametric.error = Some(error.to_string()),
        }
    }
}

pub fn cancel_resize(app: &mut App3d) {
    app.features.parametric.resize = None;
}
