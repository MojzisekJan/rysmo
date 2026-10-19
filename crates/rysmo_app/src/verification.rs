//! Opt-in native visual verification scene, activated only by an environment flag.
use crate::state::App3d;
use rysmo_math::DVec3;
use rysmo_model::{Command, Editor, Element};

pub fn basics(app: &mut App3d, mode: &str) {
    *app = App3d::new(Editor::new());
    if mode == "fillet" {
        for x in [0., 4.] {
            let corner = DVec3::X * x;
            app.editor
                .apply(Command::DrawRectangle { corner, u: DVec3::X * 3., v: DVec3::Y * 3., facing: None })
                .unwrap();
            let vertex = app.editor.mesh().find_vertex(corner).unwrap();
            let edges = app.editor.mesh().edges_of_vertex(vertex);
            if x == 0. {
                app.editor
                    .apply(Command::FilletEdges { edges: [edges[0], edges[1]], radius: 0.5, segments: 12 })
                    .unwrap();
            } else {
                app.editor.set_selection(edges.into_iter().map(Element::Edge));
            }
        }
        app.camera.target = DVec3::new(3.5, 1.5, 0.);
        app.camera.distance = 13.;
        app.camera.pitch = 0.8;
        crate::fillet::preview(app);
        return;
    }
    if ["rotation_handles", "studio", "render_half", "render_super"].contains(&mode) {
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 3., v: DVec3::Y * 2., facing: None })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.apply(Command::PushPull { face, distance: -2., keep_base: false }).unwrap();
        let elements = app.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        let group = app.editor.apply(Command::MakeGroup { elements }).unwrap().created;
        app.editor.set_selection(group);
        app.camera.target = DVec3::new(1.5, 1., 1.);
        app.camera.distance = 10.;
        app.camera.pitch = 0.6;
        app.set_tool(rysmo_tools::ToolKind::Rotate);
        if mode != "rotation_handles" {
            app.set_tool(rysmo_tools::ToolKind::Select);
            app.editor.clear_selection();
            let style = rysmo_model::Style {
                lighting: rysmo_model::Lighting::Studio,
                studio_environment: rysmo_model::StudioEnvironment::Warm,
                tone_mapping: rysmo_model::ToneMapping::AgX,
                studio_rotation: 35.,
                render_scale: if mode == "render_half" {
                    0.5
                } else if mode == "render_super" {
                    1.5
                } else {
                    1.
                },
                ..Default::default()
            };
            app.editor.apply(Command::SetStyle { style }).unwrap();
            app.show_settings = mode == "studio";
        }
        return;
    }
    if mode == "solids" || mode == "solid_split" {
        fn box_group(editor: &mut Editor, corner: DVec3) -> rysmo_model::InstanceId {
            editor
                .apply(Command::DrawRectangle { corner, u: DVec3::X * 2., v: DVec3::Y * 2., facing: Some(DVec3::Z) })
                .unwrap();
            let face = editor.mesh().faces().next().unwrap().0;
            editor
                .apply(Command::PushPull {
                    face,
                    distance: 2. * editor.mesh().face_normal(face).unwrap().z,
                    keep_base: false,
                })
                .unwrap();
            let elements = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
            let Element::Instance(id) = editor.apply(Command::MakeGroup { elements }).unwrap().created[0] else {
                unreachable!()
            };
            id
        }
        let mut original = Vec::new();
        if mode == "solid_split" {
            let a = box_group(&mut app.editor, DVec3::ZERO);
            let b = box_group(&mut app.editor, DVec3::ONE);
            original = vec![Element::Instance(a), Element::Instance(b)];
            let a = box_group(&mut app.editor, DVec3::X * 5.);
            let b = box_group(&mut app.editor, DVec3::X * 5. + DVec3::ONE);
            let out = app
                .editor
                .apply(Command::SolidBoolean {
                    left: a,
                    right: b,
                    operation: rysmo_model::solids::BooleanOp::Split,
                    keep_inputs: false,
                })
                .unwrap();
            for (i, element) in out.created.into_iter().enumerate() {
                app.editor
                    .apply(Command::Move { elements: vec![element], offset: DVec3::X * (i as f64 * 4.) })
                    .unwrap();
            }
        } else {
            for (i, op) in [
                None,
                Some(rysmo_model::solids::BooleanOp::Union),
                Some(rysmo_model::solids::BooleanOp::Subtract),
                Some(rysmo_model::solids::BooleanOp::Intersect),
            ]
            .into_iter()
            .enumerate()
            {
                let origin = DVec3::X * (i as f64 * 5.);
                let a = box_group(&mut app.editor, origin);
                let b = box_group(&mut app.editor, origin + DVec3::ONE);
                if let Some(operation) = op {
                    app.editor
                        .apply(Command::SolidBoolean { left: a, right: b, operation, keep_inputs: false })
                        .unwrap();
                } else {
                    original = vec![Element::Instance(a), Element::Instance(b)];
                }
            }
        }
        app.editor.set_selection(original);
        app.camera.target = DVec3::new(8., 1., 0.);
        app.camera.distance = 33.;
        app.camera.pitch = 0.65;
        crate::solid_tools::begin(app);
        if mode == "solid_split" {
            crate::solid_tools::preview_split(app);
        }
        return;
    }
    if mode == "mirror" || mode == "mirror_face" {
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: None })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.apply(Command::PushPull { face, distance: 1.5, keep_base: false }).unwrap();
        let elements = app.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect::<Vec<_>>();
        if mode == "mirror_face" {
            app.editor
                .apply(Command::DrawRectangle {
                    corner: DVec3::new(5., -1., 0.),
                    u: DVec3::Y * 3.,
                    v: DVec3::new(-1.5, 0., 2.),
                    facing: None,
                })
                .unwrap();
        }
        app.editor.set_selection(elements);
        app.camera.target = DVec3::new(2., 0.5, 0.5);
        app.camera.distance = 12.;
        app.camera.pitch = 0.6;
        crate::mirror::begin(app);
        if mode == "mirror_face" {
            crate::mirror::preview_face(app);
            app.camera.target = DVec3::new(3.5, 0.5, 2.);
            app.camera.distance = 17.;
        } else {
            crate::mirror::preview_offset(app);
        }
        return;
    }
    if mode == "three_point" {
        let start = DVec3::new(-3., 0., 0.);
        let end = DVec3::new(-2., -1., 0.);
        app.editor.apply(Command::DrawLines { lines: vec![[start, end]], facing: None }).unwrap();
        app.editor
            .apply(Command::DrawThreePointArc {
                start,
                through: DVec3::new(-2., 1., 0.),
                end,
                normal: DVec3::Z,
                segments: 18,
            })
            .unwrap();
        app.camera.target = DVec3::new(-0.1, 0.2, 0.);
        app.camera.distance = 9.;
        app.camera.pitch = 0.7;
        app.set_tool(rysmo_tools::ToolKind::ThreePointArc);
        return;
    }

    if mode == "arcs" {
        for (centre, radius, sweep, pie) in
            [(DVec3::new(-2.5, 0., 0.), 1.8, 135., true), (DVec3::new(1.8, 0., 0.), 1.2, 270., false)]
        {
            app.editor
                .apply(Command::DrawCentreArc {
                    centre,
                    normal: DVec3::Z,
                    start: DVec3::X,
                    radius,
                    sweep,
                    segments: 24,
                    pie,
                })
                .unwrap();
        }
        app.camera.target = DVec3::new(0., 1.1, 0.);
        app.camera.distance = 10.;
        app.camera.pitch = 0.8;
        app.set_tool(rysmo_tools::ToolKind::CenterArc);
        return;
    }

    if mode == "rotated" {
        app.editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(-2.5, -1., 0.),
                u: DVec3::new(2., 0.6, 0.),
                v: DVec3::new(-0.3, 1., 0.8),
                facing: None,
            })
            .unwrap();
        app.camera.target = DVec3::new(0.8, 0.6, 0.6);
        app.camera.distance = 9.;
        app.set_tool(rysmo_tools::ToolKind::RotatedRectangle);
        return;
    }

    if mode == "volume" {
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y * 3., facing: None })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.apply(Command::PushPull { face, distance: -4., keep_base: false }).unwrap();
        let elements = app.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        let group = app.editor.apply(Command::MakeGroup { elements }).unwrap();
        app.editor.set_selection(group.created);
        app.show_entity_info = true;
        app.camera.target = DVec3::new(1., 1.5, 2.);
        app.camera.distance = 10.;
        return;
    }

    if mode == "construction" || mode == "fractional" {
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 3., v: DVec3::Y * 2., facing: None })
            .unwrap();
        app.editor.apply(Command::AddGuideLine { origin: DVec3::Y * 0.5, dir: DVec3::X }).unwrap();
        let point = app
            .editor
            .apply(Command::AddGuideSegment { start: DVec3::new(0., 2., 0.), point: DVec3::new(1.5, 2.8, 0.) })
            .unwrap();
        app.editor
            .apply(Command::DrawArc {
                start: DVec3::new(4., 0., 0.),
                end: DVec3::new(5., 0., 0.),
                normal: DVec3::Z,
                bulge: 0.5,
                segments: 24,
            })
            .unwrap();
        app.editor.set_selection(point.created);
        app.camera.target = DVec3::new(2.5, 1., 0.);
        app.camera.distance = 8.;
        app.camera.pitch = 0.8;
        if mode == "fractional" {
            app.editor.apply(Command::SetUnits { units: rysmo_model::Units::Architectural }).unwrap();
            app.editor.apply(Command::SetLengthPrecision { precision: Some(6) }).unwrap();
            app.show_settings = true;
            app.settings_tab = 1;
        }
        return;
    }
    let geometry = app
        .editor
        .apply(Command::DrawRectangle {
            corner: DVec3::ZERO,
            u: DVec3::X * 1.4,
            v: DVec3::Y * 1.8,
            facing: Some(DVec3::Z),
        })
        .unwrap()
        .created;
    let made =
        app.editor.apply(Command::MakeComponent { elements: geometry, name: Some("Window opening".into()) }).unwrap();
    let Element::Instance(original) = made.created[0] else { return };
    let definition = app.editor.active().instances[&original].definition;
    app.editor.enter(original).unwrap();
    let face = app.editor.mesh().faces().next().unwrap().0;
    app.editor.apply(Command::Erase { elements: vec![Element::Face(face)] }).unwrap();
    app.editor.exit();
    app.editor.apply(Command::Erase { elements: vec![Element::Instance(original)] }).unwrap();
    let mut settings = app.editor.doc().definitions[&definition].component.clone();
    settings.glue = rysmo_model::components::GlueMode::Any;
    settings.cut_opening = true;
    settings.description = "A planar outline glued to a wall, cutting one face.".into();
    app.editor
        .apply(Command::SetComponentProperties {
            definition,
            name: "Window opening".into(),
            settings: settings.clone(),
        })
        .unwrap();
    app.editor
        .apply(Command::DrawRectangle {
            corner: DVec3::ZERO,
            u: DVec3::X * 6.,
            v: DVec3::Z * 4.,
            facing: Some(-DVec3::Y),
        })
        .unwrap();
    let wall = app.editor.mesh().faces().next().unwrap().0;
    app.editor
        .apply(Command::PlaceGluedInstance {
            definition,
            face: wall,
            origin: DVec3::new(4., 0., 1.2),
            rotation: 0.,
            scale: 1.,
        })
        .unwrap();
    app.editor
        .apply(Command::DrawRectangle {
            corner: DVec3::new(7., 0., 0.),
            u: DVec3::X * 2.,
            v: DVec3::Z * 3.,
            facing: Some(-DVec3::Y),
        })
        .unwrap();
    let hidden = app
        .editor
        .mesh()
        .faces()
        .find(|(id, _)| app.editor.mesh().face_contains(*id, DVec3::new(8., 0., 1.)))
        .unwrap()
        .0;
    let mut elements: Vec<_> = app.editor.mesh().face_edges(hidden).into_iter().map(Element::Edge).collect();
    elements.push(Element::Face(hidden));
    app.editor.apply(Command::SetGeometryHidden { elements, hidden: true }).unwrap();
    let mut style = app.editor.doc().style;
    style.show_hidden_geometry = mode == "visibility";
    app.editor.apply(Command::SetStyle { style }).unwrap();
    app.show_entity_info = false;
    app.editor.clear_selection();
    app.camera.target = DVec3::new(4., 0., 2.);
    app.camera.distance = 12.;
    app.camera.yaw = -1.25;
    app.camera.pitch = 0.25;
    if mode == "units" {
        app.editor
            .apply(Command::SetMeasurementSettings {
                settings: rysmo_model::MeasurementSettings {
                    show_unit_markers: true,
                    force_zero_inches: false,
                    area_units: None,
                    area_precision: 3,
                    volume_units: None,
                    volume_precision: 3,
                    length_snap: Some(0.1),
                    angle_precision: 2,
                    angle_snap: Some(22.5),
                },
            })
            .unwrap();
        app.show_settings = true;
        app.settings_tab = 1;
    } else if mode == "components" {
        app.show_components = true;
        app.component_selected = Some(definition);
        app.component_edit = Some((definition, "Window opening".into(), settings));
    }
}

pub fn rotated_draft(app: &mut App3d) {
    use rysmo_tools::Pointer;
    let start = DVec3::new(0.5, 0., 0.);
    for point in [start, start + DVec3::new(2., 0.6, 0.)] {
        if let Some(at) = app.local_view().project(point) {
            crate::input::tool_pointer(app, at, Pointer::Move);
            crate::input::tool_pointer(app, at, Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(app, at, Pointer::Up);
        }
    }
    if let Some(at) = app.local_view().project(start + DVec3::new(-0.6, 2., 1.3)) {
        crate::input::tool_pointer(app, at, Pointer::Move);
    }
}

pub fn centre_draft(app: &mut App3d) {
    use rysmo_tools::Pointer;
    let centre = DVec3::new(0., 3., 0.);
    for point in [centre, centre + DVec3::X] {
        if let Some(at) = app.local_view().project(point) {
            crate::input::tool_pointer(app, at, Pointer::Move);
            crate::input::tool_pointer(app, at, Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(app, at, Pointer::Up);
        }
    }
    if let Some(at) = app.local_view().project(centre + DVec3::Y) {
        crate::input::tool_pointer(app, at, Pointer::Move);
    }
}

pub fn three_draft(app: &mut App3d) {
    use rysmo_tools::Pointer;
    for point in [DVec3::ZERO, DVec3::new(1., 1.2, 0.)] {
        if let Some(at) = app.local_view().project(point) {
            crate::input::tool_pointer(app, at, Pointer::Move);
            crate::input::tool_pointer(app, at, Pointer::Down { clicks: 1 });
            crate::input::tool_pointer(app, at, Pointer::Up);
        }
    }
    if let Some(at) = app.local_view().project(DVec3::new(2.8, 0.2, 0.)) {
        crate::input::tool_pointer(app, at, Pointer::Move);
    }
}
