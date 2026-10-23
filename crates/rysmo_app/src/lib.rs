//! Rysmo: a simple 3D modeller.
//!
//! Usage: `rysmo [model.rysmo] [--mcp-port PORT]`, or `rysmo --mcp [model.rysmo]`
//! to serve MCP on stdio without a window (see docs/mcp.md).
//!
//! Environment variables, mostly for testing:
//! - `RYSMO_DEMO=1` starts with a small demo model;
//! - `RYSMO_TOUCH=1` uses the touch layout from the start;
//! - `RYSMO_SCREENSHOT=out.png` saves a screenshot after a moment and quits
//!   (`RYSMO_SCREENSHOT_FRAME=n` picks the frame, 90 by default);
//! - `RYSMO_TRACE=1` prints the clicks the tools receive.

#![forbid(unsafe_code)]

mod actions;
#[cfg(any(target_arch = "wasm32", test))]
mod autosave;
mod brand;
mod command_search;
mod component_panel;
mod detail_tool;
mod drape_tool;
mod environment;
mod faces;
mod features;
mod files;
mod fillet;
mod flip_tool;
mod help;
mod icons;
mod idle;
mod input;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod input_script;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod live_mcp;
#[cfg(target_os = "macos")]
mod macos;
mod menu;
mod mirror;
mod object_clipboard;
mod panels;
mod portable_files;
mod preferences;
mod print_check;
mod render;
mod render_view;
#[cfg(target_os = "ios")]
mod safe_area;
mod scene_panel;
mod section_panel;
mod smoove_tool;
mod solid_tools;
mod stamp_tool;
mod state;
mod studio;
mod tablet;
mod terrain_tool;
mod text3d_tool;
mod text_tool;
mod texture_pins;
mod texture_view;
mod theme;
mod tool_panel;
mod ui;
mod unsaved;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod verification;
mod viewport;
#[cfg(target_arch = "wasm32")]
mod web;
mod welcome;
mod widgets;

use bevy::prelude::*;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_egui::EguiPlugin;
use rysmo_math::DVec3;
use rysmo_model::{Command, Editor, Element};
use rysmo_tools::ToolKind;

use crate::state::App3d;

/// Command-line options (desktop only).
#[derive(Default)]
struct Options {
    file: Option<String>,
    /// Serve MCP on stdio without opening a window.
    #[cfg_attr(any(target_arch = "wasm32", target_os = "android", target_os = "ios"), allow(dead_code))]
    mcp_stdio: bool,
    /// Serve MCP for the open window on this local port (0 picks a free one).
    #[cfg_attr(any(target_arch = "wasm32", target_os = "android", target_os = "ios"), allow(dead_code))]
    mcp_port: Option<u16>,
}

fn options() -> Options {
    #[cfg_attr(any(target_arch = "wasm32", target_os = "android", target_os = "ios"), allow(unused_mut))]
    let mut o = Options::default();
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--mcp" => o.mcp_stdio = true,
                "--mcp-port" => o.mcp_port = args.next().and_then(|p| p.parse().ok()),
                _ if !a.starts_with("--") => o.file = Some(a),
                _ => eprintln!("rysmo: unknown option {a}"),
            }
        }
        if o.mcp_port.is_none() {
            o.mcp_port = std::env::var("RYSMO_MCP_PORT").ok().and_then(|p| p.parse().ok());
        }
    }
    o
}

pub fn run() {
    let opts = options();
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if opts.mcp_stdio {
        // Headless: the same server as `rysmo-mcp`, no window, no GPU.
        let mut session = rysmo_mcp::Session::new();
        if let Some(path) = &opts.file {
            match std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|t| rysmo_io::load(&t).map_err(|e| e.to_string()))
            {
                Ok(doc) => {
                    session.editor.replace_document(doc);
                    session.path = Some(path.into());
                }
                Err(e) => {
                    eprintln!("rysmo: cannot open {path}: {e}");
                    std::process::exit(1);
                }
            }
        }
        if let Err(e) = rysmo_mcp::run_stdio(session) {
            eprintln!("rysmo: {e}");
            std::process::exit(1);
        }
        return;
    }

    let mut app3d = App3d::new(Editor::new());
    app3d.preferences = preferences::Preferences::load();
    if let Some(path) = &opts.file {
        match files::open_path(&mut app3d, std::path::Path::new(path)) {
            Ok(m) => app3d.message = Some(m),
            Err(e) => app3d.message = Some(e),
        }
    } else if demo_requested() {
        demo(&mut app3d);
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_BRAND_PREVIEW").is_some() {
        app3d.show_about = true;
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_DIALOG_PREVIEW").is_some() {
        text3d_tool::begin(&mut app3d);
        if let Some(draft) = app3d.text3d.as_mut() {
            draft.content = "Rysmo".into();
        }
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if let Ok(mode) = std::env::var("RYSMO_BASICS_PREVIEW") {
        verification::basics(&mut app3d, &mode);
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_HELP_PREVIEW").is_some() {
        app3d.help.open = true;
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_UNITS_PREVIEW").is_some() {
        app3d.show_settings = true;
        app3d.settings_tab = 1;
        app3d.editor.apply(Command::SetUnits { units: rysmo_model::Units::Inches }).unwrap();
        app3d.editor.apply(Command::SetLengthPrecision { precision: Some(3) }).unwrap();
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_CLIPBOARD_PREVIEW").is_some() {
        object_clipboard::screenshot_preview(&mut app3d);
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_WRAPPED_TEXTURE_PREVIEW").is_some() {
        let entry = rysmo_library::materials().into_iter().find(|m| m.texture.is_some()).unwrap();
        let texture = entry.texture.unwrap();
        let material = app3d
            .editor
            .apply(Command::AddTexturedMaterial {
                name: entry.name.into(),
                color: entry.color,
                image_name: texture.file.into(),
                mime: "image/png".into(),
                data: texture.png,
                size: [0.8, 0.25],
            })
            .unwrap()
            .material;
        let instance = *app3d.editor.active().instances.keys().next().unwrap();
        app3d.editor.enter(instance).unwrap();
        let faces = app3d
            .editor
            .mesh()
            .faces()
            .filter(|(f, _)| app3d.editor.mesh().face_normal(*f).is_some_and(|n| n.z.abs() < 0.1))
            .map(|(f, _)| f)
            .collect::<Vec<_>>();
        app3d
            .editor
            .apply(Command::PaintWrappedElements {
                elements: faces.iter().map(|f| Element::Face(*f)).collect(),
                material,
                back: false,
                source: faces[0],
                placement: rysmo_model::TexturePlacement { rotation: 90., ..Default::default() },
            })
            .unwrap();
        app3d.editor.exit();
        app3d.editor.clear_selection();
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && (std::env::var_os("RYSMO_TEXTURE_PIN_PREVIEW").is_some()
            || std::env::var_os("RYSMO_TEXTURE_VIEW_PREVIEW").is_some())
    {
        let entry = rysmo_library::materials().into_iter().find(|m| m.texture.is_some()).unwrap();
        let tx = entry.texture.unwrap();
        let material = app3d
            .editor
            .apply(Command::AddTexturedMaterial {
                name: entry.name.into(),
                color: entry.color,
                image_name: tx.file.into(),
                mime: "image/png".into(),
                data: tx.png,
                size: if std::env::var_os("RYSMO_TEXTURE_VIEW_PREVIEW").is_some() { [1., 1.] } else { tx.size },
            })
            .unwrap()
            .material;
        let faces = app3d.editor.mesh().faces().map(|(id, _)| id).collect::<Vec<_>>();
        app3d.editor.apply(Command::Paint { faces: faces.clone(), material, back: false }).unwrap();
        app3d
            .editor
            .apply(Command::SetTexturePlacement {
                faces,
                placement: Some(rysmo_model::TexturePlacement {
                    pins: Some([[0., 0.], [1., 0.], [1.3, 1.], [0.1, 0.8]]),
                    ..Default::default()
                }),
            })
            .unwrap();
        app3d.editor.clear_selection();
        if std::env::var_os("RYSMO_TEXTURE_VIEW_PREVIEW").is_some() {
            let face = app3d
                .editor
                .mesh()
                .faces()
                .find(|(id, _)| app3d.editor.mesh().face_normal(*id).is_some_and(|n| n.y < -0.8))
                .map(|(id, _)| id)
                .unwrap();
            app3d.editor.apply(Command::SetTexturePlacement { faces: vec![face], placement: None }).unwrap();
            app3d.editor.set_selection([Element::Face(face)]);
            texture_view::begin(&mut app3d);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_EDGE_MATERIAL_PREVIEW").is_some() {
        let material = app3d
            .editor
            .apply(Command::AddMaterial { name: "Orange edges".into(), color: [1., 0.35, 0.03, 1.], texture: None })
            .unwrap()
            .material;
        let elements = app3d.editor.mesh().edges().map(|(id, _)| rysmo_model::Element::Edge(id)).collect();
        let _ = app3d.editor.apply(Command::PaintElements { elements, material, back: false });
        let style = rysmo_model::Style { edge_color: rysmo_model::EdgeColor::ByMaterial, ..app3d.editor.doc().style };
        let _ = app3d.editor.apply(Command::SetStyle { style });
        app3d.editor.clear_selection();
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_RENDERER_PREVIEW").is_some() {
        app3d.show_settings = true;
        let style = rysmo_model::Style {
            lighting: rysmo_model::Lighting::Sun,
            shadows: true,
            shadow_quality: rysmo_model::ShadowQuality::Medium,
            anti_aliasing: rysmo_model::AntiAliasing::Fxaa,
            ..Default::default()
        };
        let _ = app3d.editor.apply(Command::SetStyle { style });
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && std::env::var_os("RYSMO_COMPONENTS_PREVIEW").is_some()
        && let Some(definition) = app3d.editor.doc().definitions.iter().find(|(_, d)| !d.group).map(|(id, _)| *id)
    {
        let instances = app3d.editor.doc().root.instances.keys().copied().collect::<Vec<_>>();
        if let Some(id) = instances.first() {
            app3d
                .editor
                .apply(rysmo_model::Command::MakeGroup { elements: vec![rysmo_model::Element::Instance(*id)] })
                .unwrap();
        }
        if let Some(id) = instances.last() {
            app3d
                .editor
                .apply(rysmo_model::Command::SetInstancesHidden { instances: vec![*id], hidden: true })
                .unwrap();
        }
        app3d.show_components = true;
        app3d.inspector_tab = 0;
        app3d.component_selected = Some(definition);
        app3d.component_edit = Some((
            definition,
            app3d.editor.doc().definitions[&definition].name.clone(),
            app3d.editor.doc().definitions[&definition].component.clone(),
        ));
        component_panel::begin(&mut app3d, definition).unwrap();
        app3d.component_place.as_mut().unwrap().origin = rysmo_math::DVec3::new(7.5, 2., 0.);
        app3d.camera.frame(rysmo_math::DVec3::new(-0.5, -0.5, 0.), rysmo_math::DVec3::new(9., 3.5, 1.2));
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_FOLLOW_ME_PREVIEW").is_some() {
        use rysmo_math::DVec3;
        use rysmo_model::Command;
        app3d.editor = Editor::new();
        app3d
            .editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(0., -0.2, 0.),
                u: DVec3::new(0., 0.4, 0.),
                v: DVec3::new(0., 0., 0.3),
                facing: Some(-DVec3::X),
            })
            .unwrap();
        app3d
            .editor
            .apply(Command::DrawLines {
                lines: vec![
                    [DVec3::new(0., 0.5, 0.), DVec3::new(3., 0.5, 0.)],
                    [DVec3::new(3., 0.5, 0.), DVec3::new(3., 3., 0.)],
                ],
                facing: None,
            })
            .unwrap();
        app3d.camera.target = DVec3::new(1.5, 1.2, 0.2);
        app3d.camera.distance = 8.;
        app3d.set_tool(rysmo_tools::ToolKind::FollowMe);
        for (point, event) in [
            (DVec3::new(0., 0., 0.15), rysmo_tools::Pointer::Move),
            (DVec3::new(0., 0., 0.15), rysmo_tools::Pointer::Down { clicks: 1 }),
            (DVec3::new(0., 0., 0.15), rysmo_tools::Pointer::Up),
            (DVec3::new(1.5, 0.5, 0.), rysmo_tools::Pointer::Move),
            (DVec3::new(3., 1.5, 0.), rysmo_tools::Pointer::Move),
        ] {
            let at = app3d.view().project(point).unwrap();
            input::tool_pointer(&mut app3d, at, event);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_OFFSET_HOLES_PREVIEW").is_some() {
        app3d.editor = Editor::new();
        for (corner, size) in [(rysmo_math::DVec3::ZERO, 4.), (rysmo_math::DVec3::new(1.5, 1.5, 0.), 1.)] {
            app3d
                .editor
                .apply(rysmo_model::Command::DrawRectangle {
                    corner,
                    u: rysmo_math::DVec3::X * size,
                    v: rysmo_math::DVec3::Y * size,
                    facing: None,
                })
                .unwrap();
        }
        app3d.camera.target = rysmo_math::DVec3::new(2., 2., 0.);
        app3d.camera.distance = 8.;
        app3d.camera.parallel = true;
        app3d.camera.set_standard_view(rysmo_math::StandardView::Top);
        app3d.set_tool(rysmo_tools::ToolKind::Offset);
        let at = app3d.view().project(rysmo_math::DVec3::new(0.8, 0.8, 0.)).unwrap();
        input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
        input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
        let at = app3d.view().project(rysmo_math::DVec3::new(1.25, 2., 0.)).unwrap();
        input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_SCENES_PREVIEW").is_some() {
        for (name, view) in [("Perspective", rysmo_math::StandardView::Iso), ("Top", rysmo_math::StandardView::Top)] {
            let mut camera = app3d.camera;
            camera.set_standard_view(view);
            let scene = rysmo_model::scenes::Scene::capture(app3d.editor.doc(), camera, name.into());
            app3d.editor.apply(rysmo_model::Command::AddScene { scene }).unwrap();
        }
        let id = app3d.editor.doc().scenes[0].id;
        scene_panel::restore(&mut app3d, id);
        app3d.show_scenes = true;
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_SECTION_PREVIEW").is_some() {
        app3d.editor = Editor::new();
        let _ = app3d.editor.apply(Command::DrawRectangle {
            corner: DVec3::ZERO,
            u: DVec3::X * 4.,
            v: DVec3::Y * 4.,
            facing: Some(DVec3::Z),
        });
        let face = app3d.editor.mesh().faces().next().map(|(id, _)| id);
        if let Some(face) = face {
            let _ = app3d.editor.apply(Command::PushPull { face, distance: -4., keep_base: false });
        }
        let _ = app3d.editor.apply(Command::AddSection {
            plane: rysmo_model::sections::SectionPlane {
                name: "Oblique cut".into(),
                origin: DVec3::splat(2.),
                normal: DVec3::new(1., 0.2, 0.1).normalize(),
            },
        });
        app3d.show_sections = true;
        app3d.camera.frame(DVec3::ZERO, DVec3::splat(4.));
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_DRAPE_PREVIEW").is_some() {
        app3d.editor = Editor::new();
        let _ = app3d.editor.apply(Command::CreateTerrainGrid {
            origin: DVec3::ZERO,
            u: DVec3::X * 6.,
            v: DVec3::Y * 6.,
            spacing: 1.,
        });
        let mut document = app3d.editor.doc().clone();
        for definition in document.definitions.values_mut().map(std::sync::Arc::make_mut) {
            definition.mesh.map_all(|p| {
                DVec3::new(
                    p.x,
                    p.y,
                    0.8 * (p.x * std::f64::consts::PI / 6.).sin() * (p.y * std::f64::consts::PI / 6.).sin(),
                )
            });
        }
        app3d.editor.replace_document(document);
        let _ = app3d.editor.apply(Command::DrawRectangle {
            corner: DVec3::new(1.5, 1.5, 3.),
            u: DVec3::X * 3.,
            v: DVec3::Y * 3.,
            facing: Some(DVec3::Z),
        });
        let sources = app3d.editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect::<Vec<_>>();
        app3d.editor.set_selection(sources);
        app3d.camera.set_standard_view(rysmo_math::StandardView::Top);
        app3d.camera.frame(DVec3::ZERO, DVec3::new(6., 6., 4.));
        if crate::drape_tool::begin(&mut app3d).is_ok()
            && let Some(at) = app3d.local_view().project(DVec3::new(1., 1., 0.2))
        {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
        app3d.camera.set_standard_view(rysmo_math::StandardView::Iso);
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_STAMP_PREVIEW").is_some() {
        app3d.editor = Editor::new();
        let hole = std::env::var_os("RYSMO_STAMP_HOLE_PREVIEW").is_some();
        let extent = if hole { 10. } else { 6. };
        let source_size = if hole { 6. } else { 2. };
        if std::env::var_os("RYSMO_STAMP_RAW_PREVIEW").is_some() {
            let mut document = rysmo_model::Document::default();
            document.with_mesh(None, |mesh| *mesh = rysmo_model::terrain::grid(extent, extent, 1.).unwrap()).unwrap();
            app3d.editor.replace_document(document);
        } else {
            let _ = app3d.editor.apply(Command::CreateTerrainGrid {
                origin: DVec3::ZERO,
                u: DVec3::X * extent,
                v: DVec3::Y * extent,
                spacing: 1.,
            });
        }
        let _ = app3d.editor.apply(Command::DrawRectangle {
            corner: DVec3::new(2., 2., 4.),
            u: DVec3::X * source_size,
            v: DVec3::Y * source_size,
            facing: Some(DVec3::Z),
        });
        if hole {
            let _ = app3d.editor.apply(Command::DrawRectangle {
                corner: DVec3::new(4., 4., 4.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            });
        }
        let source_face = app3d
            .editor
            .mesh()
            .faces()
            .find(|(_, face)| face.outer.len() == 4 && (!hole || !face.holes.is_empty()))
            .map(|(id, _)| id);
        if let Some(face) = source_face {
            app3d.editor.set_selection([Element::Face(face)]);
            app3d.camera.frame(DVec3::ZERO, DVec3::new(extent, extent, 5.));
            if crate::stamp_tool::begin(&mut app3d).is_ok() {
                if std::env::var_os("RYSMO_STAMP_SOURCE_PREVIEW").is_none() {
                    if let Some(at) = app3d.local_view().project(DVec3::new(1., 1., 0.)) {
                        input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                    }
                    let _ = crate::stamp_tool::value(&mut app3d, "2m");
                }
                let _ = crate::stamp_tool::value(&mut app3d, if hole { "1.5mo" } else { "0.5mo" });
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_ADD_DETAIL_PREVIEW").is_some() {
        app3d.editor = Editor::new();
        if let Ok(out) = app3d.editor.apply(Command::CreateTerrainGrid {
            origin: DVec3::ZERO,
            u: DVec3::X * 4.,
            v: DVec3::Y * 4.,
            spacing: 2.,
        }) && let Some(Element::Instance(instance)) = out.created.first()
        {
            let _ = app3d.editor.enter(*instance);
            app3d.camera.frame(DVec3::ZERO, DVec3::new(4., 4., 1.));
            crate::detail_tool::begin(&mut app3d);
            if let Some(at) = app3d.view().project(DVec3::new(1.5, 0.5, 0.)) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
            }
            if let Some(at) = app3d.view().project(DVec3::new(1.5, 0.5, 0.8)) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_FLIP_EDGE_PREVIEW").is_some() {
        app3d.editor = Editor::new();
        if let Ok(out) = app3d.editor.apply(Command::CreateTerrainGrid {
            origin: DVec3::ZERO,
            u: DVec3::X * 4.,
            v: DVec3::Y * 4.,
            spacing: 1.,
        }) && let Some(Element::Instance(instance)) = out.created.first()
        {
            let _ = app3d.editor.enter(*instance);
            app3d.camera.frame(DVec3::ZERO, DVec3::new(4., 4., 0.));
            crate::flip_tool::begin(&mut app3d);
            if let Some(at) = app3d.view().project(DVec3::new(1.5, 1.5, 0.)) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_CONTOURS_DEMO").is_some() {
        app3d.editor = Editor::new();
        let lines = (0..5)
            .map(|row| {
                [
                    DVec3::new(0., row as f64 * 2., (row as f64 * 0.7).sin() * 2. + 1.),
                    DVec3::new(10., row as f64 * 2., (row as f64 * 0.7).sin() * 2. + 1.),
                ]
            })
            .collect();
        let _ = app3d.editor.apply(Command::DrawLines { lines, facing: None });
        let selected = app3d.editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect::<Vec<_>>();
        app3d.editor.set_selection(selected);
        actions::run(&mut app3d, actions::Action::FromContours);
        app3d.camera.frame(DVec3::ZERO, DVec3::new(10., 8., 3.));
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_TERRAIN_GRID_PREVIEW").is_some() {
        app3d.camera.frame(DVec3::ZERO, DVec3::new(10., 6., 0.));
        crate::terrain_tool::begin(&mut app3d);
        let _ = crate::terrain_tool::value(&mut app3d, "1m");
        for point in [DVec3::ZERO, DVec3::X * 10.] {
            if let Some(at) = app3d.view().project(point) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
            }
        }
        if let Some(at) = app3d.view().project(DVec3::new(10., 6., 0.)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
        if std::env::var_os("RYSMO_TERRAIN_GRID_CORRECTION").is_some() {
            if let Some(at) = app3d.view().project(DVec3::new(10., 6., 0.)) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
            }
            let _ = crate::terrain_tool::correct(&mut app3d, "8m,4m");
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && (std::env::var_os("RYSMO_TERRAIN_GRID_DEMO").is_some()
            || std::env::var_os("RYSMO_SMOOVE_DEMO").is_some()
            || std::env::var_os("RYSMO_SMOOVE_PREVIEW").is_some())
    {
        app3d.editor = Editor::new();
        if let Ok(out) = app3d.editor.apply(Command::CreateTerrainGrid {
            origin: DVec3::ZERO,
            u: DVec3::X * 10.,
            v: DVec3::Y * 6.,
            spacing: 1.,
        }) && let Some(Element::Instance(instance)) = out.created.first()
        {
            let definition = app3d.editor.active().instances[instance].definition;
            if std::env::var_os("RYSMO_SMOOVE_DEMO").is_some() && app3d.editor.enter(*instance).is_ok() {
                let vertices = app3d.editor.mesh().vertices().map(|(id, _)| id).collect();
                let _ = app3d.editor.apply(Command::Smoove {
                    vertices,
                    centres: vec![DVec3::new(5., 3., 0.)],
                    radius: 3.,
                    normal: DVec3::Z,
                    offset: DVec3::Z * 2.,
                });
                app3d.editor.exit();
            }
            if let Some((lo, hi)) = app3d.editor.doc().definition_bounds(Some(definition)) {
                app3d.camera.frame(lo, hi);
            }
            if std::env::var_os("RYSMO_SMOOVE_PREVIEW").is_some() && app3d.editor.enter(*instance).is_ok() {
                app3d.camera.frame(DVec3::ZERO, DVec3::new(10., 6., 2.));
                crate::smoove_tool::begin(&mut app3d);
                let _ = crate::smoove_tool::value(&mut app3d, "3mr");
                if let Some(at) = app3d.view().project(DVec3::new(5., 3., 0.)) {
                    input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                }
                if let Some(at) = app3d.view().project(DVec3::new(5., 3., 1.5)) {
                    input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
                }
                if std::env::var_os("RYSMO_SMOOVE_CORRECTION").is_some() {
                    let _ = crate::smoove_tool::finish(&mut app3d);
                    let _ = crate::smoove_tool::value(&mut app3d, "2m");
                    let _ = crate::smoove_tool::value(&mut app3d, "2mr");
                }
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_3D_TEXT_DIALOG").is_some() {
        crate::text3d_tool::begin(&mut app3d);
        if let Some(draft) = &mut app3d.text3d {
            draft.content = "Rysmo\nPřízemí 8".into();
            draft.height = "50cm".into();
            draft.depth = "8cm".into();
            draft.settings.bold = true;
            draft.settings.font = rysmo_model::text3d::Text3dFont::Serif;
            draft.settings.align = rysmo_model::text3d::Text3dAlign::Centre;
        }
        if std::env::var_os("RYSMO_3D_TEXT_PLACEMENT").is_some() && crate::text3d_tool::prepare(&mut app3d).is_ok() {
            if let Some((lo, hi)) = app3d.text3d.as_ref().and_then(|d| d.mesh.as_ref()).and_then(|m| m.bounds()) {
                app3d.camera.frame(lo, hi);
            }
            if let Some(at) = app3d.view().project(DVec3::ZERO) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_3D_TEXT_DEMO").is_some() {
        app3d.editor = Editor::new();
        if let Ok(out) = app3d.editor.apply(Command::CreateText3d {
            content: "Rysmo\nPřízemí 8".into(),
            settings: rysmo_model::text3d::Text3dSettings { depth: 0.25, ..Default::default() },
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Y,
        }) && let Some(Element::Instance(instance)) = out.created.first()
        {
            let definition = app3d.editor.active().instances[instance].definition;
            if let Some((lo, hi)) = app3d.editor.doc().definition_bounds(Some(definition)) {
                app3d.camera.frame(lo, hi);
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_NEW_TEXT_PREVIEW").is_some() {
        app3d.set_tool(ToolKind::Text);
        if let Some(at) = app3d.view().project(DVec3::new(0.7, 0.6, 0.)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 2 });
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_SHOW_STYLES").is_some() {
        app3d.show_settings = true;
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_GROUP_TEXT_PREVIEW").is_some() {
        let text = rysmo_model::TextId(1004);
        if app3d.editor.active().texts.contains_key(&text) {
            let mut elements: Vec<_> =
                app3d.editor.mesh().curve_edges(rysmo_mesh::CurveId(45)).into_iter().map(Element::Edge).collect();
            elements.push(Element::Text(text));
            if let Ok(out) = app3d.editor.apply(Command::MakeGroup { elements })
                && let Some(Element::Instance(group)) = out.created.first()
            {
                app3d.editor.set_selection([Element::Instance(*group)]);
                app3d.set_tool(ToolKind::Move);
                if let Some(at) = app3d.view().project(DVec3::ZERO) {
                    input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                    input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
                }
                if let Some(at) = app3d.view().project(DVec3::new(0., 0.9, 0.)) {
                    input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
                }
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_SCALE_TEXT_PREVIEW").is_some() {
        app3d.editor.set_selection([Element::Text(rysmo_model::TextId(1004))]);
        app3d.set_tool(ToolKind::Scale);
        if let Some(at) = app3d.view().project(DVec3::new(3., 1., 1.1)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
        }
        if let Some(at) = app3d.view().project(DVec3::new(3., 1., 1.65)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_ROTATE_TEXT_PREVIEW").is_some() {
        app3d.editor.set_selection([Element::Text(rysmo_model::TextId(1004))]);
        app3d.set_tool(ToolKind::Rotate);
        for point in [DVec3::new(1.5, 1.5, 0.), DVec3::new(3.5, 1.5, 0.)] {
            if let Some(at) = app3d.view().project(point) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
            }
        }
        if let Some(at) = app3d.view().project(DVec3::new(1.5, 3.5, 0.)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && let Ok(id) = std::env::var("RYSMO_MOVE_TEXT_PREVIEW")
        && let Ok(id) = id.parse::<u64>()
    {
        app3d.set_tool(ToolKind::Move);
        if std::env::var_os("RYSMO_COPY_TEXT_PREVIEW").is_some() {
            app3d.ctrl_toggle = true;
        }
        app3d.editor.set_selection([Element::Text(rysmo_model::TextId(id))]);
        if let Some(at) = app3d.view().project(DVec3::ZERO) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
        }
        if let Some(at) = app3d.view().project(DVec3::new(0., 0.9, 0.)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && let Ok(id) = std::env::var("RYSMO_INSPECT_TEXT")
        && let Ok(id) = id.parse::<u64>()
    {
        app3d.editor.set_selection([Element::Text(rysmo_model::TextId(id))]);
        app3d.show_entity_info = true;
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && let Ok(id) = std::env::var("RYSMO_INSPECT_DIMENSION")
        && let Ok(id) = id.parse::<u64>()
    {
        app3d.editor.set_selection([Element::Dimension(rysmo_model::DimensionId(id))]);
        app3d.show_entity_info = true;
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && let Ok(id) = std::env::var("RYSMO_PREVIEW_DIMENSION_MOVE")
        && let Ok(id) = id.parse::<u64>()
    {
        app3d.editor.set_selection([Element::Dimension(rysmo_model::DimensionId(id))]);
        app3d.set_tool(ToolKind::Move);
        if let Some(at) = app3d.local_view().project(DVec3::ZERO) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
        }
        if let Some(at) = app3d.local_view().project(DVec3::new(0., -0.9, 0.)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && let Ok(id) = std::env::var("RYSMO_PREVIEW_DIMENSION_ROTATE")
        && let Ok(id) = id.parse::<u64>()
    {
        let id = rysmo_model::DimensionId(id);
        let mut elements = vec![Element::Dimension(id)];
        if let Some(dimension) = app3d.editor.active().dimensions.get(&id)
            && let rysmo_model::dimensions::DimensionMeasure::Radial { reference, .. } = &dimension.measure
            && reference.path.is_empty()
            && let Some(curve) = reference.curve
        {
            elements.extend(app3d.editor.mesh().curve_edges(curve).into_iter().map(Element::Edge));
        }
        app3d.editor.set_selection(elements);
        app3d.set_tool(ToolKind::Rotate);
        for point in [DVec3::ZERO, DVec3::X * 2.] {
            if let Some(at) = app3d.local_view().project(point) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
            }
        }
        if let Some(at) = app3d.local_view().project(DVec3::Y * 2.) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && let Ok(id) = std::env::var("RYSMO_PREVIEW_DIMENSION_SCALE")
        && let Ok(id) = id.parse::<u64>()
    {
        let id = rysmo_model::DimensionId(id);
        let mut elements = vec![Element::Dimension(id)];
        let mut points = Vec::new();
        if let Some(dimension) = app3d.editor.active().dimensions.get(&id) {
            if let Some(layout) = dimension.layout(app3d.editor.doc(), None, rysmo_math::DMat4::IDENTITY, 0.) {
                points.extend(layout.segments.iter().flatten().copied());
                points.push(layout.text_position);
            }
            if let rysmo_model::dimensions::DimensionMeasure::Radial { reference, .. } = &dimension.measure
                && reference.path.is_empty()
                && let Some(curve) = reference.curve
            {
                for edge in app3d.editor.mesh().curve_edges(curve) {
                    elements.push(Element::Edge(edge));
                    let (a, b) = app3d.editor.mesh().edge_points(edge);
                    points.extend([a, b]);
                }
            }
        }
        if let Some(first) = points.first().copied() {
            let (lo, hi) = points.into_iter().fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
            let handle = DVec3::new(hi.x, (lo.y + hi.y) * 0.5, (lo.z + hi.z) * 0.5);
            let end = handle + DVec3::X * (hi.x - lo.x);
            app3d.editor.set_selection(elements);
            app3d.set_tool(ToolKind::Scale);
            if let Some(at) = app3d.local_view().project(handle) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
            }
            if let Some(at) = app3d.local_view().project(end) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some()
        && let Ok(id) = std::env::var("RYSMO_PREVIEW_GROUP_DIMENSION")
        && let Ok(id) = id.parse::<u64>()
    {
        let id = rysmo_model::DimensionId(id);
        let mut elements = vec![Element::Dimension(id)];
        if let Some(dimension) = app3d.editor.active().dimensions.get(&id)
            && let rysmo_model::dimensions::DimensionMeasure::Radial { reference, .. } = &dimension.measure
            && reference.path.is_empty()
            && let Some(curve) = reference.curve
        {
            elements.extend(app3d.editor.mesh().curve_edges(curve).into_iter().map(Element::Edge));
        }
        if let Ok(out) = app3d.editor.apply(Command::MakeGroup { elements }) {
            app3d.editor.set_selection(out.created);
            app3d.set_tool(ToolKind::Move);
            if let Some(at) = app3d.local_view().project(DVec3::ZERO) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
            }
            if let Some(at) = app3d.local_view().project(DVec3::new(0., -0.9, 0.)) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_PREVIEW_DIMENSION").is_some() {
        app3d.set_tool(ToolKind::Dimension);
        for point in [DVec3::ZERO, DVec3::X * 2.] {
            if let Some(at) = app3d.local_view().project(point) {
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Down { clicks: 1 });
                input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Up);
            }
        }
        if let Some(at) = app3d.local_view().project(DVec3::new(1., -0.9, 0.)) {
            input::tool_pointer(&mut app3d, at, rysmo_tools::Pointer::Move);
        }
    }

    app3d.touch.enabled |= app3d.preferences.touch;
    #[cfg(target_arch = "wasm32")]
    {
        app3d.touch.enabled |= web::touch_device();
    }
    app3d.show_entity_info |= app3d.preferences.remember_layout && app3d.preferences.inspector;
    app3d.inspector_tab = app3d.preferences.inspector_tab;
    if app3d.preferences.remember_layout {
        let p = app3d.preferences.panels;
        app3d.show_materials |= p[0];
        app3d.show_tags |= p[1];
        app3d.show_sections |= p[2];
        app3d.show_scenes |= p[3];
        app3d.show_components |= p[4];
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        app3d.recent = app3d.preferences.recent.clone();
    }
    app3d.welcome =
        app3d.preferences.show_welcome && opts.file.is_none() && opts.mcp_port.is_none() && !demo_requested();
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() {
        app3d.welcome = std::env::var_os("RYSMO_WELCOME_PREVIEW").is_some();
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window()),
        close_when_requested: false,
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    .insert_resource(bevy_egui::EguiGlobalSettings { auto_create_primary_context: false, ..default() })
    .insert_resource(app3d)
    .add_systems(Update, unsaved::close_requests)
    .add_plugins((viewport::plugin, faces::plugin, render::plugin, input::plugin, ui::plugin, idle::plugin));
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        if let Some(path) = std::env::var_os("RYSMO_SCREENSHOT") {
            app.insert_resource(ScreenshotAt(path.into())).add_systems(Update, screenshot);
            app.add_plugins(input_script::plugin);
        }
        if let Some(port) = opts.mcp_port {
            match live_mcp::start(&mut app, port) {
                Ok(info) => {
                    eprintln!("rysmo: MCP on 127.0.0.1:{} with token {}", info.port, info.token);
                    eprintln!(
                        "rysmo: connect with: rysmo-mcp --connect 127.0.0.1:{} --token {}",
                        info.port, info.token
                    );
                    let mut a = app.world_mut().resource_mut::<App3d>();
                    a.message =
                        Some(format!("MCP server on 127.0.0.1:{}. The token is under Window > MCP server.", info.port));
                    a.mcp = Some((info.port, info.token));
                    a.show_mcp = true;
                }
                Err(e) => eprintln!("rysmo: cannot serve MCP on port {port}: {e}"),
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    app.add_plugins(web::plugin);
    #[cfg(target_os = "ios")]
    app.add_plugins(safe_area::plugin);
    #[cfg(target_os = "macos")]
    app.add_plugins(macos::plugin);
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    app.add_systems(Update, brand::window_icon);
    app.run();
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
fn window() -> Window {
    let screenshot = std::env::var_os("RYSMO_SCREENSHOT").is_some();
    // Verification runs can take another window size, such as an upright tablet.
    let requested = std::env::var("RYSMO_WINDOW_SIZE").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    });
    let size = match requested.filter(|_| screenshot) {
        Some(size) => size,
        None if screenshot && std::env::var_os("RYSMO_COMPACT_PREVIEW").is_some() => (960, 420),
        None => (1280, 800),
    };
    Window {
        title: "Rysmo".into(),
        name: Some("rysmo".into()),
        resolution: size.into(),
        // A scripted run must not take the keyboard from whatever the user is typing in.
        focused: !input_script::active(),
        // The menu row continues the title bar, beside the traffic lights.
        #[cfg(target_os = "macos")]
        titlebar_transparent: true,
        #[cfg(target_os = "macos")]
        titlebar_show_title: false,
        #[cfg(target_os = "macos")]
        fullsize_content_view: true,
        ..default()
    }
}

#[cfg(target_os = "android")]
fn window() -> Window {
    Window { title: "Rysmo".into(), ..default() }
}

/// On iOS a windowed size becomes the UIWindow frame (1280×720 points by default), wider
/// and shorter than a phone; borderless fullscreen makes it the screen.
#[cfg(target_os = "ios")]
fn window() -> Window {
    Window {
        title: "Rysmo".into(),
        mode: bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Primary),
        ..default()
    }
}

/// In the browser the window is the page's canvas, sized by the page.
#[cfg(target_arch = "wasm32")]
fn window() -> Window {
    Window {
        title: "Rysmo".into(),
        canvas: Some("#rysmo".into()),
        fit_canvas_to_parent: true,
        prevent_default_event_handling: true,
        ..default()
    }
}

fn demo_requested() -> bool {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        std::env::var_os("RYSMO_DEMO").is_some()
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        false
    }
    #[cfg(target_arch = "wasm32")]
    {
        web::query_has("demo")
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
#[derive(Resource)]
struct ScreenshotAt(std::path::PathBuf);

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
fn screenshot(
    mut commands: Commands,
    at: Res<ScreenshotAt>,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
    mut app: ResMut<App3d>,
) {
    *frame += 1;
    // Let native DPI settle before opening a centred preview window.
    if *frame == 15 && std::env::var_os("RYSMO_SEARCH_PREVIEW").is_some() {
        command_search::open(&mut app);
        app.command_search.query = "rotate".into();
    }
    if *frame == 15 && std::env::var("RYSMO_BASICS_PREVIEW").as_deref() == Ok("rotated") {
        verification::rotated_draft(&mut app);
    }
    if *frame == 15 && std::env::var("RYSMO_BASICS_PREVIEW").as_deref() == Ok("arcs") {
        verification::centre_draft(&mut app);
    }
    if *frame == 15 && std::env::var("RYSMO_BASICS_PREVIEW").as_deref() == Ok("three_point") {
        verification::three_draft(&mut app);
    }
    let at_frame: u32 = std::env::var("RYSMO_SCREENSHOT_FRAME").ok().and_then(|f| f.parse().ok()).unwrap_or(90);
    // An input script takes its own screenshots and decides when to quit.
    if input_script::active() {
        return;
    }
    if *frame == at_frame {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(at.0.clone()));
    }
    if *frame == at_frame + 60 {
        exit.write(AppExit::Success);
    }
}

/// A box with a pocket, a cylinder and the Push/Pull tool active, so a screenshot shows
/// faces, edges, a selection and the UI.
fn demo(app: &mut App3d) {
    let e = &mut app.editor;
    let _ = e.apply(Command::DrawRectangle {
        corner: DVec3::ZERO,
        u: DVec3::new(3.0, 0.0, 0.0),
        v: DVec3::new(0.0, 2.0, 0.0),
        facing: None,
    });
    let base = e.mesh().faces().next().map(|(f, _)| f);
    if let Some(f) = base {
        let _ = e.apply(Command::PushPull { face: f, distance: -1.2, keep_base: false });
    }
    let _ = e.apply(Command::DrawRectangle {
        corner: DVec3::new(0.5, 0.5, 1.2),
        u: DVec3::new(1.2, 0.0, 0.0),
        v: DVec3::new(0.0, 1.0, 0.0),
        facing: None,
    });
    let top = e.mesh().faces().find(|(f, _)| e.mesh().face_contains(*f, DVec3::new(1.0, 1.0, 1.2))).map(|(f, _)| f);
    if let Some(f) = top {
        let _ = e.apply(Command::PushPull { face: f, distance: -0.6, keep_base: false });
    }
    let _ = e.apply(Command::DrawCircle {
        centre: DVec3::new(4.5, 1.0, 0.0),
        normal: DVec3::Z,
        radius: 0.7,
        segments: 24,
        start: None,
    });
    let disc = e.mesh().faces().find(|(f, _)| e.mesh().face_contains(*f, DVec3::new(4.5, 1.0, 0.0))).map(|(f, _)| f);
    if let Some(f) = disc {
        let _ = e.apply(Command::PushPull { face: f, distance: -0.5, keep_base: false });
    }
    // The cylinder becomes a component, copied twice along red.
    let cylinder: Vec<Element> = {
        let m = e.mesh();
        let near = |p: DVec3| p.distance(DVec3::new(4.5, 1.0, p.z)) < 0.75;
        let faces =
            m.faces().filter(|(_, x)| m.loop_points(&x.outer).iter().all(|p| near(*p))).map(|(f, _)| Element::Face(f));
        let edges = m.edges().filter(|(id, _)| {
            let (a, b) = m.edge_points(*id);
            near(a) && near(b)
        });
        faces.chain(edges.map(|(id, _)| Element::Edge(id))).collect()
    };
    if let Ok(out) = e.apply(Command::MakeComponent { elements: cylinder, name: Some("Drum".into()) }) {
        let _ = e.apply(Command::Copy { elements: out.created.clone(), offset: DVec3::new(1.8, 0.0, 0.0), copies: 2 });
        e.set_selection(out.created);
    }
    // Paint: brick on the box's front, see-through glass on the pocket's floor.
    let brick = e
        .apply(Command::AddMaterial { name: "Brick".into(), color: [0.66, 0.30, 0.22, 1.0], texture: None })
        .ok()
        .and_then(|o| o.material);
    let front = e.mesh().faces().find(|(f, _)| e.mesh().face_contains(*f, DVec3::new(1.5, 0.0, 0.6))).map(|(f, _)| f);
    if let Some(f) = front {
        let _ = e.apply(Command::Paint { faces: vec![f], material: brick, back: false });
    }
    let _ = e.apply(Command::AddGuideLine { origin: DVec3::new(0.0, -0.5, 0.0), dir: DVec3::X });
    app.camera.frame(DVec3::new(-0.5, -0.5, 0.0), DVec3::new(8.5, 2.5, 1.2));
    app.camera.distance *= 0.85;
    app.set_tool(ToolKind::Select);
}
