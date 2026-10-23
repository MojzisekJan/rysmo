use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{Command, DefinitionId, Element, parametric::Spec};

#[derive(Default)]
pub struct State {
    pub open: bool,
    spec: Spec,
    editing: Option<DefinitionId>,
    error: Option<String>,
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
    let response=crate::theme::window("Parametric components").open(&mut state.open).default_width(400.).resizable(true).show(ctx,|ui| {
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
        let validation=state.spec.validate();
        if let Err(error)=&validation {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        if let Some(error)=&state.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        ui.weak("Dimensions use local metres. Panes use ordinary face materials. Regeneration replaces generated faces; instance paint and placement remain. Make Unique to edit only one copy.");
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
    if update && let Some(definition) = state.editing {
        match app.editor.apply(Command::SetParametric { definition, spec: state.spec.clone() }) {
            Ok(_) => state.error = None,
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    app.features.parametric = state;
}
