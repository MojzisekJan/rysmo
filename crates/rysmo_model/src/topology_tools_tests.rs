use crate::{Command, Editor, Element};
use rysmo_math::DVec3;

fn chain(nested: bool) -> Editor {
    let mut editor = Editor::new();
    let out = editor
        .apply(Command::DrawLines {
            lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::X, DVec3::X + DVec3::Y]],
            facing: None,
        })
        .unwrap();
    if nested {
        let out = editor.apply(Command::MakeGroup { elements: out.created }).unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        editor.enter(id).unwrap();
    }
    editor
}
#[test]
fn weld_explode_and_copied_curve_have_independent_undo_in_nested_contexts() {
    for nested in [false, true] {
        let mut editor = chain(nested);
        let edges: Vec<_> = editor.mesh().edges().map(|(id, _)| id).collect();
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        assert!(editor.apply(Command::WeldEdges { edges: edges.clone() }).unwrap().changed);
        assert_eq!(editor.undo_depth(), depth + 1);
        assert_eq!(editor.undo_label(), Some("Weld Edges"));
        let welded = editor.doc().clone();
        let curve = editor.mesh().curves().next().unwrap().0;
        let revision = editor.revision();
        assert!(!editor.apply(Command::WeldEdges { edges: edges.clone() }).unwrap().changed);
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.undo_depth(), depth + 1);
        let copied = editor
            .apply(Command::Copy {
                elements: edges.iter().copied().map(Element::Edge).collect(),
                offset: DVec3::Y * 4.,
                copies: 1,
            })
            .unwrap();
        assert_eq!(editor.mesh().curves().count(), 2);
        let copied_edge = copied
            .created
            .iter()
            .find_map(|element| if let Element::Edge(id) = element { Some(*id) } else { None })
            .unwrap();
        let copied = editor.doc().clone();
        let out = editor.apply(Command::ExplodeCurves { edges: vec![copied_edge] }).unwrap();
        assert_eq!(out.created.len(), 2);
        assert_eq!(editor.mesh().curves().count(), 1);
        assert!(editor.mesh().curve(curve).is_some());
        assert_eq!(editor.undo_label(), Some("Explode Curve"));
        assert!(editor.undo());
        assert_eq!(editor.doc(), &copied);
        assert!(editor.redo());
        assert_eq!(editor.mesh().curves().count(), 1);
        assert!(editor.undo());
        assert!(editor.undo());
        assert_eq!(editor.doc(), &welded);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
}
#[test]
fn invalid_weld_and_explosion_leave_revision_and_history_unchanged() {
    let mut editor = chain(false);
    let edges: Vec<_> = editor.mesh().edges().map(|(id, _)| id).collect();
    let before = editor.doc().clone();
    let revision = editor.revision();
    let depth = editor.undo_depth();
    for command in [
        Command::WeldEdges { edges: vec![edges[0]] },
        Command::ExplodeCurves { edges: edges.clone() },
        Command::WeldEdges { edges: vec![edges[0], crate::EdgeId(u64::MAX)] },
    ] {
        assert!(editor.apply(command).is_err());
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.undo_depth(), depth);
    }
}
#[test]
fn orient_repairs_a_closed_solid_preserves_reference_and_has_one_undo() {
    for nested in [false, true] {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::ZERO,
                u: DVec3::X * 4.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::PushPull { face, distance: 3., keep_base: false }).unwrap();
        if nested {
            let elements = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
            let out = editor.apply(Command::MakeGroup { elements }).unwrap();
            let Element::Instance(id) = out.created[0] else { panic!() };
            editor.enter(id).unwrap();
        }
        let faces: Vec<_> = editor.mesh().faces().map(|(id, _)| id).collect();
        let seed = faces[0];
        let reversed = faces[1];
        let placement =
            rysmo_mesh::TexturePlacement { rotation: 27., offset: [0.2, 0.3], scale: [1.3, 0.9], ..Default::default() };
        editor.apply(Command::SetTexturePlacement { faces: vec![reversed], placement: Some(placement) }).unwrap();
        editor.apply(Command::ReverseFaces { faces: vec![reversed] }).unwrap();
        let sample = editor.mesh().loop_points(&editor.mesh().face(reversed).unwrap().outer)[0];
        let uv = crate::texture::uv_at(sample, editor.mesh().face_normal(reversed).unwrap(), [2., 3.], &placement);
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        let reference = editor.mesh().face(seed).unwrap().clone();
        let out = editor.apply(Command::OrientFaces { seed }).unwrap();
        assert_eq!(out.created, vec![Element::Face(reversed)]);
        assert_eq!(editor.mesh().face(seed), Some(&reference));
        assert_eq!(editor.undo_depth(), depth + 1);
        assert_eq!(editor.undo_label(), Some("Orient Faces"));
        assert!((crate::solid_validation::validate(editor.mesh(), false).unwrap() - 24.).abs() < 1e-8);
        let after = editor.doc().clone();
        assert_eq!(
            crate::texture::uv_at(sample, editor.mesh().face_normal(reversed).unwrap(), [2., 3.], &placement),
            uv
        );
        let revision = editor.revision();
        assert!(!editor.apply(Command::OrientFaces { seed }).unwrap().changed);
        assert_eq!(editor.undo_depth(), depth + 1);
        assert_eq!(editor.revision(), revision);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
        assert!(editor.redo());
        assert_eq!(editor.doc(), &after);
    }
}
#[test]
fn exploding_an_analytic_circle_is_undone_with_its_original_parameters() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 3., segments: 24, start: None })
        .unwrap();
    let before = editor.doc().clone();
    let edge = editor.mesh().edges().next().unwrap().0;
    assert!(
        !editor.apply(Command::WeldEdges { edges: editor.mesh().edges().map(|(id, _)| id).collect() }).unwrap().changed
    );
    editor.apply(Command::ExplodeCurves { edges: vec![edge] }).unwrap();
    assert_eq!(editor.mesh().curves().count(), 0);
    assert_eq!(editor.mesh().edge_count(), 24);
    assert_eq!(editor.mesh().face_count(), 1);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}
