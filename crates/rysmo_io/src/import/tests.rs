use super::*;
use rysmo_math::DVec3;
use rysmo_model::{Command, Editor};

#[test]
fn obj_concave_negative_indices_and_source_axis_units() {
    let text = b"v 0 0 0\nv 2 0 0\nv 2 2 0\nv 1 1 0\nv 0 2 0\nf -5 -4 -3 -2 -1\n";
    let imported =
        read("floor.OBJ", text, Options { unit_scale: Some(0.5), obj_up: UpAxis::Z, ..Default::default() }).unwrap();
    // A flat concave polygon stays one editable face.
    let mesh = &imported.document.root.mesh;
    assert_eq!(mesh.face_count(), 1);
    assert_eq!(mesh.faces().next().unwrap().1.outer.len(), 5);
    let (lo, hi) = imported.document.definition_bounds(None).unwrap();
    assert_eq!(lo, DVec3::ZERO);
    assert_eq!(hi, DVec3::new(1., 1., 0.));
    assert!(read("bad.obj", b"v 0 0 0\nf 1 2 3", Options::default()).is_err());
    assert!(read("bad.obj", b"v NaN 0 0", Options::default()).is_err());
}
#[test]
fn dxf_header_units_layers_and_curved_polyline() {
    let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nWalls\n10\n0\n20\n0\n11\n1000\n21\n0\n0\nLWPOLYLINE\n8\nArc\n90\n2\n10\n0\n20\n0\n42\n1\n10\n1000\n20\n0\n0\nENDSEC\n0\nEOF\n";
    let imported = read("plan.dxf", dxf.as_bytes(), Options::default()).unwrap();
    let (lo, hi) = imported.document.definition_bounds(None).unwrap();
    assert!((hi.x - 1.).abs() < 1e-6);
    assert!((lo.y + 0.5).abs() < 1e-6);
    assert_eq!(imported.document.tags.len(), 2);
    assert_eq!(imported.document.root.instances.len(), 2);
    assert!(read("bad.dxf", b"0\nLINE\n10\nbad\n", Options::default()).is_err());
}
#[test]
fn glb_export_import_preserves_world_bounds_and_invalid_chunks_fail() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawRectangle {
            corner: DVec3::new(2., 3., 4.),
            u: DVec3::X * 5.,
            v: DVec3::Y * 2.,
            facing: Some(DVec3::Z),
        })
        .unwrap();
    let mut bytes = crate::gltf::export(editor.doc());
    let imported = read("panel.glb", &bytes, Options::default()).unwrap();
    assert_eq!(imported.document.definition_bounds(None), editor.doc().definition_bounds(None));
    bytes.truncate(bytes.len() - 1);
    assert!(read("broken.glb", &bytes, Options::default()).is_err());
}

#[test]
fn glb_material_surface_and_normal_texture_uv_survive_exchange() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: Some(DVec3::Z) })
        .unwrap();
    let face = editor.mesh().faces().next().unwrap().0;
    let material = editor
        .apply(Command::AddMaterial { name: "Metal".into(), color: [0.5, 0.5, 0.5, 1.], texture: None })
        .unwrap()
        .material
        .unwrap();
    editor
        .apply(Command::AddNormalMap {
            material,
            name: "Normal".into(),
            mime: "image/png".into(),
            data: include_bytes!("../../../rysmo_app/brand/icon-192.png").to_vec(),
        })
        .unwrap();
    let mut pbr = editor.doc().materials[&material].pbr;
    pbr.metallic = 0.75;
    pbr.roughness = 0.23;
    pbr.normal_scale = 0.4;
    editor.apply(Command::SetMaterialPbr { material, pbr }).unwrap();
    editor.apply(Command::Paint { faces: vec![face], material: Some(material), back: false }).unwrap();
    let imported = read("surface.glb", &crate::gltf::export(editor.doc()), Options::default()).unwrap();
    let material = imported.document.materials.values().next().unwrap();
    assert!((material.pbr.metallic - 0.75).abs() < 1e-6);
    assert!((material.pbr.roughness - 0.23).abs() < 1e-6);
    assert!((material.pbr.normal_scale - 0.4).abs() < 1e-6);
    assert!(material.pbr.normal_map.is_some());
    assert!(imported.document.definitions.values().any(|d| d.mesh.faces().any(|(_, f)| f.uv.is_some())));
}

#[test]
fn shared_hierarchy_expansion_is_bounded_before_preview_or_collection_installation() {
    use rysmo_math::DMat4;
    use rysmo_model::{Definition, DefinitionId, Instance, InstanceId};
    let mut doc = rysmo_model::Document::new();
    for level in 1..30 {
        let mut definition = Definition::default();
        if level > 1 {
            for child in 0..2 {
                definition.instances.insert(
                    InstanceId(1000 + level * 2 + child),
                    Instance {
                        definition: DefinitionId(level - 1),
                        transform: DMat4::IDENTITY,
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                        glued_to: None,
                    },
                );
            }
        }
        doc.definitions.insert(DefinitionId(level), definition);
    }
    doc.root.instances.insert(
        InstanceId(999),
        Instance {
            definition: DefinitionId(29),
            transform: DMat4::IDENTITY,
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: None,
        },
    );
    assert!(validate_expansion(&doc).is_err());
}

#[test]
fn shared_normal_images_are_exported_once_for_different_materials() {
    let mut editor = Editor::new();
    for x in [0., 2.] {
        editor
            .apply(Command::DrawRectangle { corner: DVec3::X * x, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap();
    }
    let faces: Vec<_> = editor.mesh().faces().map(|(id, _)| id).collect();
    let material = editor
        .apply(Command::AddMaterial { name: "First".into(), color: [0.5; 4], texture: None })
        .unwrap()
        .material
        .unwrap();
    editor
        .apply(Command::AddNormalMap {
            material,
            name: "Normal".into(),
            mime: "image/png".into(),
            data: include_bytes!("../../../rysmo_app/brand/icon-192.png").to_vec(),
        })
        .unwrap();
    let mut doc = editor.doc().clone();
    let other = rysmo_model::MaterialId(doc.reserve_id());
    let mut copy = doc.materials[&material].clone();
    copy.name = "Second".into();
    copy.pbr.roughness = 0.2;
    doc.materials.insert(other, copy);
    editor = Editor::with_document(doc);
    for (face, material) in faces.into_iter().zip([material, other]) {
        editor.apply(Command::Paint { faces: vec![face], material: Some(material), back: false }).unwrap();
    }
    let bytes = crate::gltf::export(editor.doc());
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + n]).unwrap();
    assert_eq!(json["images"].as_array().unwrap().len(), 1);
    assert_eq!(json["materials"][0]["normalTexture"]["index"], json["materials"][1]["normalTexture"]["index"]);
}

#[test]
fn dxf_block_basepoint_scale_rotation_and_layer_inheritance_are_applied() {
    let text = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n10\n1\n20\n2\n0\nLINE\n8\n0\n10\n1\n20\n2\n11\n3\n21\n2\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n8\nWalls\n10\n10\n20\n20\n41\n2\n42\n2\n50\n90\n0\nENDSEC\n0\nEOF\n";
    let imported = read("block.dxf", text.as_bytes(), Options::default()).unwrap();
    let (lo, hi) = imported.document.definition_bounds(None).unwrap();
    assert!(lo.distance(DVec3::new(10., 20., 0.)) < 1e-8);
    assert!(hi.distance(DVec3::new(10., 24., 0.)) < 1e-8);
    assert!(imported.document.tags.values().any(|tag| tag.name == "Walls"));
}

#[test]
fn obj_planar_quads_stay_single_faces_unless_bent_or_unevenly_mapped() {
    let cube = b"v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
f 1/1 4/4 3/3 2/2\nf 5/1 6/2 7/3 8/4\nf 1/1 2/2 6/3 5/4\nf 2/1 3/2 7/3 6/4\nf 3/1 4/2 8/3 7/4\nf 4/1 1/2 5/3 8/4\n";
    let options = Options { obj_up: UpAxis::Z, ..Default::default() };
    let mesh = read("cube.obj", cube, options).unwrap().document.root.mesh;
    assert_eq!(mesh.face_count(), 6);
    assert!(mesh.faces().all(|(_, face)| face.outer.len() == 4 && face.uv.is_some()));
    // A bent quad and a quad whose UVs are not one affine map keep the triangulated fallback.
    let bent = b"v 0 0 0\nv 1 0 0\nv 1 1 0.3\nv 0 1 0\nf 1 2 3 4\n";
    assert_eq!(read("bent.obj", bent, options).unwrap().document.root.mesh.face_count(), 2);
    let skewed = b"v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 3 2\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
    assert_eq!(read("skewed.obj", skewed, options).unwrap().document.root.mesh.face_count(), 2);
}
