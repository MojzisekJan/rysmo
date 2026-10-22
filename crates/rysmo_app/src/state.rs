//! The app's one resource: the editing session, the tools, the camera and UI state.

use std::path::PathBuf;

use bevy::prelude::*;
use rysmo_math::{DVec2, DVec3, OrbitCamera, View};
use rysmo_model::Editor;
use rysmo_tools::gesture::Gestures;
use rysmo_tools::{Ctx, Modifiers, ToolBox, ToolKind};

#[derive(Resource)]
pub struct App3d {
    pub features: crate::features::State,
    pub preferences: crate::preferences::Preferences,
    pub preference_store: crate::preferences::Store,
    pub welcome: bool,
    pub editor: Editor,
    pub tools: ToolBox,
    pub camera: OrbitCamera,
    pub path: Option<PathBuf>,
    /// Text typed into the VCB.
    pub vcb: String,
    /// A message for the status bar that outlives the tool's own (file errors).
    pub message: Option<String>,
    /// Window size in logical pixels.
    pub viewport: DVec2,
    pub canvas: Option<(DVec2, DVec2)>,
    /// The cursor over the 3D view, in logical pixels.
    pub cursor: Option<DVec2>,
    pub drape: Option<crate::drape_tool::Draft>,
    pub stamp: Option<crate::stamp_tool::Draft>,
    pub detail: Option<crate::detail_tool::Draft>,
    pub flip: Option<crate::flip_tool::State>,
    pub mirror: Option<crate::mirror::Draft>,
    pub fillet: Option<crate::fillet::Draft>,
    pub solid_tools: Option<crate::solid_tools::Draft>,
    pub print_check: Option<crate::print_check::Dialog>,
    pub smoove: Option<crate::smoove_tool::Draft>,
    pub smoove_radius: f64,
    pub terrain_last: Option<crate::terrain_tool::LastGrid>,
    pub terrain: Option<crate::terrain_tool::Draft>,
    pub terrain_spacing: f64,
    pub text3d: Option<crate::text3d_tool::Draft>,
    pub text3d_defaults: rysmo_model::text3d::Text3dSettings,
    pub text_creation: Option<crate::text_tool::TextCreation>,
    pub text_edit: Option<(rysmo_model::TextId, String, String)>,
    pub dimension_text_edit: Option<(rysmo_model::DimensionId, String, String)>,
    pub dimension_text_sizes: std::collections::BTreeMap<rysmo_model::dimensions::DimensionTextKey, DVec2>,
    pub text_sizes: std::collections::BTreeMap<rysmo_model::text::TextKey, DVec2>,
    pub held: Modifiers,
    /// Ctrl toggles modes in Move (copy) and Push/Pull (new starting face).
    pub ctrl_toggle: bool,
    pub touch: TouchState,
    pub last_click: Option<(f64, DVec2, u8)>,
    pub context_menu: Option<DVec2>,
    /// Whether the button that opened the context menu has been released.
    pub context_menu_armed: bool,
    /// Which camera drag is going on with the mouse.
    pub camera_drag: Option<CameraDrag>,
    pub orbit_pivot: Option<DVec3>,
    pub walking: Option<(DVec2, DVec2)>,
    pub positioning: Option<(DVec2, DVec3)>,
    pub eye_height: f64,
    pub camera_history: Vec<rysmo_math::OrbitCamera>,
    pub zoom_window: Option<(DVec2, DVec2)>,
    pub last_wheel_time: Option<f64>,
    pub camera_setting_editing: bool,
    pub zoom_focal: bool,
    /// The left button is down over the 3D view.
    pub left_down: bool,
    pub show_about: bool,
    pub help: crate::help::HelpState,
    pub volume_cache: crate::panels::VolumeCache,
    pub unsaved: crate::unsaved::Guard,
    pub command_search: crate::command_search::SearchState,
    pub show_materials: bool,
    pub show_tags: bool,
    pub show_settings: bool,
    pub settings_tab: usize,
    pub settings_filter: String,
    #[cfg(target_arch = "wasm32")]
    pub autosave_settings: crate::autosave::Settings,
    pub measurement_edit_revision: Option<u64>,
    pub show_sections: bool,
    pub show_scenes: bool,
    pub show_components: bool,
    pub component_filter: String,
    pub outliner_filter: String,
    pub outliner_sort: bool,
    pub outliner_rename: Option<(Vec<rysmo_model::InstanceId>, String)>,
    pub outliner_collapsed: std::collections::BTreeSet<Vec<rysmo_model::InstanceId>>,
    pub component_selected: Option<rysmo_model::DefinitionId>,
    pub component_edit: Option<(rysmo_model::DefinitionId, String, rysmo_model::components::ComponentSettings)>,
    pub object_clipboard: Option<crate::object_clipboard::Contents>,
    pub paste_place: Option<crate::object_clipboard::Placement>,
    pub document_generation: u64,
    pub component_place: Option<crate::component_panel::Placement>,
    pub selected_scene: Option<u64>,
    pub scene_update: Option<(u64, rysmo_model::scenes::SceneProperties)>,
    pub texture_session: Option<crate::texture_view::Session>,
    pub section_placing: Option<(u64, Vec<rysmo_model::InstanceId>)>,
    pub show_entity_info: bool,
    pub portable_files: Option<crate::portable_files::Dialog>,
    pub inspector_tab: usize,
    pub soften_angle: f64,
    pub entity_edit_revision: Option<u64>,
    pub style_edit_revision: Option<u64>,
    /// The colour being edited in the Materials window.
    pub new_color: [f32; 4],
    pub new_tag: String,
    /// Recent files (the web build keeps them in the browser's storage).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub recent: Vec<String>,
    /// The live MCP server's port and token, when it runs.
    pub mcp: Option<(u16, String)>,
    pub show_mcp: bool,
    /// The material whose colour is being dragged, to keep one undo step per drag.
    pub material_editing: Option<rysmo_model::MaterialId>,
    /// Screen rectangles of the panels, which the 3D view does not get clicks from.
    pub ui_rects: Vec<(DVec2, DVec2)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraDrag {
    Middle,
    Orbit,
    Pan,
    Zoom,
    LookAround,
}

/// Touch-specific state: whether touch is in use and the on-screen modifier toggles.
#[derive(Default)]
pub struct TouchState {
    pub enabled: bool,
    pub gestures: Gestures,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// The current one-finger drag orbits instead of going to the tool.
    pub orbiting: bool,
    pub navigation_started: bool,
}

impl App3d {
    pub fn new(editor: Editor) -> Self {
        let camera = editor.doc().camera.unwrap_or_default();
        let touch_forced = std::env::var_os("RYSMO_TOUCH").is_some_and(|v| !v.is_empty() && v != "0");
        App3d {
            features: Default::default(),
            editor,
            tools: ToolBox::new(),
            camera,
            path: None,
            vcb: String::new(),
            message: None,
            canvas: None,
            viewport: DVec2::new(1280.0, 800.0),
            cursor: None,
            drape: None,
            stamp: None,
            detail: None,
            flip: None,
            mirror: None,
            solid_tools: None,
            print_check: None,
            fillet: None,
            smoove: None,
            smoove_radius: 3.048,
            terrain_last: None,
            terrain: None,
            terrain_spacing: 3.048,
            text3d: None,
            text3d_defaults: Default::default(),
            text_creation: None,
            text_edit: None,
            dimension_text_edit: None,
            dimension_text_sizes: Default::default(),
            text_sizes: Default::default(),
            held: Modifiers::default(),
            ctrl_toggle: false,
            touch: TouchState {
                enabled: touch_forced || cfg!(any(target_os = "android", target_os = "ios")),
                ..Default::default()
            },
            last_click: None,
            preferences: Default::default(),
            preference_store: Default::default(),
            welcome: false,
            context_menu: None,
            context_menu_armed: false,
            camera_drag: None,
            orbit_pivot: None,
            positioning: None,
            walking: None,
            eye_height: 1.6764,
            camera_history: Vec::new(),
            zoom_window: None,
            last_wheel_time: None,
            camera_setting_editing: false,
            zoom_focal: false,
            left_down: false,
            show_about: false,
            help: Default::default(),
            command_search: Default::default(),
            unsaved: Default::default(),
            volume_cache: Default::default(),
            show_materials: false,
            show_tags: false,
            show_settings: false,
            settings_tab: 0,
            settings_filter: String::new(),
            #[cfg(target_arch = "wasm32")]
            autosave_settings: Default::default(),
            measurement_edit_revision: None,
            show_sections: false,
            show_scenes: false,
            show_components: false,
            component_filter: String::new(),
            outliner_filter: String::new(),
            outliner_sort: false,
            outliner_rename: None,
            outliner_collapsed: Default::default(),
            component_selected: None,
            component_edit: None,
            object_clipboard: None,
            paste_place: None,
            document_generation: 1,
            component_place: None,
            selected_scene: None,
            scene_update: None,
            texture_session: None,
            section_placing: None,
            show_entity_info: false,
            portable_files: None,
            inspector_tab: 1,
            soften_angle: 20.0,
            entity_edit_revision: None,
            style_edit_revision: None,
            new_color: [0.75, 0.55, 0.35, 1.0],
            new_tag: String::new(),
            material_editing: None,
            recent: Vec::new(),
            mcp: None,
            show_mcp: false,
            ui_rects: Vec::new(),
        }
    }

    /// Whether a screen point is over a panel.
    /// A late startup snapshot must not replace a chosen document or edited model.
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn accepts_recovery(&self, startup_generation: u64) -> bool {
        self.document_generation == startup_generation && self.path.is_none() && !self.editor.is_dirty()
    }

    pub fn over_panel(&self, p: DVec2) -> bool {
        self.ui_rects.iter().any(|(lo, hi)| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y)
    }

    /// The camera's view of the world.
    pub fn view(&self) -> View {
        let mut view = self.camera.view(self.canvas_size());
        view.offset = self.canvas_origin();
        view
    }

    pub fn canvas_size(&self) -> DVec2 {
        self.canvas.map(|(_, size)| size).unwrap_or(self.viewport).max(DVec2::ONE)
    }
    pub fn canvas_origin(&self) -> DVec2 {
        self.canvas.map(|(origin, _)| origin).unwrap_or(DVec2::ZERO)
    }
    pub fn canvas_centre(&self) -> DVec2 {
        self.canvas_origin() + self.canvas_size() * 0.5
    }

    /// The camera's view in the coordinates of the group or component being edited.
    pub fn local_view(&self) -> View {
        self.view().local(self.editor.context_transform())
    }

    /// The modifiers the tools see: held keys, the on-screen toggles, and the
    /// Ctrl toggle for Move and Push/Pull.
    pub fn modifiers(&self) -> Modifiers {
        let ctrl_is_toggle = matches!(
            self.tools.active(),
            ToolKind::Move
                | ToolKind::PushPull
                | ToolKind::Rotate
                | ToolKind::Scale
                | ToolKind::TapeMeasure
                | ToolKind::Protractor
        );
        Modifiers {
            shift: self.held.shift || self.touch.shift,
            ctrl: if ctrl_is_toggle { self.ctrl_toggle || self.touch.ctrl } else { self.held.ctrl || self.touch.ctrl },
            alt: self.held.alt || self.touch.alt,
        }
    }

    pub fn pick_scale(&self) -> f64 {
        if self.touch.enabled { 2.5 } else { 1.0 }
    }

    /// Runs `f` with the tool context for a cursor position.
    pub fn with_ctx<R>(&mut self, cursor: DVec2, f: impl FnOnce(&mut ToolBox, &mut Editor, &Ctx) -> R) -> R {
        let view = self.local_view();
        let ctx = Ctx { view: &view, cursor, modifiers: self.modifiers(), scale: self.pick_scale() };
        f(&mut self.tools, &mut self.editor, &ctx)
    }

    pub fn set_tool(&mut self, kind: ToolKind) {
        crate::texture_view::finish(self, false);
        self.paste_place = None;
        self.component_place = None;
        self.drape = None;
        self.stamp = None;
        self.section_placing = None;
        self.detail = None;
        self.flip = None;
        self.mirror = None;
        self.solid_tools = None;
        self.fillet = None;
        self.smoove = None;
        self.terrain_last = None;
        self.terrain = None;
        self.text3d = None;
        self.text_creation = None;
        self.zoom_window = None;
        self.positioning = None;
        self.walking = None;
        self.camera_drag = None;
        if matches!(kind, ToolKind::LookAround | ToolKind::Walk) {
            self.eye_height = self.camera.eye().z - self.camera.eye_base_z;
        }
        self.tools.set_tool(kind);
        if kind == ToolKind::PaintBucket {
            self.show_materials = true;
        }
        self.ctrl_toggle = false;
        self.vcb.clear();
        self.message = None;
    }

    pub fn status(&self) -> String {
        // Errors read as sentence fragments; the status line starts them as sentences.
        let sentence = |m: &String| {
            let mut chars = m.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
        };
        self.message.as_ref().map(sentence).unwrap_or_else(|| {
            let mut s = if self.fillet.is_some() {
                "Fillet: set the radius and segments, then Apply. Escape cancels."
            } else if self.solid_tools.is_some() {
                "Solid Tools: choose an operation and confirm Apply. Escape cancels."
            } else if self.component_place.is_some() {
                "Place component: click an insertion point or enter a distance. Escape cancels."
            } else if let Some(draft) = &self.drape {
                draft.status()
            } else if let Some(d) = &self.stamp {
                d.status()
            } else if let Some(d) = &self.detail {
                d.status()
            } else if let Some(draft) = &self.mirror {
                draft.status()
            } else if self.flip.is_some() {
                "Flip Edge: hover an interior terrain edge and click to replace its diagonal. Escape cancels."
            } else if let Some(draft) = &self.smoove {
                draft.status()
            } else if let Some(draft) = &self.terrain {
                draft.status()
            } else if let Some(draft) = &self.text3d {
                if draft.mesh.is_some() {
                    "Place 3D text. Click to finish; Escape cancels."
                } else {
                    "Enter 3D text and its settings, then choose Place."
                }
            } else if let Some(draft) = &self.text_creation {
                if draft.placing {
                    "Place the text and leader. Click or release a drag to enter its content. Escape cancels."
                } else {
                    "Enter text. Enter twice or Ctrl+Enter completes it. Escape cancels."
                }
            } else {
                self.tools.status()
            }
            .to_string();
            if self.modifiers().ctrl && self.terrain.is_none() && self.text3d.is_none() && self.smoove.is_none() {
                s.push_str(match self.tools.active() {
                    ToolKind::Move | ToolKind::Rotate => "  [Copy]",
                    ToolKind::PushPull => "  [New starting face]",
                    ToolKind::TapeMeasure => "  [Measure only]",
                    ToolKind::Protractor => "  [Create guide]",
                    ToolKind::Scale => "  [About the centre]",
                    _ => "",
                });
            }
            let ctx = self.editor.context();
            if !ctx.is_empty() {
                let doc = self.editor.doc();
                let names: Vec<String> = (1..=ctx.len())
                    .filter_map(|k| doc.resolve(&ctx[..k]).ok().flatten())
                    .filter_map(|d| doc.definitions.get(&d).map(|x| x.name.clone()))
                    .collect();
                s = format!("Editing {}. {}", names.join(" > "), s);
            }
            s
        })
    }

    pub fn zoom_drag(&mut self, delta: DVec2, shift: bool) {
        if delta.y.abs() < 1e-10 {
            return;
        }
        if shift && !self.camera.parallel {
            self.camera.fov_y = (self.camera.fov_y.to_degrees() - delta.y * 0.25).clamp(1.0, 120.0).to_radians();
            self.zoom_focal = false;
        } else {
            self.camera.zoom_at(1.0 + delta.y * 0.01, self.canvas_size() * 0.5, self.canvas_size());
        }
    }

    pub fn annotation_box(&self, from: DVec2, to: DVec2) -> rysmo_tools::AnnotationBoxHits {
        let doc = self.editor.doc();
        let view = self.view();
        let mut hits = rysmo_tools::AnnotationBoxHits::default();
        for occurrence in doc.occurrences(self.editor.context()).into_iter().filter(|o| o.in_context) {
            let Some(definition) = doc.def(occurrence.definition) else { continue };
            if doc.style.section_planes {
                for (id, plane) in &definition.sections.planes {
                    let source = rysmo_model::sections::glyph(plane, doc.definition_bounds(occurrence.definition));
                    let lines = source
                        .iter()
                        .filter_map(|[a, b]| {
                            view.project_segment(
                                occurrence.transform.transform_point3(*a),
                                occurrence.transform.transform_point3(*b),
                            )
                        })
                        .collect::<Vec<_>>();
                    let crossing = lines.iter().any(|(a, b)| section_line_box(*a, *b, from.min(to), from.max(to)));
                    let enclosed = lines.len() == source.len()
                        && lines.iter().all(|(a, b)| {
                            [*a, *b].iter().all(|p| p.cmpge(from.min(to)).all() && p.cmple(from.max(to)).all())
                        });
                    if let Some(instance) = occurrence.top {
                        let entry = hits.instances.entry(instance).or_insert((false, true));
                        entry.0 |= crossing;
                        entry.1 &= enclosed;
                    } else if if to.x < from.x { crossing } else { enclosed } {
                        hits.elements.push(rysmo_model::Element::Section(*id));
                    }
                }
            }
            for (id, text) in &definition.texts {
                let Some(layout) = text.layout(doc, occurrence.definition, occurrence.transform, &view) else {
                    continue;
                };
                let size = self.text_sizes.get(&text.text_key()).copied().unwrap_or_else(|| text.estimated_size());
                let Some(geometry) = text.geometry(&layout, size) else { continue };
                let crossing = geometry.box_hit(from.min(to), from.max(to), true);
                let enclosed = geometry.box_hit(from.min(to), from.max(to), false);
                if let Some(instance) = occurrence.top {
                    let entry = hits.instances.entry(instance).or_insert((false, true));
                    entry.0 |= crossing;
                    entry.1 &= enclosed;
                } else if if to.x < from.x { crossing } else { enclosed } {
                    hits.elements.push(rysmo_model::Element::Text(*id));
                }
            }
            for (id, dimension) in &definition.dimensions {
                let location =
                    occurrence.transform.transform_point3(dimension.resolve(doc, occurrence.definition).from);
                let Some(layout) = dimension.layout(
                    doc,
                    occurrence.definition,
                    occurrence.transform,
                    view.pixel_size_at(location) * 6.,
                ) else {
                    continue;
                };
                if !layout.visible(&view, doc.style.dimensions) {
                    continue;
                }
                let label = dimension.label_in_document(&layout.measurement, doc);
                let size = self.dimension_text_sizes.get(&dimension.text_key(label)).copied();
                let crossing = dimension.box_hit(&layout, &view, from.min(to), from.max(to), true, size);
                let enclosed = dimension.box_hit(&layout, &view, from.min(to), from.max(to), false, size);
                if let Some(instance) = occurrence.top {
                    let entry = hits.instances.entry(instance).or_insert((false, true));
                    entry.0 |= crossing;
                    entry.1 &= enclosed;
                } else if if to.x < from.x { crossing } else { enclosed } {
                    hits.elements.push(rysmo_model::Element::Dimension(*id));
                }
            }
        }
        hits
    }

    #[cfg(test)]
    pub fn dimension_box(&self, from: DVec2, to: DVec2) -> Vec<rysmo_model::Element> {
        self.annotation_box(from, to).elements
    }

    pub fn pick_annotation(&self, cursor: DVec2) -> Option<rysmo_infer::Pick> {
        let doc = self.editor.doc();
        let view = self.view();
        doc.occurrences(self.editor.context())
            .into_iter()
            .flat_map(|occurrence| {
                let Some(definition) = doc.def(occurrence.definition) else { return Vec::new() };
                let mut hits = Vec::new();
                if doc.style.section_planes {
                    for (id, plane) in &definition.sections.planes {
                        let distance =
                            rysmo_model::sections::glyph(plane, doc.definition_bounds(occurrence.definition))
                                .into_iter()
                                .filter_map(|[a, b]| {
                                    view.project_segment(
                                        occurrence.transform.transform_point3(a),
                                        occurrence.transform.transform_point3(b),
                                    )
                                })
                                .map(|(a, b)| rysmo_math::distance_point_segment_2d(cursor, a, b))
                                .fold(f64::INFINITY, f64::min);
                        if distance > 6. * self.pick_scale() {
                            continue;
                        }
                        let pick = if !occurrence.in_context {
                            rysmo_infer::Pick::Outside
                        } else if let Some(instance) = occurrence.top {
                            rysmo_infer::Pick::Element(rysmo_model::Element::Instance(instance))
                        } else {
                            rysmo_infer::Pick::Element(rysmo_model::Element::Section(*id))
                        };
                        hits.push((distance, 2u8, pick));
                    }
                }
                for (id, text) in &definition.texts {
                    let Some(layout) = text.layout(doc, occurrence.definition, occurrence.transform, &view) else {
                        continue;
                    };
                    let size = self.text_sizes.get(&text.text_key()).copied().unwrap_or_else(|| text.estimated_size());
                    let Some(geometry) = text.geometry(&layout, size) else { continue };
                    let distance = geometry.hit_distance(cursor);
                    if distance > 6. * self.pick_scale() {
                        continue;
                    }
                    let pick = if !occurrence.in_context {
                        rysmo_infer::Pick::Outside
                    } else if let Some(instance) = occurrence.top {
                        rysmo_infer::Pick::Element(rysmo_model::Element::Instance(instance))
                    } else {
                        rysmo_infer::Pick::Element(rysmo_model::Element::Text(*id))
                    };
                    hits.push((distance, 1u8, pick));
                }
                hits.extend(
                    definition
                        .dimensions
                        .iter()
                        .filter_map(|(id, dimension)| {
                            let from = occurrence
                                .transform
                                .transform_point3(dimension.resolve(doc, occurrence.definition).from);
                            let layout = dimension.layout(
                                doc,
                                occurrence.definition,
                                occurrence.transform,
                                view.pixel_size_at(from) * 6.0,
                            )?;
                            if !layout.visible(&view, doc.style.dimensions) {
                                return None;
                            }
                            let text = dimension.label_in_document(&layout.measurement, doc);
                            let size = self.dimension_text_sizes.get(&dimension.text_key(text)).copied();
                            let distance = dimension.hit_distance(&layout, &view, cursor, size)?;
                            if distance > 6.0 * self.pick_scale() {
                                return None;
                            }
                            let pick = if !occurrence.in_context {
                                rysmo_infer::Pick::Outside
                            } else if occurrence.top.is_none() {
                                rysmo_infer::Pick::Element(rysmo_model::Element::Dimension(*id))
                            } else {
                                rysmo_infer::Pick::Element(rysmo_model::Element::Instance(occurrence.top?))
                            };
                            Some((distance, 0u8, pick))
                        })
                        .collect::<Vec<_>>(),
                );
                hits
            })
            .enumerate()
            .min_by(|(order, a), (other, b)| {
                a.0.total_cmp(&b.0).then_with(|| b.1.cmp(&a.1)).then_with(|| other.cmp(order))
            })
            .map(|(_, (_, _, pick))| pick)
    }

    pub fn pick_orbit_pivot(&self, cursor: DVec2) -> DVec3 {
        self.editor.doc().ray_surface(self.view().ray(cursor)).map(|hit| hit.point).unwrap_or(self.camera.target)
    }

    pub fn remember_view(&mut self) {
        self.last_wheel_time = None;
        if self.camera_history.last() != Some(&self.camera) {
            self.camera_history.push(self.camera);
            if self.camera_history.len() > 32 {
                self.camera_history.remove(0);
            }
        }
    }

    pub fn previous_view(&mut self) {
        self.last_wheel_time = None;
        while let Some(camera) = self.camera_history.pop() {
            if camera != self.camera {
                self.camera = camera;
                break;
            }
        }
    }

    /// Frames the whole model (Zoom Extents).
    pub fn zoom_extents(&mut self) {
        self.remember_view();
        match self.editor.doc().definition_bounds(None) {
            Some((lo, hi)) => self.camera.frame(lo, hi),
            None => self.camera.frame(DVec3::splat(-1.0), DVec3::splat(1.0)),
        }
    }
}

/// Model coordinates (Z up, metres) to Bevy's (Y up).
pub fn to_bevy(p: DVec3) -> Vec3 {
    Vec3::new(p.x as f32, p.z as f32, -p.y as f32)
}

#[cfg(test)]
mod dimension_pick_tests {
    use super::*;
    #[test]
    fn pointer_box_selects_crossed_dimension_but_excludes_partial_window() {
        for (crossing, position, alignment) in [false, true].into_iter().flat_map(|crossing| {
            use rysmo_model::dimensions::DimensionTextPosition::*;
            [Centered, OutsideStart, OutsideEnd].into_iter().flat_map(move |position| {
                use rysmo_model::dimensions::DimensionTextAlignment::*;
                [Screen, DimensionLine].into_iter().map(move |alignment| (crossing, position, alignment))
            })
        }) {
            let document = rysmo_io::load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
            let mut app = App3d::new(Editor::with_document(document));
            let id = rysmo_model::DimensionId(10);
            let mut dimension = app.editor.active().dimensions[&id].clone();
            dimension.text_position = position;
            dimension.alignment = alignment;
            app.editor.apply(rysmo_model::Command::SetDimension { id, dimension: dimension.clone() }).unwrap();
            let layout = dimension.layout(app.editor.doc(), None, rysmo_math::DMat4::IDENTITY, 0.1).unwrap();
            app.dimension_text_sizes.insert(
                dimension.text_key(dimension.label_in_document(&layout.measurement, app.editor.doc())),
                DVec2::new(120., 20.),
            );
            let (lo, hi) = dimension.text_bounds(&layout, &app.view(), DVec2::new(120., 20.)).unwrap();
            let centre = (lo + hi) * 0.5;
            let from = centre + DVec2::new(if crossing { 20. } else { -20. }, -20.);
            let to = centre + DVec2::new(if crossing { -20. } else { 20. }, 20.);
            crate::input::tool_pointer(&mut app, from, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Up);
            assert_eq!(app.editor.selection().contains(&rysmo_model::Element::Dimension(id)), crossing);
        }
    }
    fn annotation_group(nested: bool) -> (App3d, rysmo_model::InstanceId, rysmo_model::DimensionId) {
        use rysmo_model::{Command, Element};
        let source = rysmo_io::load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let dimension = source.root.dimensions[&rysmo_model::DimensionId(10)].detached(&source, None);
        let mut editor = Editor::new();
        let added = editor.apply(Command::AddDimension { dimension }).unwrap();
        let id = added.dimensions[0];
        let group = editor.apply(Command::MakeGroup { elements: vec![Element::Dimension(id)] }).unwrap().created[0];
        let Element::Instance(mut instance) = group else { panic!("group") };
        if nested {
            let outer = editor.apply(Command::MakeGroup { elements: vec![group] }).unwrap().created[0];
            let Element::Instance(outer) = outer else { panic!("outer") };
            editor.enter(outer).unwrap();
            instance = *editor.active().instances.keys().next().unwrap();
        }
        let mut app = App3d::new(editor);
        app.camera = source.camera.unwrap();
        let occurrence =
            app.editor.doc().occurrences(app.editor.context()).into_iter().find(|o| o.top == Some(instance)).unwrap();
        let dimension = &app.editor.doc().def(occurrence.definition).unwrap().dimensions[&id];
        let layout = dimension.layout(app.editor.doc(), occurrence.definition, occurrence.transform, 0.1).unwrap();
        app.dimension_text_sizes.insert(
            dimension.text_key(dimension.label_in_document(&layout.measurement, app.editor.doc())),
            DVec2::new(600., 80.),
        );
        (app, instance, id)
    }

    #[test]
    fn group_text_is_picked_and_crossing_selection_applies_modifiers_once_in_context() {
        use rysmo_model::Element;
        for nested in [false, true] {
            for (ctrl, shift, selected, expected) in [
                (false, false, false, true),
                (true, false, false, true),
                (false, true, false, true),
                (false, true, true, false),
                (true, true, true, false),
            ] {
                let (mut app, instance, _) = annotation_group(nested);
                let occurrence = app
                    .editor
                    .doc()
                    .occurrences(app.editor.context())
                    .into_iter()
                    .find(|o| o.top == Some(instance))
                    .unwrap();
                let dimension =
                    app.editor.doc().def(occurrence.definition).unwrap().dimensions.values().next().unwrap();
                let layout =
                    dimension.layout(app.editor.doc(), occurrence.definition, occurrence.transform, 0.1).unwrap();
                let rect = dimension.text_rect(&layout, &app.view(), DVec2::new(600., 80.)).unwrap();
                let (_, hi) = rect.bounds();
                let at = DVec2::new(hi.x - 10., rect.centre.y);
                assert_eq!(app.pick_annotation(at), Some(rysmo_infer::Pick::Element(Element::Instance(instance))));
                if selected {
                    app.editor.set_selection([Element::Instance(instance)]);
                } else {
                    app.editor.clear_selection();
                }
                app.held = rysmo_tools::Modifiers { ctrl, shift, ..Default::default() };
                let from = DVec2::new(hi.x + 10., rect.centre.y - 10.);
                let to = DVec2::new(hi.x - 20., rect.centre.y + 10.);
                crate::input::tool_pointer(&mut app, from, rysmo_tools::Pointer::Down { clicks: 1 });
                crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
                crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Up);
                assert_eq!(
                    app.editor.is_selected(Element::Instance(instance)),
                    expected,
                    "nested={nested} ctrl={ctrl} shift={shift}"
                );
                assert!(app.editor.selection().iter().all(|e| matches!(e, Element::Instance(_))));
            }
        }
    }

    #[test]
    fn group_geometry_and_annotation_box_hit_toggle_the_group_only_once() {
        use rysmo_model::Element;
        for nested in [false, true] {
            let (mut app, instance, _) = annotation_group(nested);
            app.editor.set_selection([Element::Instance(instance)]);
            app.held.shift = true;
            // Covers both the group bounds and the measured label. The Select
            // transaction must combine these hits before applying Shift.
            let from = DVec2::new(-1000., -1000.);
            let to = DVec2::new(2500., 2000.);
            crate::input::tool_pointer(&mut app, from, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Up);
            assert!(!app.editor.is_selected(Element::Instance(instance)));
            crate::input::tool_pointer(&mut app, from, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Up);
            assert!(app.editor.is_selected(Element::Instance(instance)));
        }
    }

    #[test]
    fn group_window_requires_the_whole_measured_label_including_nested_annotations() {
        use rysmo_model::Element;
        for nested in [false, true] {
            let (mut app, instance, _) = annotation_group(nested);
            let inst = &app.editor.active().instances[&instance];
            let (lo, hi) = app.editor.doc().definition_bounds(Some(inst.definition)).unwrap();
            let points = rysmo_tools::box_lines(lo, hi, &(app.editor.context_transform() * inst.transform))
                .into_iter()
                .flat_map(|(a, b)| [a, b])
                .filter_map(|p| app.view().project(p))
                .collect::<Vec<_>>();
            let lo = points.iter().copied().fold(DVec2::splat(f64::INFINITY), DVec2::min) - DVec2::splat(10.);
            let hi = points.iter().copied().fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max) + DVec2::splat(10.);
            for (from, to, expected) in
                [(lo, hi, false), (lo - DVec2::new(600., 100.), hi + DVec2::new(600., 100.), true)]
            {
                crate::input::tool_pointer(&mut app, from, rysmo_tools::Pointer::Down { clicks: 1 });
                crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
                crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Up);
                assert_eq!(app.editor.is_selected(Element::Instance(instance)), expected, "nested={nested}");
            }
        }
    }

    #[test]
    fn picker_reprojects_measured_text_bounds_after_camera_motion() {
        let document = rysmo_io::load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let mut app = App3d::new(Editor::with_document(document));
        let id = rysmo_model::DimensionId(10);
        let dimension = app.editor.active().dimensions[&id].clone();
        let layout = dimension.layout(app.editor.doc(), None, rysmo_math::DMat4::IDENTITY, 0.1).unwrap();
        let label = dimension.label_in_document(&layout.measurement, app.editor.doc());
        app.dimension_text_sizes.insert(dimension.text_key(label), DVec2::new(120., 20.));
        for _ in 0..2 {
            let at = app.view().project(layout.text_position).unwrap() + DVec2::new(40., 8.);
            assert_eq!(app.pick_annotation(at), Some(rysmo_infer::Pick::Element(rysmo_model::Element::Dimension(id))));
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
            assert!(app.editor.selection().contains(&rysmo_model::Element::Dimension(id)));
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 2 });
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
            assert!(app.show_entity_info);
            app.camera.pan(DVec2::new(100., 20.), app.viewport);
        }
    }
}

#[cfg(test)]
mod text_pick_tests {
    use super::*;
    use rysmo_model::{Command, Element, TextId};
    fn fixture() -> App3d {
        let doc = rysmo_io::load(include_str!("../../../testdata/text-preview.rysmo")).unwrap();
        let mut app = App3d::new(Editor::with_document(doc));
        for text in app.editor.active().texts.values() {
            app.text_sizes.insert(text.text_key(), DVec2::new(220., 50.));
        }
        app
    }
    fn geometry(app: &App3d, id: TextId) -> rysmo_model::text::TextGeometry {
        let text = &app.editor.active().texts[&id];
        let layout = text
            .layout(app.editor.doc(), app.editor.active_definition(), app.editor.context_transform(), &app.view())
            .unwrap();
        text.geometry(&layout, app.text_sizes[&text.text_key()]).unwrap()
    }
    #[test]
    fn text_pointer_select_erase_and_undo_use_rendered_bounds() {
        let mut app = fixture();
        for id in [TextId(1001), TextId(1003), TextId(1004)] {
            let at = geometry(&app, id).rect.centre;
            assert_eq!(app.pick_annotation(at), Some(rysmo_infer::Pick::Element(Element::Text(id))));
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
            assert_eq!(app.editor.selection().iter().copied().collect::<Vec<_>>(), vec![Element::Text(id)]);
            app.show_entity_info = false;
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 2 });
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
            assert!(app.show_entity_info);
            crate::input::erase_selection(&mut app);
            assert!(!app.editor.contains(Element::Text(id)));
            assert!(app.editor.selection().is_empty());
            app.editor.undo();
            assert!(app.editor.contains(Element::Text(id)));
        }
        crate::input::select_all(&mut app);
        assert!(app.editor.active().texts.keys().all(|id| app.editor.is_selected(Element::Text(*id))));
    }
    #[test]
    fn text_crossing_window_and_shift_use_one_selection_transaction() {
        let mut app = fixture();
        let id = TextId(1001);
        let g = geometry(&app, id);
        for (crossing, expected) in [(false, false), (true, true)] {
            let lo = g.rect.centre - DVec2::new(20., 20.);
            let hi = g.rect.centre + DVec2::new(20., 20.);
            let (from, to) = if crossing { (DVec2::new(hi.x, lo.y), DVec2::new(lo.x, hi.y)) } else { (lo, hi) };
            crate::input::tool_pointer(&mut app, from, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Up);
            assert_eq!(app.editor.is_selected(Element::Text(id)), expected);
        }
        app.held.shift = true;
        let (lo, hi) = g.rect.bounds();
        for expected in [false, true] {
            crate::input::tool_pointer(&mut app, lo - DVec2::splat(15.), rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, hi + DVec2::splat(15.), rysmo_tools::Pointer::Move);
            crate::input::tool_pointer(&mut app, hi + DVec2::splat(15.), rysmo_tools::Pointer::Up);
            assert_eq!(app.editor.is_selected(Element::Text(id)), expected);
        }
    }
    #[test]
    fn nested_group_text_pick_and_box_select_the_active_child_instance() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let elements = editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect();
        let Element::Instance(inner) = editor.apply(Command::MakeGroup { elements }).unwrap().created[0] else {
            panic!()
        };
        editor.enter(inner).unwrap();
        let source = fixture();
        let mut text = source.editor.active().texts[&TextId(1003)].clone();
        let rysmo_model::text::TextPlacement::Attached { anchor, leader } = &mut text.placement else { panic!() };
        anchor.point = DVec3::ZERO;
        anchor.reference = None;
        *leader = rysmo_model::text::TextLeader::ViewBased { offset: DVec2::new(150., -100.) };
        editor.apply(Command::AddText { text }).unwrap();
        editor.exit();
        let Element::Instance(outer) =
            editor.apply(Command::MakeGroup { elements: vec![Element::Instance(inner)] }).unwrap().created[0]
        else {
            panic!()
        };
        let mut app = App3d::new(editor);
        for expected in [outer, inner] {
            let occurrence = app
                .editor
                .doc()
                .occurrences(app.editor.context())
                .into_iter()
                .find(|o| app.editor.doc().def(o.definition).is_some_and(|d| !d.texts.is_empty()))
                .unwrap();
            let text = app.editor.doc().def(occurrence.definition).unwrap().texts.values().next().unwrap();
            let layout =
                text.layout(app.editor.doc(), occurrence.definition, occurrence.transform, &app.view()).unwrap();
            let geometry = text.geometry(&layout, DVec2::new(220., 50.)).unwrap();
            app.text_sizes.insert(text.text_key(), DVec2::new(220., 50.));
            assert_eq!(
                app.pick_annotation(geometry.rect.centre),
                Some(rysmo_infer::Pick::Element(Element::Instance(expected)))
            );
            let (lo, hi) = geometry.rect.bounds();
            let from = DVec2::new(hi.x + 10., lo.y - 10.);
            let to = lo - DVec2::new(10., -60.);
            crate::input::tool_pointer(&mut app, from, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
            crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Up);
            assert!(app.editor.is_selected(Element::Instance(expected)));
            if expected == outer {
                app.editor.enter(outer).unwrap();
            }
        }
    }
    #[test]
    fn move_tool_picks_text_previews_and_commits_without_moving_anchor() {
        let mut app = fixture();
        let id = TextId(1003);
        app.set_tool(rysmo_tools::ToolKind::Move);
        let original = app.editor.active().texts[&id].clone();
        let at = geometry(&app, id).rect.centre;
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(app.editor.is_selected(Element::Text(id)));
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
        let to = at + DVec2::new(80., -30.);
        crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
        assert_eq!(app.tools.preview(&app.editor).moving_texts.len(), 1);
        assert_eq!(app.editor.active().texts[&id], original);
        crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Down { clicks: 1 });
        assert_eq!(app.editor.undo_label(), Some("Move"));
        let rysmo_model::text::TextPlacement::Attached { anchor: old, .. } = &original.placement else { panic!() };
        let rysmo_model::text::TextPlacement::Attached { anchor: new, .. } = &app.editor.active().texts[&id].placement
        else {
            panic!()
        };
        assert_eq!(old.reference, new.reference);
        assert_eq!(old.point, new.point);
        app.vcb = "100 mm".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.undo_label(), Some("Move"));
        app.editor.undo();
        assert_eq!(app.editor.active().texts[&id], original);
    }
    #[test]
    fn ctrl_move_text_copy_supports_precise_distance_arrays_and_undo() {
        let mut app = fixture();
        let id = TextId(1003);
        app.set_tool(rysmo_tools::ToolKind::Move);
        app.ctrl_toggle = true;
        let before = app.editor.doc().clone();
        let original = app.editor.active().texts[&id].clone();
        let at = geometry(&app, id).rect.centre;
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
        let to = at + DVec2::new(80., -30.);
        crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
        assert_eq!(app.tools.preview(&app.editor).moving_texts.len(), 1);
        crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Down { clicks: 1 });
        assert_eq!(app.editor.undo_label(), Some("Copy"));
        assert_eq!(app.editor.active().texts.len(), 6);
        assert_eq!(app.editor.active().texts[&id], original);
        app.vcb = "100 mm".into();
        crate::input::apply_vcb(&mut app);
        app.vcb = "3x".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.active().texts.len(), 8);
        assert_eq!(app.editor.active().texts[&id], original);
        app.vcb = "/2".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.active().texts.len(), 7);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn rotate_text_preview_precise_angle_correction_and_undo() {
        let mut app = fixture();
        let id = TextId(1004);
        let original = app.editor.active().texts[&id].clone();
        app.editor.set_selection([Element::Text(id)]);
        app.set_tool(rysmo_tools::ToolKind::Rotate);
        for point in [DVec3::ZERO, DVec3::X * 2.] {
            let at = app.view().project(point).unwrap();
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
        }
        let at = app.view().project(DVec3::Y * 2.).unwrap();
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Move);
        assert_eq!(app.tools.preview(&app.editor).moving_texts.len(), 1);
        assert_eq!(app.editor.active().texts[&id], original);
        app.vcb = "90".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.undo_label(), Some("Rotate"));
        app.vcb = "45".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.undo_label(), Some("Rotate"));
        app.editor.undo();
        assert_eq!(app.editor.active().texts[&id], original);
    }
    #[test]
    fn ctrl_rotate_text_supports_radial_array_and_undo() {
        let mut app = fixture();
        let before = app.editor.doc().clone();
        let id = TextId(1004);
        app.editor.set_selection([Element::Text(id)]);
        app.set_tool(rysmo_tools::ToolKind::Rotate);
        app.ctrl_toggle = true;
        for point in [DVec3::ZERO, DVec3::X * 2.] {
            let at = app.view().project(point).unwrap();
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
        }
        let at = app.view().project(DVec3::Y * 2.).unwrap();
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Move);
        assert_eq!(app.tools.preview(&app.editor).moving_texts.len(), 1);
        app.vcb = "90".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.active().texts.len(), 6);
        app.vcb = "3x".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.active().texts.len(), 8);
        app.vcb = "/2".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.active().texts.len(), 7);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn scale_text_by_grip_and_precise_factor_with_preview_and_undo() {
        let mut app = fixture();
        let id = TextId(1004);
        let original = app.editor.active().texts[&id].clone();
        app.editor.set_selection([Element::Text(id)]);
        app.set_tool(rysmo_tools::ToolKind::Scale);
        let at = app.view().project(DVec3::new(3., 1., 1.1)).unwrap();
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        crate::input::tool_pointer(&mut app, at, rysmo_tools::Pointer::Up);
        let to = app.view().project(DVec3::new(3., 1., 2.2)).unwrap();
        crate::input::tool_pointer(&mut app, to, rysmo_tools::Pointer::Move);
        assert_eq!(app.tools.preview(&app.editor).moving_texts.len(), 1);
        app.vcb = "2".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.undo_label(), Some("Scale"));
        assert_eq!(app.editor.active().texts[&id].font, original.font);
        app.vcb = "1.5".into();
        crate::input::apply_vcb(&mut app);
        assert_eq!(app.editor.undo_label(), Some("Scale"));
        app.editor.undo();
        assert_eq!(app.editor.active().texts[&id], original);
    }
}

#[cfg(test)]
mod canvas_tests {
    use super::*;
    #[test]
    fn selection_uses_canvas_coordinates_with_a_docked_inspector() {
        let mut app = App3d::new(Editor::new());
        app.viewport = DVec2::new(1280., 900.);
        app.canvas = Some((DVec2::new(100., 80.), DVec2::new(640., 480.)));
        app.editor
            .apply(rysmo_model::Command::DrawRectangle {
                corner: DVec3::ZERO,
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.camera.set_standard_view(rysmo_math::StandardView::Top);
        app.camera.target = DVec3::new(1., 1., 0.);
        crate::input::tool_pointer(&mut app, DVec2::new(420., 320.), rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(app.editor.is_selected(rysmo_model::Element::Face(face)));
    }
}

/// Liang-Barsky clipping against the selection rectangle.
fn section_line_box(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> bool {
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
}

#[cfg(test)]
mod construction_modifier_tests {
    use super::*;
    #[test]
    fn tape_ctrl_toggle_survives_release_and_clears_on_tool_switch() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.set_tool(ToolKind::TapeMeasure);
        app.ctrl_toggle = true;
        app.held.ctrl = false;
        assert!(app.modifiers().ctrl);
        app.ctrl_toggle = false;
        assert!(!app.modifiers().ctrl);
        app.ctrl_toggle = true;
        app.set_tool(ToolKind::Line);
        app.set_tool(ToolKind::TapeMeasure);
        assert!(!app.modifiers().ctrl);
    }
}

#[cfg(test)]
mod recovery_startup_tests {
    use super::*;
    #[test]
    fn recovery_uses_the_actual_startup_generation_and_rejects_new_work() {
        let mut app = App3d::new(Editor::new());
        let startup = app.document_generation;
        assert!(app.accepts_recovery(startup));
        app.editor.mark_dirty();
        assert!(!app.accepts_recovery(startup));
        app.editor.mark_saved();
        assert!(app.accepts_recovery(startup));
        app.path = Some("chosen.rysmo".into());
        assert!(!app.accepts_recovery(startup));
        crate::files::new(&mut app);
        assert!(!app.accepts_recovery(startup));
    }
}
