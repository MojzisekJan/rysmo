//! The Materials and Tags windows. Both are small floating windows that stay out of
//! the 3D view's way.

use bevy_egui::egui;
use rysmo_model::{Command, Element, InstanceId, MaterialId};
use rysmo_tools::ToolKind;

use crate::state::App3d;

/// Cache topology and triangulation while the Properties panel is idle.
#[derive(Default)]
pub(crate) struct VolumeCache {
    entry: Option<VolumeCacheEntry>,
}
#[derive(PartialEq, Eq)]
struct VolumeCacheKey {
    generation: u64,
    revision: u64,
    context: Vec<InstanceId>,
    instance: InstanceId,
}
struct VolumeCacheEntry {
    key: VolumeCacheKey,
    value: Result<f64, &'static str>,
}
impl VolumeCache {
    fn get(&mut self, editor: &rysmo_model::Editor, generation: u64, id: InstanceId) -> Result<f64, &'static str> {
        let key = VolumeCacheKey {
            generation,
            revision: editor.revision(),
            context: editor.context().to_vec(),
            instance: id,
        };
        if let Some(cached) = &self.entry
            && cached.key == key
        {
            return cached.value;
        }
        let value = editor
            .active()
            .instances
            .get(&id)
            .ok_or("Missing instance")
            .and_then(|inst| editor.doc().instance_volume(inst, editor.context_transform()));
        self.entry = Some(VolumeCacheEntry { key, value });
        value
    }
}

/// Generate the bundled images once, rather than on every UI frame.
fn library() -> &'static [rysmo_library::LibraryMaterial] {
    static LIBRARY: std::sync::OnceLock<Vec<rysmo_library::LibraryMaterial>> = std::sync::OnceLock::new();
    LIBRARY.get_or_init(rysmo_library::materials)
}

fn choose_library_material(app: &mut App3d, entry: &rysmo_library::LibraryMaterial) {
    // Reuse the document material, including any edits the user made to it.
    let existing = app.editor.doc().material_by_name(entry.name);
    let id = if let Some(id) = existing {
        Some(id)
    } else {
        let cmd = if let Some(t) = &entry.texture {
            Command::AddTexturedMaterial {
                name: entry.name.into(),
                color: entry.color,
                image_name: t.file.into(),
                mime: "image/png".into(),
                data: t.png.clone(),
                size: t.size,
            }
        } else {
            Command::AddMaterial { name: entry.name.into(), color: entry.color, texture: None }
        };
        apply(app, cmd).and_then(|out| out.material)
    };
    if let Some(id) = id {
        app.tools.set_paint(Some(id));
        app.set_tool(ToolKind::PaintBucket);
    }
}

fn swatch_color(c: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
        (c[3] * 255.0) as u8,
    )
}

fn apply(app: &mut App3d, cmd: Command) -> Option<rysmo_model::Outcome> {
    match app.editor.apply(cmd) {
        Ok(o) => Some(o),
        Err(e) => {
            app.message = Some(format!("{e}."));
            None
        }
    }
}

pub fn materials(ctx: &egui::Context, app: &mut App3d) {
    if !app.show_materials {
        return;
    }
    let mut open = true;
    crate::theme::window("Materials")
        .beside_model()
        .open(&mut open)
        .default_width(230.0)
        .resizable(true)
        .vscroll(true)
        .show(ctx, |ui| {
            let mut surfaces = app.tools.paint_surface();
            if crate::widgets::checkbox(ui, &mut surfaces, "Paint connected surfaces")
                .on_hover_text("Paint facets joined by soft edges together. Turn off to paint individual faces.")
                .changed()
            {
                app.tools.set_paint_surface(surfaces);
            }
            ui.label("In model");
            let current = app.tools.paint();
            let size = if app.touch.enabled { 40.0 } else { 30.0 };
            let mut pick: Option<Option<MaterialId>> = None;
            ui.horizontal_wrapped(|ui| {
                // The default material (no paint): white front, blue-grey back.
                let (r, p) = ui.allocate_painter(egui::vec2(size, size), egui::Sense::click());
                p.rect_filled(r.rect, 3.0, egui::Color32::from_rgb(245, 243, 235));
                p.line_segment(
                    [r.rect.left_bottom(), r.rect.right_top()],
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(163, 178, 204)),
                );
                if current.is_none() {
                    p.rect_stroke(r.rect, 3.0, egui::Stroke::new(2.0, egui::Color32::BLUE), egui::StrokeKind::Outside);
                }
                if r.on_hover_text("Default").clicked() {
                    pick = Some(None);
                }
                let mats: Vec<(MaterialId, String, [f32; 4])> =
                    app.editor.doc().materials.iter().map(|(id, m)| (*id, m.name.clone(), m.color)).collect();
                for (id, name, color) in mats {
                    let (r, p) = ui.allocate_painter(egui::vec2(size, size), egui::Sense::click());
                    // A checkerboard behind see-through colours.
                    if color[3] < 1.0 {
                        p.rect_filled(r.rect, 3.0, egui::Color32::from_gray(200));
                        let half = r.rect.size() * 0.5;
                        p.rect_filled(egui::Rect::from_min_size(r.rect.min, half), 0.0, egui::Color32::from_gray(120));
                        p.rect_filled(
                            egui::Rect::from_min_size(r.rect.min + half, half),
                            0.0,
                            egui::Color32::from_gray(120),
                        );
                    }
                    p.rect_filled(r.rect, 3.0, swatch_color(color));
                    if current == Some(id) {
                        p.rect_stroke(
                            r.rect,
                            3.0,
                            egui::Stroke::new(2.0, egui::Color32::BLUE),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if r.on_hover_text(name).clicked() {
                        pick = Some(Some(id));
                    }
                }
            });
            if let Some(m) = pick {
                app.tools.set_paint(m);
                if app.tools.active() != ToolKind::PaintBucket {
                    app.set_tool(ToolKind::PaintBucket);
                }
            }
            ui.separator();
            if let Some(id) = current
                && let Some(m) = app.editor.doc().materials.get(&id).cloned()
            {
                let mut color = m.color;
                ui.horizontal(|ui| {
                    ui.label(&m.name);
                    ui.color_edit_button_rgba_unmultiplied(&mut color);
                });
                ui.add(crate::widgets::Slider::new(&mut color[3], 0.05..=1.0).text("Opacity"));
                if color != m.color {
                    // One undo step per drag: replace the step made by the previous frame.
                    if app.editor.undo_label() == Some("Edit Material") && app.material_editing == Some(id) {
                        app.editor.undo();
                    }
                    app.material_editing = Some(id);
                    apply(app, Command::SetMaterial { material: id, name: None, color: Some(color) });
                }
                let mut pbr=m.pbr;
                if m.texture.is_some() {crate::widgets::checkbox(ui,&mut pbr.tint_texture,"Tint colour texture");}
                ui.add(crate::widgets::Slider::new(&mut pbr.roughness,0.0..=1.0).text("Roughness"));
                ui.add(crate::widgets::Slider::new(&mut pbr.metallic,0.0..=1.0).text("Metallic"));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Load normal map...").clicked() {crate::features::data_files::pick(app,crate::features::data_files::Purpose::NormalMap(id));}
                    if pbr.normal_map.is_some() && ui.button("Remove normal map").clicked() {pbr.normal_map=None;}
                });
                if pbr.normal_map.is_some() {
                    ui.add(crate::widgets::Slider::new(&mut pbr.normal_scale,0.0..=8.0).text("Normal strength"));
                    crate::widgets::checkbox(ui,&mut pbr.normal_flip_y,"Flip normal Y (DirectX)");
                    ui.weak("Normal maps share face UVs. Without a colour texture, one UV unit equals one metre. Use sun or studio lighting to see surface shading.");
                }
                if pbr!=m.pbr {
                    if app.editor.undo_label()==Some("Material Surface") && app.material_editing==Some(id) {app.editor.undo();}
                    app.material_editing=Some(id);
                    apply(app,Command::SetMaterialPbr{material:id,pbr});
                }
                let mut glow = m.glow;
                ui.horizontal(|ui| {
                    ui.label("Glow");
                    ui.add(
                        crate::widgets::Number::new(&mut glow)
                            .speed(10.0)
                            .range(0.0..=rysmo_model::MAX_GLOW)
                            .suffix(" cd/m²"),
                    )
                    .on_hover_text("Light the faces give off in sun or studio lighting. A lamp shade is about 2000.");
                });
                if glow != m.glow {
                    if app.editor.undo_label() == Some("Glow") && app.material_editing == Some(id) {
                        app.editor.undo();
                    }
                    app.material_editing = Some(id);
                    apply(app, Command::SetMaterialGlow { material: id, glow });
                }
            }
            if let Some(id) = app.tools.paint()
                && let Some(mut texture) = app.editor.doc().materials.get(&id).and_then(|m| m.texture)
            {
                let mut size_changed = false;
                let units = app.editor.doc().units;
                let factor = units.per_metre();
                let mut size = texture.size.map(|value| value * factor);
                ui.label(format!("Texture size ({})", units.suffix()));
                ui.horizontal(|ui| {
                    ui.label("Width");
                    size_changed |= ui
                        .add(
                            crate::widgets::Number::new(&mut size[0])
                                .speed(0.01 * factor)
                                .range(factor * 0.001..=factor * 10000.0),
                        )
                        .changed();
                    ui.label("Height");
                    size_changed |= ui
                        .add(
                            crate::widgets::Number::new(&mut size[1])
                                .speed(0.01 * factor)
                                .range(factor * 0.001..=factor * 10000.0),
                        )
                        .changed();
                });
                if size_changed {
                    texture.size = size.map(|value| value / factor);
                    if app.editor.undo_label() == Some("Texture") && app.material_editing == Some(id) {
                        app.editor.undo();
                    }
                    app.material_editing = Some(id);
                    apply(app, Command::SetMaterialTexture { material: id, texture: Some(texture) });
                }
                if ui.button("Remove texture").clicked() {
                    apply(app, Command::SetMaterialTexture { material: id, texture: None });
                    app.material_editing = None;
                }
            }
            if !ui.ctx().input(|i| i.pointer.any_down()) {
                app.material_editing = None;
            }
            ui.horizontal(|ui| {
                ui.color_edit_button_rgba_unmultiplied(&mut app.new_color);
                if ui.button("New material").clicked() {
                    let n = app.editor.doc().materials.len() + 1;
                    let color = app.new_color;
                    if let Some(out) =
                        apply(app, Command::AddMaterial { name: format!("Material {n}"), color, texture: None })
                    {
                        app.tools.set_paint(out.material);
                    }
                }
            });
            if ui.button("Load texture image...").clicked() {
                crate::files::import_texture(app);
            }
            crate::widgets::section(ui, "Material library", false, |ui| {
                egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    for entry in library() {
                        ui.horizontal(|ui| {
                            let (r, painter) = ui.allocate_painter(egui::vec2(20.0, 20.0), egui::Sense::hover());
                            painter.rect_filled(r.rect, 3.0, swatch_color(entry.color));
                            let label = if entry.texture.is_some() {
                                format!("{} (texture)", entry.name)
                            } else {
                                entry.name.to_string()
                            };
                            if ui.button(label).clicked() {
                                choose_library_material(app, entry);
                            }
                        });
                    }
                });
            });
            ui.label("Alt+click samples a material. Ctrl paints connected faces, Shift all matching.");
        });
    app.show_materials = open;
}

pub fn tags(ctx: &egui::Context, app: &mut App3d) {
    if !app.show_tags {
        return;
    }
    let mut open = true;
    crate::theme::window("Tags")
        .beside_model()
        .open(&mut open)
        .default_width(230.0)
        .resizable(true)
        .vscroll(true)
        .show(ctx, |ui| {
            let selected: Vec<InstanceId> = app
                .editor
                .selected()
                .into_iter()
                .filter_map(|e| if let Element::Instance(i) = e { Some(i) } else { None })
                .collect();
            let tags: Vec<_> = app.editor.doc().tags.iter().map(|(id, t)| (*id, t.clone())).collect();
            egui::Grid::new("tags-grid").num_columns(3).show(ui, |ui| {
                ui.label("");
                ui.label("Untagged");
                if ui.add_enabled(!selected.is_empty(), egui::Button::new("Assign")).clicked() {
                    apply(app, Command::AssignTag { instances: selected.clone(), tag: None });
                }
                ui.end_row();
                for (id, tag) in tags {
                    let mut visible = tag.visible;
                    if crate::widgets::checkbox(ui, &mut visible, "").on_hover_text("Visible").changed() {
                        apply(app, Command::SetTag { tag: id, name: None, visible: Some(visible) });
                    }
                    ui.label(&tag.name);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!selected.is_empty(), egui::Button::new("Assign")).clicked() {
                            apply(app, Command::AssignTag { instances: selected.clone(), tag: Some(id) });
                        }
                        if ui.small_button("Delete").clicked() {
                            apply(app, Command::DeleteTag { tag: id });
                        }
                    });
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(crate::theme::singleline(&mut app.new_tag).hint_text("New tag").desired_width(130.0));
                if ui.button("Add").clicked() && !app.new_tag.trim().is_empty() {
                    let name = std::mem::take(&mut app.new_tag);
                    apply(app, Command::AddTag { name });
                }
            });
            ui.label("Tags apply to groups and components.");
        });
    app.show_tags = open;
}

fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, value: &mut T, options: &[(T, &str)]) {
    let selected = options.iter().find(|(v, _)| v == value).map(|(_, name)| *name).unwrap_or("");
    crate::widgets::combo(egui::ComboBox::from_id_salt(label)).selected_text(selected).show_ui(ui, |ui| {
        for (option, name) in options {
            ui.selectable_value(value, *option, *name);
        }
    });
}

pub fn settings(ctx: &egui::Context, app: &mut App3d) {
    use rysmo_model::{EdgeColor, FaceStyle, Style};
    if !app.show_settings {
        app.measurement_edit_revision = None;
        return;
    }
    if app.settings_tab != 1 {
        app.measurement_edit_revision = None;
    }
    let mut select_text = None;
    let mut update_text = None;
    let mut update_dimensions = None;
    let mut select_dimensions = false;
    let before = app.editor.doc().style;
    let mut style = before;
    let mut open = true;
    let window = crate::theme::window("Settings")
        .open(&mut open)
        .default_size(egui::vec2(680., 520.))
        .max_height((ctx.content_rect().height() - 160.).max(180.))
        .min_size(egui::vec2(400., 240.))
        .resizable(true)
        .vscroll(true)
        .show(ctx, |ui| {
            ui.add(
                crate::theme::singleline(&mut app.settings_filter)
                    .hint_text("Find a category: shadows, units, mouse, layout...")
                    .desired_width(f32::INFINITY),
            );
            let query = app.settings_filter.to_lowercase();
            ui.horizontal_wrapped(|ui| {
                for (index, label, keywords) in [
                    (3, "Application", "startup welcome recovery auto-save power sleep files"),
                    (4, "Controls", "mouse navigation zoom orbit pan keyboard touch shortcuts"),
                    (5, "Workspace", "layout inspector windows tool options"),
                    (0, "Appearance", "renderer lighting shadows quality textures display performance"),
                    (1, "Model units", "units measurements precision snapping length area volume angle"),
                    (2, "Annotations", "text dimensions labels font"),
                ] {
                    if (query.is_empty() || format!("{label} {keywords}").to_lowercase().contains(&query))
                        && crate::icons::tab(ui, label, app.settings_tab == index).clicked()
                    {
                        app.settings_tab = index;
                    }
                }
            });
            ui.separator();
            if app.settings_tab >= 3 {
                crate::tool_panel::application_settings(ui, app, ctx);
                return;
            }
            if app.settings_tab == 1 {
                ui.heading("Editor");
                let before_units = app.editor.doc().units;
                let mut units = before_units;
                let choices = [
                    (rysmo_model::Units::Millimetres, "Millimetres"),
                    (rysmo_model::Units::Centimetres, "Centimetres"),
                    (rysmo_model::Units::Metres, "Metres"),
                    (rysmo_model::Units::Inches, "Decimal inches"),
                    (rysmo_model::Units::Feet, "Engineering (decimal feet)"),
                    (rysmo_model::Units::Architectural, "Architectural (feet and inches)"),
                    (rysmo_model::Units::Fractional, "Fractional inches"),
                ];
                crate::widgets::combo(egui::ComboBox::from_label("Project units"))
                    .selected_text(choices.iter().find(|(value, _)| *value == units).unwrap().1)
                    .show_ui(ui, |ui| {
                        for (value, label) in choices {
                            ui.selectable_value(&mut units, value, label);
                        }
                    });
                if units != before_units {
                    apply(app, Command::SetUnits { units });
                }
                ui.separator();
                let before = app.editor.doc().length_precision;
                let mut precision = before;
                crate::widgets::combo(egui::ComboBox::from_label("Length precision"))
                    .selected_text(precision.map_or_else(|| "Automatic".into(), |p| units.precision_label(p)))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut precision, None, "Automatic");
                        for p in 0..=6 {
                            ui.selectable_value(&mut precision, Some(p), units.precision_label(p));
                        }
                    });
                if precision != before {
                    apply(app, Command::SetLengthPrecision { precision });
                }
                let before = app.editor.doc().measurement;
                let mut settings = before;
                let mut editing = false;
                ui.add_enabled(
                    !units.is_fractional(),
                    crate::widgets::Check::new(&mut settings.show_unit_markers, "Display decimal unit markers"),
                );
                if units == rysmo_model::Units::Architectural {
                    crate::widgets::checkbox(ui, &mut settings.force_zero_inches, "Force display of zero inches");
                }
                crate::widgets::combo(egui::ComboBox::from_label("Area units"))
                    .selected_text(
                        settings
                            .area_units
                            .map_or_else(|| "Automatic (m² / ft²)".into(), |u| format!("{}²", u.suffix())),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut settings.area_units, None, "Automatic (m² / ft²)");
                        for unit in [
                            rysmo_model::Units::Millimetres,
                            rysmo_model::Units::Centimetres,
                            rysmo_model::Units::Metres,
                            rysmo_model::Units::Inches,
                            rysmo_model::Units::Feet,
                        ] {
                            ui.selectable_value(&mut settings.area_units, Some(unit), format!("{}²", unit.suffix()));
                        }
                    });
                crate::theme::field(ui, "Area decimal places", |ui| {
                    ui.add(crate::widgets::Number::new(&mut settings.area_precision).range(0..=6));
                });
                crate::widgets::combo(egui::ComboBox::from_label("Volume units"))
                    .selected_text(
                        settings
                            .volume_units
                            .map_or_else(|| "Automatic (m³ / ft³)".into(), |u| format!("{}³", u.suffix())),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut settings.volume_units, None, "Automatic (m³ / ft³)");
                        for unit in [
                            rysmo_model::Units::Millimetres,
                            rysmo_model::Units::Centimetres,
                            rysmo_model::Units::Metres,
                            rysmo_model::Units::Inches,
                            rysmo_model::Units::Feet,
                        ] {
                            ui.selectable_value(&mut settings.volume_units, Some(unit), format!("{}³", unit.suffix()));
                        }
                    });
                crate::theme::field(ui, "Volume decimal places", |ui| {
                    ui.add(crate::widgets::Number::new(&mut settings.volume_precision).range(0..=6));
                });
                let mut length_snap = settings.length_snap.is_some();
                if crate::widgets::checkbox(ui, &mut length_snap, "Enable length snapping").changed() {
                    settings.length_snap = length_snap.then_some(1. / units.per_metre());
                }
                if let Some(step) = &mut settings.length_snap {
                    let mut shown = *step * units.per_metre();
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.label("Length increment");
                        let response = ui.add(
                            crate::widgets::Number::new(&mut shown)
                                .range(1e-9 * units.per_metre()..=1e12 * units.per_metre())
                                .max_decimals(12)
                                .speed(0.1)
                                .suffix(format!(" {}", units.suffix())),
                        );
                        editing |= response.dragged() || response.has_focus();
                        changed = response.changed();
                    });
                    if changed {
                        *step = shown / units.per_metre();
                    }
                }
                crate::widgets::combo(egui::ComboBox::from_label("Angle precision"))
                    .selected_text(format!("{} decimal places", settings.angle_precision))
                    .show_ui(ui, |ui| {
                        for p in 0..=6 {
                            ui.selectable_value(&mut settings.angle_precision, p, format!("{p} decimal places"));
                        }
                    });
                let mut angle_snap = settings.angle_snap.is_some();
                if crate::widgets::checkbox(ui, &mut angle_snap, "Enable angle snapping").changed() {
                    settings.angle_snap = angle_snap.then_some(15.);
                }
                if let Some(step) = &mut settings.angle_snap {
                    ui.horizontal(|ui| {
                        ui.label("Angle increment");
                        let response =
                            ui.add(crate::widgets::Number::new(step).range(0.1..=180.).speed(0.5).suffix("°"));
                        editing |= response.dragged() || response.has_focus();
                    });
                }
                if settings != before {
                    let command = Command::SetMeasurementSettings { settings };
                    let result = if app.measurement_edit_revision == Some(app.editor.revision())
                        && app.editor.undo_label() == Some("Measurement settings")
                    {
                        app.editor.amend_last(command)
                    } else {
                        app.editor.apply(command)
                    };
                    if let Err(error) = result {
                        app.message = Some(error.to_string());
                    }
                    app.measurement_edit_revision = editing.then_some(app.editor.revision());
                }
                if !editing {
                    app.measurement_edit_revision = None;
                }
                ui.weak(
                    "Display precision does not round model geometry. Explicit unit suffixes override project units.",
                );
                ui.label("Examples: 125cm, 24in, 5' 3 1/2\"");
                ui.separator();
                return;
            }
            if app.settings_tab == 2 {
                crate::widgets::section(ui, "Text: defaults", true, |ui| {
                    use rysmo_model::text::{TextEndpoint as E, TextLeaderKind as L};
                    let defaults = &mut style.text_defaults;
                    for (screen, font) in [(true, &mut defaults.screen_font), (false, &mut defaults.leader_font)] {
                        ui.label(if screen { "Screen text" } else { "Attached text" });
                        crate::theme::field(ui, "Font size", |ui| {
                            ui.add(crate::widgets::Number::new(&mut font.size).range(1.0..=512.0).suffix(" px"));
                        });
                        crate::widgets::checkbox(ui, &mut font.monospace, "Monospace font");
                        ui.color_edit_button_srgb(&mut font.color);
                        if ui
                            .button(if screen { "Select all screen text" } else { "Select all attached text" })
                            .clicked()
                        {
                            select_text = Some(screen);
                        }
                        ui.separator();
                    }
                    ui.label("Leader type for new attached text");
                    choice(
                        ui,
                        "text-default-leader",
                        &mut defaults.leader,
                        &[(L::None, "None"), (L::ViewBased, "View based"), (L::Pushpin, "Pushpin")],
                    );
                    ui.label("Leader endpoint");
                    choice(
                        ui,
                        "text-default-endpoint",
                        &mut defaults.endpoint,
                        &[
                            (E::None, "None"),
                            (E::Dot, "Dot"),
                            (E::ClosedArrow, "Closed arrow"),
                            (E::OpenArrow, "Open arrow"),
                        ],
                    );
                    let ids = app
                        .editor
                        .selected()
                        .iter()
                        .filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None })
                        .collect::<Vec<_>>();
                    if ui.add_enabled(!ids.is_empty(), egui::Button::new("Update selected text")).clicked() {
                        update_text = Some(ids);
                    }
                    ui.small("Updates font, colour, leader and endpoint in the current editing context.");
                });
                crate::widgets::section(ui, "Dimensions: defaults", true, |ui| {
                    use rysmo_model::dimensions::{DimensionEndpoints as E, DimensionTextAlignment as A};
                    let defaults = &mut style.dimension_defaults;
                    crate::theme::field(ui, "Font size", |ui| {
                        ui.add(crate::widgets::Number::new(&mut defaults.font.size).range(1.0..=512.0).suffix(" px"));
                    });
                    crate::widgets::checkbox(ui, &mut defaults.font.monospace, "Monospace font");
                    ui.color_edit_button_srgb(&mut defaults.font.color);
                    choice(
                        ui,
                        "dimension-default-alignment",
                        &mut defaults.alignment,
                        &[(A::Screen, "Align to screen"), (A::DimensionLine, "Align to dimension line")],
                    );
                    choice(
                        ui,
                        "dimension-default-endpoints",
                        &mut defaults.endpoints,
                        &[
                            (E::None, "None"),
                            (E::Slash, "Slash"),
                            (E::Dot, "Dot"),
                            (E::ClosedArrow, "Closed arrow"),
                            (E::OpenArrow, "Open arrow"),
                        ],
                    );
                    ui.small("Defaults apply to new dimensions. These buttons work in the editing context.");
                    select_dimensions = ui.button("Select all dimensions").clicked();
                    let ids = app
                        .editor
                        .selection()
                        .iter()
                        .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
                        .collect::<Vec<_>>();
                    if ui.add_enabled(!ids.is_empty(), egui::Button::new("Update selected dimensions")).clicked() {
                        update_dimensions = Some(ids);
                    }
                });
                crate::widgets::section(ui, "Dimensions: visibility", true, |ui| {
                    crate::widgets::checkbox(ui, &mut style.dimensions.hide_foreshortened, "Hide when foreshortened");
                    ui.add_enabled(
                        style.dimensions.hide_foreshortened,
                        crate::widgets::Slider::new(&mut style.dimensions.angle_degrees, 0.0..=90.0)
                            .text("Minimum angle (degrees)"),
                    );
                    crate::widgets::checkbox(ui, &mut style.dimensions.hide_small, "Hide when too small");
                    ui.add_enabled(
                        style.dimensions.hide_small,
                        crate::widgets::Slider::new(&mut style.dimensions.minimum_pixels, 0.0..=4096.0)
                            .logarithmic(true)
                            .text("Minimum line size (px)"),
                    );
                    crate::widgets::checkbox(
                        ui,
                        &mut style.dimensions.highlight_non_associated,
                        "Highlight non-associated dimensions",
                    );
                    ui.add_enabled_ui(style.dimensions.highlight_non_associated, |ui| {
                        ui.color_edit_button_srgb(&mut style.dimensions.highlight_color);
                    });
                });
            } else {
                renderer_controls(ui, &mut style);
                crate::widgets::section(ui, "Display", false, |ui| {
                    crate::widgets::checkbox(ui, &mut style.show_hidden_geometry, "Show hidden geometry");
                    ui.label("Faces");
                    choice(
                        ui,
                        "face-style",
                        &mut style.faces,
                        &[
                            (FaceStyle::Textured, "Textured"),
                            (FaceStyle::Shaded, "Shaded"),
                            (FaceStyle::Monochrome, "Monochrome"),
                            (FaceStyle::HiddenLine, "Hidden Line"),
                            (FaceStyle::Wireframe, "Wireframe"),
                            (FaceStyle::XRay, "X-ray"),
                        ],
                    );
                    if style.faces == FaceStyle::XRay {
                        ui.add(crate::widgets::Slider::new(&mut style.xray_opacity, 0.05..=1.0).text("Face opacity"));
                    }
                    crate::widgets::checkbox(ui, &mut style.edges, "Edges");
                    crate::widgets::checkbox(ui, &mut style.profiles, "Profiles");
                    ui.label("Edge colour");
                    choice(
                        ui,
                        "edge-color",
                        &mut style.edge_color,
                        &[
                            (EdgeColor::Black, "Black"),
                            (EdgeColor::ByMaterial, "By material"),
                            (EdgeColor::ByAxis, "By axis"),
                        ],
                    );
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Background");
                        ui.color_edit_button_rgb(&mut style.background);
                    });
                    ui.horizontal(|ui| {
                        crate::widgets::checkbox(ui, &mut style.sky, "Sky");
                        ui.color_edit_button_rgb(&mut style.sky_color);
                    });
                    ui.horizontal(|ui| {
                        crate::widgets::checkbox(ui, &mut style.ground, "Ground");
                        ui.color_edit_button_rgb(&mut style.ground_color);
                    });
                    ui.horizontal(|ui| {
                        crate::widgets::checkbox(ui, &mut style.fog, "Fog");
                        ui.add_enabled(
                            style.fog,
                            crate::widgets::Number::new(&mut style.fog_distance).range(10. ..=100_000.).suffix(" m"),
                        )
                        .on_hover_text("Distance at which the fog hides the model");
                    });
                    ui.separator();
                    crate::widgets::checkbox(ui, &mut style.section_planes, "Section planes");
                    crate::widgets::checkbox(ui, &mut style.section_cuts, "Section cuts");
                    crate::widgets::checkbox(ui, &mut style.section_lines, "Section lines");
                });
            }
            if ui.button("Reset appearance and annotation defaults").clicked() {
                style = Style::default();
            }
        });
    if let Some(window) = window {
        let r = window.response.rect;
        app.ui_rects.push((
            rysmo_math::DVec2::new(r.min.x as f64, r.min.y as f64),
            rysmo_math::DVec2::new(r.max.x as f64, r.max.y as f64),
        ));
    }
    if style != before {
        let result =
            if app.style_edit_revision == Some(app.editor.revision()) && app.editor.undo_label() == Some("Style") {
                app.editor.amend_last(Command::SetStyle { style })
            } else {
                app.editor.apply(Command::SetStyle { style })
            };
        match result {
            Ok(_) => {
                app.style_edit_revision = Some(app.editor.revision());
                app.message = None;
            }
            Err(error) => app.message = Some(error.to_string()),
        }
    }
    if !ctx.input(|i| i.pointer.any_down()) || !open {
        app.style_edit_revision = None;
    }
    if let Some(screen) = select_text {
        use rysmo_model::text::TextPlacement;
        let elements = app
            .editor
            .active()
            .texts
            .iter()
            .filter(|(_, text)| matches!(text.placement, TextPlacement::Screen { .. }) == screen)
            .map(|(id, _)| Element::Text(*id))
            .collect::<Vec<_>>();
        app.editor.set_selection(elements);
    }
    if let Some(ids) = update_text {
        apply(
            app,
            Command::ApplyTextDefaults {
                ids,
                camera: app.camera,
                viewport: app.viewport,
                transform: app.editor.context_transform(),
            },
        );
    }
    if select_dimensions {
        app.editor
            .set_selection(app.editor.active().dimensions.keys().copied().map(Element::Dimension).collect::<Vec<_>>());
    }
    if let Some(ids) = update_dimensions {
        apply(app, Command::ApplyDimensionDefaults { ids });
    }
    app.show_settings = open;
    if !open {
        app.measurement_edit_revision = None;
    }
}

#[cfg(test)]
pub fn entity_info(ctx: &egui::Context, app: &mut App3d) {
    crate::theme::window("Entity Info").beside_model().default_width(300.).show(ctx, |ui| entity_info_ui(ui, app));
}

pub fn entity_info_ui(ui: &mut egui::Ui, app: &mut App3d) {
    let ctx = ui.ctx().clone();
    if !app.show_entity_info {
        return;
    }
    let selected = app.editor.selected();
    let text_info = if let [Element::Text(id)] = selected.as_slice() {
        app.editor.active().texts.get(id).cloned().map(|text| (*id, text))
    } else {
        None
    };
    if text_info.is_none() {
        app.text_edit = None;
    }
    let dimension_info = if let [Element::Dimension(id)] = selected.as_slice() {
        app.editor.active().dimensions.get(id).cloned().and_then(|dimension| {
            let layout = dimension.layout(
                app.editor.doc(),
                app.editor.active_definition(),
                app.editor.context_transform(),
                0.0,
            )?;
            Some((*id, dimension, layout.measurement))
        })
    } else {
        None
    };
    if dimension_info.is_none() {
        app.dimension_text_edit = None;
    }
    if app.texture_session.is_some() {
        ui.label("Texture Position is active in the model. Use Done or Cancel to finish.");
        return;
    }
    let mesh = app.editor.mesh();
    let faces: Vec<_> =
        selected.iter().filter_map(|e| if let Element::Face(id) = e { Some(*id) } else { None }).collect();
    let instances: Vec<_> =
        selected.iter().filter_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None }).collect();
    let mut edges = std::collections::BTreeSet::new();
    for item in &selected {
        match item {
            Element::Edge(id) => {
                edges.insert(*id);
            }
            Element::Face(id) => {
                edges.extend(mesh.face_edges(*id));
            }
            _ => {}
        }
    }
    let edges: Vec<_> = edges.into_iter().collect();
    let units = app.editor.doc().units;
    let length: f64 = edges
        .iter()
        .map(|id| {
            let (a, b) = mesh.edge_points(*id);
            a.distance(b)
        })
        .sum();
    let area: f64 =
        faces.iter().map(|id| app.editor.doc().effective_mesh(app.editor.active_definition()).face_area(*id)).sum();
    let placement = faces.first().and_then(|id| mesh.face(*id)).map(|f| f.uv.unwrap_or_default());
    let selected_curves: std::collections::BTreeSet<_> =
        edges.iter().filter_map(|id| mesh.edge(*id).and_then(|e| e.curve)).collect();
    let curve = if selected_curves.len() == 1 {
        let id = *selected_curves.first().unwrap();
        mesh.curve(id).map(|c| (id, *c))
    } else {
        None
    };
    let flags: Vec<_> = edges.iter().filter_map(|id| mesh.edge(*id)).map(|e| (e.soft, e.smooth, e.hidden)).collect();
    let open = true;
    let mut command = None;
    let mut text_style_edit = false;
    ui.scope(|ui| {
        if selected.is_empty() {
            ui.label("Select a face, edge, group, dimension or text.");
            return;
        }
        if let Some((id, original)) = &text_info {
            use rysmo_model::text::{TextEndpoint, TextLeader, TextLeaderKind, TextPlacement};
            ui.heading("Text");
            ui.label(match &original.placement {
                TextPlacement::Screen { .. } => "Screen text (fixed on screen)",
                TextPlacement::Attached { leader: TextLeader::ViewBased { .. }, .. } => "View-based leader",
                TextPlacement::Attached { leader: TextLeader::Pushpin { .. }, .. } => "Pushpin leader (model space)",
                TextPlacement::Attached { leader: TextLeader::None { .. }, .. } => "Attached text without a leader",
            });
            if let TextPlacement::Attached { anchor, .. } = &original.placement {
                ui.label(if anchor.resolve(app.editor.doc(), app.editor.active_definition()).1 {
                    "Associated with geometry"
                } else {
                    "Not associated with geometry"
                });
            }
            if app.text_edit.as_ref().is_none_or(|(cached, base, _)| cached != id || base != &original.content) {
                app.text_edit = Some((*id, original.content.clone(), original.content.clone()));
            }
            let (_, _, draft) = app.text_edit.as_mut().unwrap();
            ui.label("Content");
            let response = ui.add(crate::theme::multiline(draft).id(egui::Id::unique(("annotation-text", id.0))));
            ui.small("Enter adds a line. Ctrl+Enter applies the text.");
            let submit = response.has_focus() && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
            let mut text = original.clone();
            let content_applied = ui.button("Apply text").clicked() || submit;
            if content_applied {
                text.content = draft.clone();
                command = Some(Command::SetText { id: *id, text: text.clone() });
            }
            if ui.button("Discard text changes").clicked()
                || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                *draft = original.content.clone();
            }
            let previous_font = text.font;
            crate::theme::field(ui, "Font size", |ui| {
                ui.add(crate::widgets::Number::new(&mut text.font.size).range(1.0..=512.0).speed(0.5).suffix(" px"));
            });
            crate::widgets::checkbox(ui, &mut text.font.monospace, "Monospace font");
            ui.horizontal(|ui| {
                ui.label("Colour");
                ui.color_edit_button_srgb(&mut text.font.color);
            });
            let mut leader_changed = false;
            if let TextPlacement::Attached { leader, .. } = &text.placement {
                let original_kind = match leader {
                    TextLeader::None { .. } => TextLeaderKind::None,
                    TextLeader::ViewBased { .. } => TextLeaderKind::ViewBased,
                    TextLeader::Pushpin { .. } => TextLeaderKind::Pushpin,
                };
                let mut kind = original_kind;
                crate::widgets::combo(egui::ComboBox::from_label("Leader type"))
                    .selected_text(match kind {
                        TextLeaderKind::None => "None",
                        TextLeaderKind::ViewBased => "View based",
                        TextLeaderKind::Pushpin => "Pushpin",
                    })
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            (TextLeaderKind::None, "None"),
                            (TextLeaderKind::ViewBased, "View based"),
                            (TextLeaderKind::Pushpin, "Pushpin"),
                        ] {
                            ui.selectable_value(&mut kind, value, label);
                        }
                    });
                if kind != original_kind {
                    if let Some(converted) = text.with_leader(
                        app.editor.doc(),
                        app.editor.active_definition(),
                        app.editor.context_transform(),
                        &app.view(),
                        kind,
                    ) {
                        text = converted;
                        leader_changed = true;
                    } else {
                        app.message = Some("Cannot convert this leader in the current view.".into());
                    }
                }
            }
            let mut offset_changed = false;
            if let TextPlacement::Screen { position } = &mut text.placement {
                let mut pixels = *position * app.viewport;
                for (index, axis) in ["X", "Y"].iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Screen position {axis} (px)"));
                        offset_changed |= ui.add(crate::widgets::Number::new(&mut pixels[index]).speed(1.)).changed();
                    });
                }
                if offset_changed {
                    *position = pixels / app.viewport;
                }
            }
            if let TextPlacement::Attached { leader, .. } = &mut text.placement {
                match leader {
                    TextLeader::ViewBased { offset } => {
                        for (index, label) in ["Label offset X (px)", "Label offset Y (px)"].iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(*label);
                                offset_changed |=
                                    ui.add(crate::widgets::Number::new(&mut offset[index]).speed(1.)).changed();
                            });
                        }
                    }
                    TextLeader::Pushpin { offset } | TextLeader::None { offset } => {
                        let basis = app.editor.active().axes.basis();
                        let mut values =
                            basis.inverse().transform_vector3(*offset).to_array().map(|v| v * units.per_metre());
                        for (index, axis) in ["Red", "Green", "Blue"].iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("Label offset {axis} ({})", units.suffix()));
                                offset_changed |= ui
                                    .add(
                                        crate::widgets::Number::new(&mut values[index]).speed(0.01 * units.per_metre()),
                                    )
                                    .changed();
                            });
                        }
                        if offset_changed {
                            *offset =
                                basis.transform_vector3(rysmo_math::DVec3::from_array(values) / units.per_metre());
                        }
                    }
                }
            }
            let previous_endpoint = text.endpoint;
            if matches!(
                text.placement,
                TextPlacement::Attached { leader: TextLeader::ViewBased { .. } | TextLeader::Pushpin { .. }, .. }
            ) {
                crate::widgets::combo(egui::ComboBox::from_label("Leader endpoint"))
                    .selected_text(match text.endpoint {
                        TextEndpoint::None => "None",
                        TextEndpoint::Dot => "Dot",
                        TextEndpoint::ClosedArrow => "Closed arrow",
                        TextEndpoint::OpenArrow => "Open arrow",
                    })
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            (TextEndpoint::None, "None"),
                            (TextEndpoint::Dot, "Dot"),
                            (TextEndpoint::ClosedArrow, "Closed arrow"),
                            (TextEndpoint::OpenArrow, "Open arrow"),
                        ] {
                            ui.selectable_value(&mut text.endpoint, value, label);
                        }
                    });
            }
            if text.font != previous_font || text.endpoint != previous_endpoint || leader_changed || offset_changed {
                text_style_edit = !content_applied;
                command = Some(Command::SetText { id: *id, text });
            }
            return;
        }
        if let Some((id, original, measurement)) = &dimension_info {
            ui.heading("Dimension");
            let mut automatic = original.clone();
            automatic.text.clear();
            ui.label(format!("Measured value: {}", automatic.label(measurement, units)));
            ui.label(if measurement.associated { "Associated with geometry" } else { "Not associated with geometry" });
            if app.dimension_text_edit.as_ref().is_none_or(|(cached, base, _)| cached != id || base != &original.text) {
                app.dimension_text_edit = Some((*id, original.text.clone(), original.text.clone()));
            }
            let mut dimension = original.clone();
            let (_, _, draft) = app.dimension_text_edit.as_mut().unwrap();
            ui.label("Text");
            let text_id = egui::Id::unique(("dimension-text", id.0));
            let response = ui.add(crate::theme::multiline(draft).id(text_id));
            ui.small(
                "<> inserts the current measurement. Empty text uses the automatic value. Ctrl+Enter applies text.",
            );
            let submit =
                response.has_focus() && ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::Enter));
            let content_applied = ui.button("Apply text").clicked() || submit;
            if content_applied {
                dimension.text = draft.clone();
                command = Some(Command::SetDimension { id: *id, dimension: dimension.clone() });
            }
            if ui.button("Use automatic text").clicked() {
                dimension.text.clear();
                command = Some(Command::SetDimension { id: *id, dimension: dimension.clone() });
            }
            use rysmo_model::dimensions::DimensionTextAlignment;
            ui.label("Text alignment");
            let alignment_changed = crate::widgets::radio_value(
                ui,
                &mut dimension.alignment,
                DimensionTextAlignment::Screen,
                "Align to screen",
            )
            .changed()
                | crate::widgets::radio_value(
                    ui,
                    &mut dimension.alignment,
                    DimensionTextAlignment::DimensionLine,
                    "Align to dimension line",
                )
                .changed();
            if alignment_changed {
                command = Some(Command::SetDimension { id: *id, dimension: dimension.clone() });
            }
            let previous_font = dimension.font;
            crate::theme::field(ui, "Font size", |ui| {
                ui.add(
                    crate::widgets::Number::new(&mut dimension.font.size).range(1.0..=512.0).speed(0.5).suffix(" px"),
                );
            });
            crate::widgets::checkbox(ui, &mut dimension.font.monospace, "Monospace font");
            ui.horizontal(|ui| {
                ui.label("Colour");
                ui.color_edit_button_srgb(&mut dimension.font.color);
            });
            if dimension.font != previous_font {
                command = Some(Command::SetDimension { id: *id, dimension: dimension.clone() });
            }
            use rysmo_model::dimensions::DimensionEndpoints;
            let previous = dimension.endpoints;
            crate::widgets::combo(egui::ComboBox::from_label("Endpoints"))
                .selected_text(match previous {
                    DimensionEndpoints::None => "None",
                    DimensionEndpoints::Slash => "Slash",
                    DimensionEndpoints::Dot => "Dot",
                    DimensionEndpoints::ClosedArrow => "Closed arrow",
                    DimensionEndpoints::OpenArrow => "Open arrow",
                })
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (DimensionEndpoints::None, "None"),
                        (DimensionEndpoints::Slash, "Slash"),
                        (DimensionEndpoints::Dot, "Dot"),
                        (DimensionEndpoints::ClosedArrow, "Closed arrow"),
                        (DimensionEndpoints::OpenArrow, "Open arrow"),
                    ] {
                        ui.selectable_value(&mut dimension.endpoints, value, label);
                    }
                });
            if dimension.endpoints != previous {
                command = Some(Command::SetDimension { id: *id, dimension: dimension.clone() });
            }
            if matches!(dimension.measure, rysmo_model::dimensions::DimensionMeasure::Linear { .. }) {
                use rysmo_model::dimensions::DimensionTextPosition;
                ui.label("Text position");
                let changed = crate::widgets::radio_value(
                    ui,
                    &mut dimension.text_position,
                    DimensionTextPosition::OutsideStart,
                    "Outside start",
                )
                .changed()
                    | crate::widgets::radio_value(
                        ui,
                        &mut dimension.text_position,
                        DimensionTextPosition::Centered,
                        "Centered",
                    )
                    .changed()
                    | crate::widgets::radio_value(
                        ui,
                        &mut dimension.text_position,
                        DimensionTextPosition::OutsideEnd,
                        "Outside end",
                    )
                    .changed();
                if changed {
                    command = Some(Command::SetDimension { id: *id, dimension: dimension.clone() });
                }
            }
            if let rysmo_model::dimensions::DimensionMeasure::Radial { diameter, .. } = &mut dimension.measure {
                let changed = crate::widgets::radio_value(ui, diameter, false, "Radius").changed()
                    | crate::widgets::radio_value(ui, diameter, true, "Diameter").changed();
                if changed {
                    command = Some(Command::SetDimension { id: *id, dimension });
                }
            }
            return;
        }
        ui.label(format!("{} selected: {} faces, {} groups/components", selected.len(), faces.len(), instances.len()));
        let sections = app
            .editor
            .selected()
            .into_iter()
            .filter_map(|e| if let rysmo_model::Element::Section(id) = e { Some(id) } else { None })
            .collect::<Vec<_>>();
        if !sections.is_empty() {
            for id in sections {
                if let Some(plane) = app.editor.active().sections.planes.get(&id) {
                    ui.label(format!("Section Plane: {}", plane.name));
                }
            }
            if ui.button("Section Plane properties").clicked() {
                app.show_sections = true;
            }
            ui.weak("Move and Rotate position the selected section plane.");
        }
        if !edges.is_empty() {
            ui.label(format!("Edge length: {}", app.editor.doc().format_length(length)));
        }
        if !faces.is_empty() {
            ui.label(format!("Face area: {}", app.editor.doc().format_area(area)));
        }
        let volume = if instances.len() == 1 {
            Some(app.volume_cache.get(&app.editor, app.document_generation, instances[0]))
        } else {
            None
        };
        if instances.len() == 1
            && let Some(inst) = app.editor.active().instances.get(&instances[0])
        {
            ui.label(format!("Name: {}", inst.name));
            match volume.unwrap() {
                Ok(volume) => {
                    ui.label(format!("Volume: {}", app.editor.doc().format_volume(volume)))
                        .on_hover_text("One closed, consistently oriented shell. Self-intersections are not checked.");
                }
                Err(reason) => {
                    ui.weak("Volume unavailable").on_hover_text(reason);
                }
            }

            if ui.button("Rename instance").clicked() {
                let mut path = app.editor.context().to_vec();
                path.push(instances[0]);
                app.outliner_rename = Some((path, inst.name.clone()));
            }
            if let Some((lo, hi)) = app.editor.doc().definition_bounds(Some(inst.definition)) {
                // Transform all corners to report the axis-aligned bounds in this context.
                let points = rysmo_model::corners(lo, hi).map(|p| inst.transform.transform_point3(p));
                let lo = points.iter().copied().reduce(|a, b| a.min(b)).unwrap();
                let hi = points.iter().copied().reduce(|a, b| a.max(b)).unwrap();
                let size = hi - lo;
                ui.label(format!(
                    "Size: {} × {} × {}",
                    app.editor.doc().format_length(size.x),
                    app.editor.doc().format_length(size.y),
                    app.editor.doc().format_length(size.z)
                ));
            }
        }
        if !edges.is_empty() {
            let mut material = app.editor.mesh().edge(edges[0]).and_then(|edge| edge.material);
            let previous = material;
            ui.label("Edge material");
            let name = material
                .and_then(|id| app.editor.doc().materials.get(&id))
                .map(|m| m.name.as_str())
                .unwrap_or("Default");
            crate::widgets::combo(egui::ComboBox::from_id_salt("edge-material")).selected_text(name).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut material, None, "Default");
                    for (id, mat) in &app.editor.doc().materials {
                        ui.selectable_value(&mut material, Some(*id), &mat.name);
                    }
                },
            );
            if material != previous {
                apply(
                    app,
                    Command::PaintElements {
                        elements: edges.iter().copied().map(Element::Edge).collect(),
                        material,
                        back: false,
                    },
                );
            }
            if app.editor.doc().style.edge_color != rysmo_model::EdgeColor::ByMaterial {
                ui.small("Show edge colors with By material in Settings > Appearance > Display.");
            }
        }
        if !faces.is_empty() || !instances.is_empty() {
            let mut material =
                faces.first().and_then(|id| app.editor.mesh().face(*id)).and_then(|f| f.material).or_else(|| {
                    instances.first().and_then(|id| app.editor.active().instances.get(id)).and_then(|i| i.material)
                });
            let previous = material;
            ui.label("Front material");
            let name = material
                .and_then(|id| app.editor.doc().materials.get(&id))
                .map(|m| m.name.as_str())
                .unwrap_or("Default");
            crate::widgets::combo(egui::ComboBox::from_id_salt("entity-material")).selected_text(name).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut material, None, "Default");
                    for (id, mat) in &app.editor.doc().materials {
                        ui.selectable_value(&mut material, Some(*id), &mat.name);
                    }
                },
            );
            if material != previous {
                if !faces.is_empty() {
                    apply(app, Command::Paint { faces: faces.clone(), material, back: false });
                }
                if !instances.is_empty() {
                    apply(app, Command::PaintInstances { instances: instances.clone(), material });
                }
            }
        }
        if !faces.is_empty() {
            // A face drawn on the ground shows its back upwards, so its paint is often there.
            let mut material = faces.first().and_then(|id| app.editor.mesh().face(*id)).and_then(|f| f.back_material);
            let previous = material;
            ui.label("Back material");
            let name = material
                .and_then(|id| app.editor.doc().materials.get(&id))
                .map(|m| m.name.as_str())
                .unwrap_or("Default");
            crate::widgets::combo(egui::ComboBox::from_id_salt("entity-back-material")).selected_text(name).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut material, None, "Default");
                    for (id, mat) in &app.editor.doc().materials {
                        ui.selectable_value(&mut material, Some(*id), &mat.name);
                    }
                },
            );
            if material != previous {
                apply(app, Command::Paint { faces: faces.clone(), material, back: true });
            }
        }
        if !instances.is_empty() {
            let tags: Vec<Option<rysmo_model::TagId>> =
                instances.iter().filter_map(|id| app.editor.active().instances.get(id)).map(|i| i.tag).collect();
            let mixed = tags.windows(2).any(|pair| pair[0] != pair[1]);
            let current = tags.first().copied().flatten();
            let mut tag = current;
            ui.label("Tag");
            let name = if mixed {
                "Mixed"
            } else {
                current.and_then(|id| app.editor.doc().tags.get(&id)).map(|t| t.name.as_str()).unwrap_or("Untagged")
            };
            let mut chosen = false;
            crate::widgets::combo(egui::ComboBox::from_id_salt("entity-tag")).selected_text(name).show_ui(ui, |ui| {
                chosen |= ui.selectable_value(&mut tag, None, "Untagged").clicked();
                for (id, t) in &app.editor.doc().tags {
                    chosen |= ui.selectable_value(&mut tag, Some(*id), &t.name).clicked();
                }
            });
            if chosen && (mixed || tag != current) {
                apply(app, Command::AssignTag { instances: instances.clone(), tag });
            }
        }
        if let Some((id, curve)) = curve {
            ui.separator();
            if curve.kind == rysmo_mesh::CurveKind::Polyline {
                ui.label(format!("Deformed curve: {} segments", curve.segments));
                ui.weak("Part of it was moved, so its segments and radius are fixed.");
            } else {
                ui.label(format!("{:?}: {} segments", curve.kind, curve.segments));
                let mut segments = curve.segments;
                ui.horizontal(|ui| {
                    ui.label("Segments");
                    let min = if matches!(curve.kind, rysmo_mesh::CurveKind::Circle | rysmo_mesh::CurveKind::Polygon) {
                        3
                    } else {
                        2
                    };
                    if ui.add(crate::widgets::Number::new(&mut segments).range(min..=4096)).changed() {
                        command = Some(Command::SetCurveSegments { curve: id, segments });
                    }
                });
                let mut radius = curve.radius * units.per_metre();
                ui.horizontal(|ui| {
                    ui.label(format!("Radius ({})", units.suffix()));
                    if ui
                        .add(
                            crate::widgets::Number::new(&mut radius)
                                .speed(0.01 * units.per_metre())
                                .range(0.001 * units.per_metre()..=10000.0 * units.per_metre()),
                        )
                        .changed()
                    {
                        command = Some(Command::SetCurveRadius { curve: id, radius: radius / units.per_metre() });
                    }
                });
            }
        }
        if !edges.is_empty() {
            ui.separator();
            for (index, name) in ["Soft", "Smooth", "Hidden"].iter().enumerate() {
                let values: Vec<_> = flags
                    .iter()
                    .map(|f| match index {
                        0 => f.0,
                        1 => f.1,
                        _ => f.2,
                    })
                    .collect();
                let mut value = values.iter().all(|v| *v);
                let mixed = values.iter().any(|v| *v) && !value;
                if ui.add(crate::widgets::Check::new(&mut value, *name).indeterminate(mixed)).changed() {
                    command = Some(Command::SetEdges {
                        edges: edges.clone(),
                        soft: (index == 0).then_some(value),
                        smooth: (index == 1).then_some(value),
                        hidden: (index == 2).then_some(value),
                    });
                }
            }
            ui.add(crate::widgets::Slider::new(&mut app.soften_angle, 0.0..=180.0).text("Soften angle"));
            if ui.button("Soften/smooth by angle").clicked() {
                command = Some(Command::SoftenEdges { edges: edges.clone(), angle: app.soften_angle });
            }
        }
        if let Some(mut placement) = placement {
            ui.separator();
            ui.label("Texture position on selected faces");
            if ui.button("Position texture in model").clicked() {
                crate::texture_view::begin(app);
                return;
            }
            let factor = units.per_metre();
            let mut offsets = placement.offset.map(|v| v * factor);
            let mut changed = false;
            for (index, axis) in ["U", "V"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Offset {axis} ({})", units.suffix()));
                    changed |= ui.add(crate::widgets::Number::new(&mut offsets[index]).speed(0.01 * factor)).changed();
                });
            }
            changed |= ui
                .add(crate::widgets::Slider::new(&mut placement.rotation, -180.0..=180.0).text("Rotation (degrees)"))
                .changed();
            for (index, axis) in ["U", "V"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Scale {axis}"));
                    changed |= ui
                        .add(crate::widgets::Number::new(&mut placement.scale[index]).speed(0.01).range(0.001..=1000.0))
                        .changed();
                });
            }
            let mut projected = placement.projected_normal.is_some();
            if crate::widgets::checkbox(ui, &mut projected, "Projected texture")
                .on_hover_text("Sample with Alt+Paint Bucket, then paint other faces in this editing context.")
                .changed()
            {
                placement.projected_normal = if projected {
                    faces.first().and_then(|f| app.editor.mesh().face_normal(*f)).map(|n| n.to_array())
                } else {
                    None
                };
                changed = true;
            }
            crate::widgets::section(ui, "Texture pins", false, |ui| {
                changed |= crate::texture_pins::controls(ui, &mut placement);
            });
            if changed {
                placement.offset = offsets.map(|v| v / factor);
                command = Some(Command::SetTexturePlacement { faces: faces.clone(), placement: Some(placement) });
            }
            if ui.button("Reset texture position").clicked() {
                command = Some(Command::SetTexturePlacement { faces: faces.clone(), placement: None });
            }
        }
    });
    if let Some(command) = command {
        // Coalesce continuous parameter drags while protecting unrelated edits.
        let positioning = matches!(
            command,
            Command::SetTexturePlacement { .. } | Command::SetCurveRadius { .. } | Command::SetCurveSegments { .. }
        ) || text_style_edit;
        let label = match &command {
            Command::SetText { .. } => "Edit Text",
            Command::SetCurveRadius { .. } => "Curve Radius",
            Command::SetCurveSegments { .. } => "Curve Segments",
            _ => "Position Texture",
        };
        let segmentation = matches!(command, Command::SetCurveSegments { .. });
        let coalesced = positioning
            && app.entity_edit_revision == Some(app.editor.revision())
            && app.editor.undo_label() == Some(label);
        let depth = app.editor.undo_depth();
        let result = if coalesced { app.editor.amend_last(command) } else { app.editor.apply(command) };
        match result {
            Ok(outcome) => {
                app.message = None;
                if segmentation {
                    // The new segments join what stays selected, such as the curve's face.
                    let kept: Vec<_> = app.editor.selection().iter().copied().collect();
                    app.editor.set_selection(kept.into_iter().chain(outcome.created));
                }
                if positioning {
                    app.entity_edit_revision =
                        if coalesced && app.editor.undo_depth() < depth { None } else { Some(app.editor.revision()) };
                }
            }
            Err(error) => {
                app.message = Some(format!("{error}."));
            }
        }
    }
    if !ctx.input(|i| i.pointer.any_down()) || !open {
        app.entity_edit_revision = None;
    }
    if !open {
        app.text_edit = None;
    }
    app.show_entity_info = open;
}

pub fn resize_confirmation(ctx: &egui::Context, app: &mut App3d) {
    let Some(factor) = app.tools.pending_resize_factor() else { return };
    let mut answer = None;
    egui::Modal::new(egui::Id::unique("resize-confirmation")).frame(egui::Frame::window(&ctx.global_style())).show(
        ctx,
        |ui| {
            ui.heading("Resize model?");
            ui.label(if app.editor.context().is_empty() {
                "Resize the whole model to the measured dimension?"
            } else {
                "Resize the group/component being edited to the measured dimension?"
            });
            ui.label(format!("Scale factor: {factor:.6}"));
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    answer = Some(true);
                }
                if ui.button("No").clicked() {
                    answer = Some(false);
                }
            });
        },
    );
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(false);
    }
    if let Some(accept) = answer {
        let root = app.editor.context().is_empty();
        match app.tools.confirm_resize(&mut app.editor, accept) {
            Ok(Some(factor)) if root => {
                app.camera.target *= factor;
                app.camera.distance *= factor;
            }
            Ok(_) => {}
            Err(e) => {
                app.message = Some(e);
            }
        }
    }
}

#[cfg(test)]
mod dimension_entity_info_tests {
    use super::*;
    #[test]
    fn text_draft_is_only_committed_on_confirmation_and_undo_restores_it() {
        let document = rysmo_io::load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let mut app = App3d::new(rysmo_model::Editor::with_document(document));
        let id = rysmo_model::DimensionId(46);
        app.editor.set_selection([Element::Dimension(id)]);
        app.show_entity_info = true;
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input(), |ui| entity_info(ui.ctx(), &mut app));
        output.textures_delta.clear();
        app.dimension_text_edit.as_mut().unwrap().2 = "Size <>".into();
        let mut output = ctx.run_ui(input(), |ui| entity_info(ui.ctx(), &mut app));
        output.textures_delta.clear();
        assert!(app.editor.active().dimensions[&id].text.is_empty());
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique(("dimension-text", id.0))));
        let mut pressed = input();
        let modifiers = egui::Modifiers { ctrl: true, ..Default::default() };
        pressed.events.push(egui::Event::ModifiersChanged(modifiers));
        pressed.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        let mut output = ctx.run_ui(pressed, |ui| entity_info(ui.ctx(), &mut app));
        output.textures_delta.clear();
        assert_eq!(app.editor.active().dimensions[&id].text, "Size <>");
        assert!(app.editor.undo());
        assert!(app.editor.active().dimensions[&id].text.is_empty());
    }
}

#[cfg(test)]
mod text_entity_info_tests {
    use super::*;
    fn frame(ctx: &egui::Context, app: &mut App3d, input: egui::RawInput) {
        let mut output = ctx.run_ui(input, |ui| entity_info(ui.ctx(), app));
        output.textures_delta.clear();
    }
    #[test]
    fn draft_confirmation_cancel_selection_and_file_reset_preserve_history() {
        let fixture = include_str!("../../../testdata/text-preview.rysmo");
        let mut app = App3d::new(rysmo_model::Editor::with_document(rysmo_io::load(fixture).unwrap()));
        let id = rysmo_model::TextId(1003);
        let original = app.editor.active().texts[&id].clone();
        app.editor.set_selection([Element::Text(id)]);
        app.show_entity_info = true;
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
            ..Default::default()
        };
        frame(&ctx, &mut app, input());
        app.text_edit.as_mut().unwrap().2 = "Nový text\nDruhý řádek".into();
        frame(&ctx, &mut app, input());
        assert_eq!(app.editor.active().texts[&id], original);
        assert!(app.editor.undo_label().is_none());
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique(("annotation-text", id.0))));
        let modifiers = egui::Modifiers { ctrl: true, ..Default::default() };
        let mut pressed = input();
        pressed.events.push(egui::Event::ModifiersChanged(modifiers));
        pressed.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        frame(&ctx, &mut app, pressed);
        assert_eq!(app.editor.active().texts[&id].content, "Nový text\nDruhý řádek");
        assert_eq!(app.editor.active().texts[&id].placement, original.placement);
        app.editor.undo();
        assert_eq!(app.editor.active().texts[&id], original);
        frame(&ctx, &mut app, input());
        app.text_edit.as_mut().unwrap().2 = "Uncommitted".into();
        app.editor.clear_selection();
        frame(&ctx, &mut app, input());
        assert!(app.text_edit.is_none());
        app.text_edit = Some((id, original.content.clone(), "Uncommitted".into()));
        crate::files::open_text(&mut app, fixture, "fixture.rysmo").unwrap();
        assert!(app.text_edit.is_none());
    }
}

/// Shared by the floating Inspector and the full style editor.
fn renderer_controls(ui: &mut egui::Ui, style: &mut rysmo_model::Style) {
    use rysmo_model::{AntiAliasing, Lighting, ShadowQuality, ToneMapping};
    #[cfg(not(target_arch = "wasm32"))]
    let performance_preview = std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && std::env::var_os("RYSMO_RENDER_PERFORMANCE_PREVIEW").is_some();
    #[cfg(target_arch = "wasm32")]
    let performance_preview = false;
    ui.label("Renderer presets");
    ui.horizontal_wrapped(|ui| {
        if ui.button("Performance").on_hover_text("Flat lighting, 75% 3D resolution and 1024 px textures.").clicked() {
            style.exposure = 0.;
            style.render_scale = 0.75;
            style.texture_limit = 1024;
            style.texture_anisotropy = 1;
            style.lighting = Lighting::Flat;
            style.shadows = false;
            style.ambient_occlusion = false;
            style.anti_aliasing = AntiAliasing::Off;
            style.tone_mapping = ToneMapping::None;
            style.shadow_quality = ShadowQuality::Low;
        }
        if ui.button("Balanced").on_hover_text("Sun, 1024 px shadows and FXAA.").clicked() {
            style.exposure = 0.;
            style.render_scale = 1.;
            style.texture_limit = 2048;
            style.texture_anisotropy = 4;
            style.lighting = Lighting::Sun;
            style.shadows = true;
            style.ambient_occlusion = false;
            style.anti_aliasing = AntiAliasing::Fxaa;
            style.tone_mapping = ToneMapping::None;
            style.shadow_quality = ShadowQuality::Medium;
            style.sun_intensity = 1.;
            style.ambient_intensity = 1.;
        }
        if ui.button("Quality").on_hover_text("Sun, high-resolution shadows and MSAA 4x.").clicked() {
            style.exposure = 0.;
            style.render_scale = 1.;
            style.texture_limit = 4096;
            style.texture_anisotropy = 16;
            style.lighting = Lighting::Sun;
            style.shadows = true;
            style.ambient_occlusion = false;
            style.anti_aliasing = AntiAliasing::Msaa4;
            style.tone_mapping = ToneMapping::AgX;
            style.shadow_quality = ShadowQuality::High;
            style.sun_intensity = 1.;
            style.ambient_intensity = 1.;
        }
    });
    crate::widgets::section(ui, "Lighting", !performance_preview, |ui| {
        choice(
            ui,
            "lighting",
            &mut style.lighting,
            &[(Lighting::Flat, "Flat"), (Lighting::Sun, "Sun"), (Lighting::Studio, "Studio")],
        );
        if style.lighting == Lighting::Studio {
            choice(
                ui,
                "studio-environment",
                &mut style.studio_environment,
                &[(rysmo_model::StudioEnvironment::Neutral, "Neutral"), (rysmo_model::StudioEnvironment::Warm, "Warm")],
            );
            ui.add(crate::widgets::Slider::new(&mut style.studio_intensity, 0. ..=4.).text("Environment intensity"));
            ui.add(
                crate::widgets::Slider::new(&mut style.studio_rotation, -180. ..=180.)
                    .text("Environment rotation (degrees)"),
            );
            ui.small("Built-in studio environments light the model without changing its background. Studio lighting does not cast sun shadows.");
        }
        ui.add_enabled(
            style.lighting != Lighting::Flat,
            crate::widgets::Slider::new(&mut style.exposure, -4. ..=4.).text("Exposure (stops)"),
        );
        if style.lighting == Lighting::Sun {
            crate::widgets::checkbox(ui, &mut style.shadows, "Shadows");
            if style.shadows && !style.ground {
                ui.horizontal_wrapped(|ui| {
                    ui.small("The model casts shadows on the ground only while the ground is shown.");
                    if ui.small_button("Show ground").clicked() {
                        style.ground = true;
                    }
                });
            }
            ui.add_enabled_ui(style.shadows, |ui| {
                ui.label("Shadow quality");
                choice(
                    ui,
                    "shadow-quality",
                    &mut style.shadow_quality,
                    &[(ShadowQuality::Low, "Low"), (ShadowQuality::Medium, "Medium"), (ShadowQuality::High, "High")],
                );
            });
            ui.add(crate::widgets::Slider::new(&mut style.sun_intensity, 0. ..=4.).text("Sun intensity"));
            ui.add(crate::widgets::Slider::new(&mut style.ambient_intensity, 0. ..=4.).text("Ambient light"));
            ui.add_enabled(crate::render::ADVANCED, crate::widgets::Check::new(&mut style.atmosphere, "Atmosphere"))
                .on_hover_text("A sky and haze computed from the sun. The sky colour is not used.");
            if cfg!(target_arch = "wasm32") {
                ui.small("Atmosphere requires the desktop renderer.");
            }
            crate::widgets::section(ui, "Sun position", false, |ui| {
                crate::widgets::checkbox(ui, &mut style.sun.from_time, "Sun from date and solar time");
                if style.sun.from_time {
                    ui.add(crate::widgets::Slider::new(&mut style.sun.day, 1..=365).text("Day of year"));
                    ui.add(crate::widgets::Slider::new(&mut style.sun.hour, 0.0..=24.0).text("Solar hour"));
                    ui.add(crate::widgets::Slider::new(&mut style.sun.latitude, -90.0..=90.0).text("Latitude"));
                } else {
                    ui.add(crate::widgets::Slider::new(&mut style.sun.azimuth, 0.0..=360.0).text("Azimuth"));
                    ui.add(crate::widgets::Slider::new(&mut style.sun.elevation, -90.0..=90.0).text("Elevation"));
                }
                ui.add(crate::widgets::Slider::new(&mut style.sun.north, -180.0..=180.0).text("North angle"));
                let (azimuth, elevation) = style.sun.position();
                ui.label(format!("Sun: azimuth {azimuth:.1}°, elevation {elevation:.1}°"));
            });
        }
    });
    ui.separator();
    crate::widgets::section(ui, "Image quality", !performance_preview, |ui| {
        ui.add_enabled(
            crate::render::ADVANCED && style.lighting != Lighting::Flat,
            crate::widgets::Check::new(&mut style.ambient_occlusion, "Ambient occlusion"),
        );
        ui.label("Tone mapping");
        choice(
            ui,
            "tone-mapping",
            &mut style.tone_mapping,
            &[
                (ToneMapping::None, "None"),
                (ToneMapping::AgX, "AgX"),
                (ToneMapping::TonyMcMapface, "Tony McMapface"),
                (ToneMapping::AcesFitted, "ACES"),
            ],
        );
        if style.ambient_occlusion
            && style.anti_aliasing == AntiAliasing::Msaa4
            && crate::render::ADVANCED
            && style.lighting != Lighting::Flat
        {
            ui.label("Ambient occlusion uses FXAA instead of MSAA.");
        }
        ui.label("Anti-aliasing");
        let options = [
            (AntiAliasing::Off, "Off"),
            (AntiAliasing::Msaa4, "MSAA 4x"),
            (AntiAliasing::Fxaa, "FXAA"),
            (AntiAliasing::Taa, "TAA"),
        ];
        let options = if cfg!(target_arch = "wasm32") { &options[..3] } else { &options[..] };
        choice(ui, "anti-aliasing", &mut style.anti_aliasing, options);
        if cfg!(target_arch = "wasm32") {
            ui.label("Ambient occlusion and TAA require the desktop renderer.");
        }
        ui.label("Texture filtering");
        choice(
            ui,
            "texture-anisotropy",
            &mut style.texture_anisotropy,
            &[
                (1, "Trilinear"),
                (2, "2× anisotropic"),
                (4, "4× anisotropic"),
                (8, "8× anisotropic"),
                (16, "16× anisotropic"),
            ],
        );
    });
    ui.separator();
    crate::widgets::section(ui, "Performance", true, |ui| {
        let mut percent = (style.render_scale * 100.).round();
        if ui
            .add(crate::widgets::Slider::new(&mut percent, 50. ..=150.).step_by(5.).text("3D resolution (%)"))
            .changed()
        {
            style.render_scale = percent / 100.;
        }
        ui.small("Only the 3D image changes resolution. Interface text and picking stay at full resolution. Above 100% costs more GPU memory and time.");
        ui.label("Maximum texture size");
        choice(
            ui,
            "texture-limit",
            &mut style.texture_limit,
            &[
                (512, "512 px"),
                (1024, "1024 px"),
                (2048, "2048 px"),
                (4096, "4096 px"),
                (8192, "8192 px"),
                (0, "Original"),
            ],
        );
        ui.small(if cfg!(target_arch = "wasm32") {
            "Display textures and the 3D target are capped at 4096 px. Original images remain in the model."
        } else {
            "Display textures and the 3D target are capped at 8192 px. Original images remain in the model."
        });
    });
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    #[test]
    fn settings_pages_preserve_project_when_opened_and_register_pointer_bounds() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.show_settings = true;
        let before = app.editor.doc().clone();
        let revision = app.editor.revision();
        for tab in 0..3 {
            app.settings_tab = tab;
            app.ui_rects.clear();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 700.))),
                    ..Default::default()
                },
                |_| settings(&ctx, &mut app),
            );
            output.textures_delta.clear();
            assert_eq!(app.editor.doc(), &before);
            assert_eq!(app.editor.revision(), revision);
            assert!(app.show_settings);
            assert!(!app.ui_rects.is_empty());
        }
    }
}

#[cfg(test)]
mod measurement_panel_tests {
    use super::*;
    #[test]
    fn opening_settings_preserves_small_and_imperial_snap_increments() {
        for units in [
            rysmo_model::Units::Millimetres,
            rysmo_model::Units::Metres,
            rysmo_model::Units::Feet,
            rysmo_model::Units::Inches,
        ] {
            for step in [1e-9, 0.0125] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                let mut app = App3d::new(rysmo_model::Editor::new());
                app.editor.apply(Command::SetUnits { units }).unwrap();
                let mut value = app.editor.doc().measurement;
                value.length_snap = Some(step);
                app.editor.apply(Command::SetMeasurementSettings { settings: value }).unwrap();
                let before = app.editor.doc().clone();
                let depth = app.editor.undo_depth();
                app.show_settings = true;
                app.settings_tab = 1;
                for _ in 0..2 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.))),
                            ..Default::default()
                        },
                        |ui| settings(ui.ctx(), &mut app),
                    );
                    output.textures_delta.clear();
                }
                assert_eq!(app.editor.doc(), &before);
                assert_eq!(app.editor.undo_depth(), depth);
            }
        }
    }
}

#[cfg(test)]
mod volume_cache_tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn volume_cache_tracks_scale_undo_and_document_replacement() {
        let mut editor = rysmo_model::Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::PushPull { face, distance: -1., keep_base: false }).unwrap();
        let elements = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        let created = editor.apply(Command::MakeGroup { elements }).unwrap().created;
        let Element::Instance(id) = created[0] else { panic!("Expected group") };
        let mut cache = VolumeCache::default();
        assert!((cache.get(&editor, 0, id).unwrap() - 1.).abs() < 1e-9);
        editor
            .apply(Command::Scale {
                elements: vec![Element::Instance(id)],
                origin: DVec3::ZERO,
                factors: DVec3::new(-2., 3., 4.),
            })
            .unwrap();
        assert!((cache.get(&editor, 0, id).unwrap() - 24.).abs() < 1e-9);
        editor.undo();
        assert!((cache.get(&editor, 0, id).unwrap() - 1.).abs() < 1e-9);
        editor.replace_document(rysmo_model::Document::new());
        assert!(cache.get(&editor, 1, id).is_err());
    }
}
