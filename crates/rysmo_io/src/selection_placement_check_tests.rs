use rysmo_math::{DMat4, DVec3};
use rysmo_model::{
    Command, Editor, Element,
    solid_check::{self, Repair},
};

#[test]
fn surface_aligned_component_survives_save_open_and_one_undo() {
    let mut editor = Editor::new();
    let out = editor
        .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: Some(DVec3::Z) })
        .unwrap();
    let component =
        editor.apply(Command::MakeComponent { elements: out.created, name: Some("Panel".into()) }).unwrap().created[0];
    let clipboard = rysmo_model::clipboard::Clipboard::capture(&editor, vec![component]).unwrap();
    let anchor = clipboard.document.root.instances.values().next().unwrap().transform;
    let placement = rysmo_model::surface_placement::SurfacePlacement {
        point: DVec3::new(3., 4., 5.),
        normal: DVec3::Y,
        angle_degrees: 45.,
        offset: 0.25,
        flip: false,
    };
    let transform = placement.transform(clipboard.source_transform, DMat4::IDENTITY, anchor).unwrap();
    let before = editor.doc().clone();
    let depth = editor.undo_depth();
    let out =
        editor.apply(Command::Paste { clipboard: Box::new(clipboard), transform, reuse_definitions: true }).unwrap();
    let Element::Instance(id) = out.created[0] else { panic!() };
    let pasted = &editor.active().instances[&id];
    assert!(pasted.transform.w_axis.truncate().distance(placement.point + DVec3::Y * 0.25) < 1e-9);
    assert!(pasted.transform.inverse().transpose().transform_vector3(DVec3::Z).normalize().dot(DVec3::Y) > 0.999);
    assert_eq!(editor.undo_depth(), depth + 1);
    let source = crate::save(editor.doc());
    let loaded = crate::load(&source).unwrap();
    assert_eq!(crate::save(&loaded), source);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}
#[test]
fn repaired_closed_shell_and_removed_loose_geometry_persist() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawRectangle {
            corner: DVec3::ZERO,
            u: DVec3::X * 2.,
            v: DVec3::Y * 2.,
            facing: Some(DVec3::Z),
        })
        .unwrap();
    let face = editor.mesh().faces().next().unwrap().0;
    editor.apply(Command::PushPull { face, distance: 2., keep_base: false }).unwrap();
    editor.apply(Command::ReverseFaces { faces: vec![face] }).unwrap();
    editor
        .apply(Command::DrawLines { lines: vec![[DVec3::splat(5.), DVec3::splat(5.) + DVec3::X]], facing: None })
        .unwrap();
    editor.apply(Command::RepairSolid { repair: Repair::RemoveLooseEdges }).unwrap();
    editor.apply(Command::RepairSolid { repair: Repair::OrientClosedShells }).unwrap();
    let source = crate::save(editor.doc());
    let loaded = crate::load(&source).unwrap();
    assert_eq!(crate::save(&loaded), source);
    let report = solid_check::inspect(&loaded.root.mesh).unwrap();
    assert!(report.reason.is_none());
    assert!((report.volume.unwrap() - 8.).abs() < 1e-8);
}
