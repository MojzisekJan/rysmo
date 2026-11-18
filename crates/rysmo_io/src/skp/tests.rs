use super::*;
use rysmo_math::{DMat4, DQuat, DVec3};
use rysmo_model::{Command, Editor};
const SHARED: &[u8] = include_bytes!("../../tests/fixtures/skp/shared-2017.skp");
const HOLE: &[u8] = include_bytes!("../../tests/fixtures/skp/hole-2026.skp");
const TEXTURE: &[u8] = include_bytes!("../../tests/fixtures/skp/texture-2026.skp");

#[test]
fn genuine_2017_components_remain_shared_and_editable() {
    let imported = crate::import::read("shared.skp", SHARED, Options::default()).unwrap();
    let doc = imported.document;
    assert_eq!(doc.root.instances.len(), 2);
    let instances: Vec<_> = doc.root.instances.values().collect();
    assert_eq!(instances[0].definition, instances[1].definition);
    assert_ne!(instances[0].transform, instances[1].transform);
    assert_eq!(doc.definitions[&instances[0].definition].mesh.face_count(), 6);
    assert!(doc.camera.is_some());
    for version in [Version::V2017, Version::V2026] {
        let exported = export(&doc, version).unwrap();
        let again = read(&exported.bytes, Options::default()).unwrap().document;
        assert_eq!(again.root.instances.len(), 2);
        let values: Vec<_> = again.root.instances.values().collect();
        assert_eq!(values[0].definition, values[1].definition);
        for (a, b) in instances.iter().zip(values) {
            assert!((a.transform - b.transform).abs_diff_eq(DMat4::ZERO, 1e-9));
            assert_eq!(a.name, b.name);
        }
    }
}

#[test]
fn genuine_archive_holes_stay_open_in_both_exports() {
    let doc = read(HOLE, Options::default()).unwrap().document;
    let area = |d: &Document| d.root.mesh.faces().map(|(id, _)| d.root.mesh.face_area(id)).sum::<f64>();
    assert!(doc.root.mesh.faces().any(|(_, face)| !face.holes.is_empty()));
    let expected = area(&doc);
    assert!(expected > 0.);
    for version in [Version::V2017, Version::V2026] {
        let exported = export(&doc, version).unwrap();
        let again = read(&exported.bytes, Options::default()).unwrap().document;
        assert!(again.root.mesh.faces().any(|(_, face)| !face.holes.is_empty()));
        assert!((area(&again) - expected).abs() < 1e-8);
        let triangles: f64 = again
            .root
            .mesh
            .faces()
            .flat_map(|(id, _)| rysmo_mesh::triangulate_face(&again.root.mesh, id))
            .map(|t| {
                let p = t.map(|v| again.root.mesh.pos(v));
                (p[1] - p[0]).cross(p[2] - p[0]).length() / 2.
            })
            .sum();
        assert!((triangles - expected).abs() < 1e-8);
    }
}

#[test]
fn genuine_projective_texture_matches_source_and_survives_export() {
    let source = skpwright::Model::parse(TEXTURE).unwrap();
    let run = source.geometry.iter().find(|run| run.def_index.is_none()).unwrap();
    let doc = read(TEXTURE, Options::default()).unwrap().document;
    let mut checked = 0;
    for ((_, face), original) in doc.root.mesh.faces().zip(&run.mesh.faces) {
        let front = original.front_material.and_then(|slot| source.applied_size_of(slot));
        let back = original.back_material.and_then(|slot| source.applied_size_of(slot));
        let Some((side, size)) =
            front.map(|size| (skpwright::Side::Front, size)).or_else(|| back.map(|size| (skpwright::Side::Back, size)))
        else {
            continue;
        };
        let Some(texture) = face
            .material
            .and_then(|id| doc.materials[&id].texture)
            .or_else(|| face.back_material.and_then(|id| doc.materials[&id].texture))
        else {
            continue;
        };
        let uv = original.uv_xform(side, size).unwrap();
        let normal = DVec3::from_array(original.normal);
        let mut rebase = None;
        for &vertex in &face.outer {
            let p = doc.root.mesh.pos(vertex);
            let expected = uv.apply(p.to_array());
            let actual = rysmo_model::uv_at(p, normal, texture.size, &face.uv.unwrap());
            let shift = rysmo_math::DVec2::new(actual[0] - expected[0], actual[1] - (1. - expected[1]));
            assert!(shift.distance(shift.round()) < 1e-7, "Non-integer UV change: {shift:?}");
            if let Some(previous) = rebase {
                assert!(shift.distance(previous) < 1e-7);
            } else {
                rebase = Some(shift);
            }
            checked += 1;
        }
    }
    assert!(
        checked >= 4,
        "No mapped quad checked; source root sides: {:?}",
        run.mesh.faces.iter().map(|face| (face.front_material, face.back_material)).collect::<Vec<_>>()
    );
    for version in [Version::V2017, Version::V2026] {
        let exported = export(&doc, version).unwrap();
        let again = read(&exported.bytes, Options::default()).unwrap().document;
        assert_eq!(doc.images.values().next().unwrap().bytes, again.images.values().next().unwrap().bytes);
        for ((id, a), (_, b)) in doc.root.mesh.faces().zip(again.root.mesh.faces()) {
            let n = doc.root.mesh.face_normal(id).unwrap();
            let Some(texture) = a
                .material
                .and_then(|id| doc.materials[&id].texture)
                .or_else(|| a.back_material.and_then(|id| doc.materials[&id].texture))
            else {
                continue;
            };
            let other = b
                .material
                .and_then(|id| again.materials[&id].texture)
                .or_else(|| b.back_material.and_then(|id| again.materials[&id].texture))
                .unwrap();
            for (&v, &w) in a.outer.iter().zip(&b.outer) {
                let x = rysmo_model::uv_at(doc.root.mesh.pos(v), n, texture.size, &a.uv.unwrap_or_default());
                let y = rysmo_model::uv_at(again.root.mesh.pos(w), n, other.size, &b.uv.unwrap_or_default());
                assert!(x.into_iter().zip(y).all(|(a, b)| (a - b).abs() < 1e-7));
            }
        }
    }
}

#[test]
fn transforms_keep_rotation_nonuniform_scale_mirroring_and_metres() {
    let transform = DMat4::from_scale_rotation_translation(
        DVec3::new(-2., 3., 4.),
        DQuat::from_rotation_z(0.73),
        DVec3::new(5., -7., 11.),
    );
    let stored = mapping::stored_transform(transform).unwrap();
    assert!(mapping::transform(stored, 1.).unwrap().abs_diff_eq(transform, 1e-12));
    let scaled = mapping::transform(stored, 0.1).unwrap();
    assert!(scaled.w_axis.truncate().distance(transform.w_axis.truncate() * 0.1) < 1e-12);
    assert_eq!(scaled.x_axis, transform.x_axis);
    assert!(mapping::stored_transform(DMat4::from_scale(DVec3::ZERO)).is_none());
    let mut invalid = stored;
    invalid[12] = f64::NAN;
    assert!(mapping::transform(invalid, 1.).is_none());
}

#[test]
fn empty_and_hidden_projects_remain_importable() {
    let mut editor = Editor::new();
    for version in [Version::V2017, Version::V2026] {
        let out = export(editor.doc(), version).unwrap();
        assert!(crate::import::read("empty.skp", &out.bytes, Options::default()).is_ok());
    }
    editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
    let mut doc = editor.doc().clone();
    let faces: Vec<_> = doc.root.mesh.faces().map(|(id, _)| id).collect();
    for id in faces {
        doc.root_mut().mesh.face_mut(id).unwrap().hidden = true;
    }
    let exported = export(&doc, Version::V2026).unwrap();
    let again = crate::import::read("hidden.skp", &exported.bytes, Options::default()).unwrap();
    assert!(again.document.root.mesh.faces().all(|(_, face)| face.hidden));
}

#[test]
fn rejects_bad_containers_and_excessive_declared_archive_expansion() {
    for length in [0, 1, 4, 20, 100] {
        assert!(read(&HOLE[..length], Options::default()).is_err());
    }
    let mut bytes = HOLE.to_vec();
    // The file also contains nested style archives. Mutate the outer directory.
    let end = bytes.windows(4).rposition(|p| p == b"PK\x05\x06").unwrap();
    let size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap()) as usize;
    let directory = end - size;
    assert_eq!(&bytes[directory..directory + 4], b"PK\x01\x02");
    bytes[directory + 24..directory + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    let error = read(&bytes, Options::default()).err().unwrap().to_string();
    assert!(error.contains("expansion"), "{error}");
    let options = Options { unit_scale: Some(f64::NAN), ..Default::default() };
    assert!(read(SHARED, options).is_err());
}

#[test]
fn circles_and_scene_property_flags_survive_both_containers() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: DVec3::new(0.3, 0.7, 0.2),
            normal: DVec3::Z,
            radius: 1.7,
            segments: 24,
            start: Some(DVec3::X),
        })
        .unwrap();
    let mut doc = editor.doc().clone();
    let mut scene = rysmo_model::scenes::Scene::capture(&doc, Default::default(), "Appearance only".into());
    scene.id = doc.reserve_id();
    scene.properties.camera = false;
    scene.style.faces = rysmo_model::FaceStyle::Wireframe;
    doc.scenes.push(scene);
    doc.measurement.area_units = Some(rysmo_model::Units::Centimetres);
    doc.measurement.volume_units = Some(rysmo_model::Units::Millimetres);
    for version in [Version::V2017, Version::V2026] {
        let out = export(&doc, version).unwrap();
        let again = read(&out.bytes, Options::default()).unwrap().document;
        let (id, curve) = again.root.mesh.curves().next().unwrap();
        assert_eq!(curve.kind, rysmo_mesh::CurveKind::Circle);
        assert_eq!(curve.segments, 24);
        assert!((curve.radius - 1.7).abs() < 1e-9);
        assert_eq!(again.root.mesh.curve_edges(id).len(), 24);
        assert_eq!(again.scenes.len(), 1);
        assert!(!again.scenes[0].properties.camera);
        assert!(again.scenes[0].properties.style);
        assert!(!again.scenes[0].properties.hidden_geometry);
        assert_eq!(again.scenes[0].style.faces, rysmo_model::FaceStyle::Wireframe);
        if version == Version::V2026 {
            assert_eq!(again.measurement.area_units, doc.measurement.area_units);
            assert_eq!(again.measurement.volume_units, doc.measurement.volume_units);
        }
    }
}

#[test]
fn repeated_conversion_does_not_create_untagged_tags() {
    let mut doc = Document::new();
    let id = TagId(doc.reserve_id());
    doc.tags.insert(id, Tag { name: "Walls".into(), visible: false });
    for version in [Version::V2017, Version::V2026, Version::V2017, Version::V2026] {
        let out = export(&doc, version).unwrap();
        doc = read(&out.bytes, Options::default()).unwrap().document;
        assert_eq!(doc.tags.len(), 1);
        let tag = doc.tags.values().next().unwrap();
        assert_eq!(tag.name, "Walls");
        assert!(!tag.visible);
    }
}

#[test]
fn dynamic_openings_are_baked_without_mutating_the_editable_host() {
    use rysmo_mesh::Element;
    use rysmo_model::components::{ComponentSettings, GlueMode};
    let mut editor = Editor::new();
    let outline = editor
        .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
        .unwrap()
        .created;
    let made = editor.apply(Command::MakeComponent { elements: outline, name: Some("Window".into()) }).unwrap();
    let Element::Instance(original) = made.created[0] else { panic!() };
    let definition = editor.doc().root.instances[&original].definition;
    editor.enter(original).unwrap();
    let face = editor.mesh().faces().next().unwrap().0;
    editor.apply(Command::Erase { elements: vec![Element::Face(face)] }).unwrap();
    editor.exit();
    editor.apply(Command::Erase { elements: vec![Element::Instance(original)] }).unwrap();
    editor
        .apply(Command::SetComponentProperties {
            definition,
            name: "Window".into(),
            settings: ComponentSettings { glue: GlueMode::Any, cut_opening: true, ..Default::default() },
        })
        .unwrap();
    editor
        .apply(Command::DrawRectangle {
            corner: DVec3::new(2., 2., 0.),
            u: DVec3::X * 10.,
            v: DVec3::Y * 10.,
            facing: Some(DVec3::Z),
        })
        .unwrap();
    let host = editor.mesh().faces().next().unwrap().0;
    editor
        .apply(Command::PlaceGluedInstance {
            definition,
            face: host,
            origin: DVec3::new(5., 5., 0.),
            rotation: 0.,
            scale: 1.,
        })
        .unwrap();
    let before = editor.doc().clone();
    assert!((before.root.mesh.face_area(host) - 100.).abs() < 1e-9);
    assert!((before.effective_mesh(None).face_area(host) - 99.).abs() < 1e-9);
    for version in [Version::V2017, Version::V2026] {
        let out = export(editor.doc(), version).unwrap();
        assert!(out.warnings.iter().any(|warning| warning.contains("baked")));
        let after = read(&out.bytes, Options::default()).unwrap().document;
        let area: f64 = after.root.mesh.faces().map(|(id, _)| after.root.mesh.face_area(id)).sum();
        assert!((area - 99.).abs() < 1e-8, "{version:?}: {area}");
        assert!(after.root.mesh.faces().any(|(_, face)| !face.holes.is_empty()));
        assert_eq!(editor.doc(), &before);
    }
}

#[test]
fn material_variants_share_embedded_image_storage_and_keep_their_tints() {
    let mut doc = read(TEXTURE, Options::default()).unwrap().document;
    let mut tint = doc.materials.values().find(|m| m.texture.is_some()).unwrap().clone();
    tint.name = "Tint variant".into();
    tint.color = [0.2, 0.4, 0.6, 1.];
    tint.pbr.tint_texture = true;
    let id = MaterialId(doc.reserve_id());
    doc.materials.insert(id, tint);
    let face = doc.root.mesh.faces().next().unwrap().0;
    doc.root_mut().mesh.face_mut(face).unwrap().material = Some(id);
    for version in [Version::V2017, Version::V2026] {
        let out = export(&doc, version).unwrap();
        assert!(!out.warnings.iter().any(|warning| warning.contains("Advanced material")));
        let after = read(&out.bytes, Options::default()).unwrap().document;
        assert_eq!(after.images.len(), 1);
        let textures: Vec<_> = after.materials.values().filter_map(|material| material.texture).collect();
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].image, textures[1].image);
        assert!(after.materials.values().find(|m| m.name == "Tint variant").unwrap().pbr.tint_texture);
    }
}

fn encode_source(model: &skpwright::Model) -> Vec<u8> {
    model.to_archive_with(skpwright::ArchiveTarget::ByContent, skpwright::WriteEnv { unix_time: 0, seed: 7 }).unwrap().0
}

#[test]
fn nonplanar_textured_face_preserves_vertices_sides_and_projective_uvs() {
    let mut source = skpwright::Model::parse(TEXTURE).unwrap();
    let run = source.geometry.iter_mut().find(|run| run.def_index.is_none()).unwrap();
    let index = run.mesh.faces[0].outer[0] as usize;
    run.mesh.vertices[index][2] += 0.0001;
    run.mesh.faces[0].hidden = true;
    if run.mesh.faces[0].front_material.is_none() {
        run.mesh.faces[0].front_material = run.mesh.faces[0].back_material;
        if let Some(texture) = &mut run.mesh.faces[0].texture {
            texture.front = texture.back;
        }
    }
    run.mesh.faces[0].back_material = run.mesh.faces[0].front_material;
    let expected_vertices = run.mesh.vertices.clone();
    let original = run.mesh.faces[0].clone();
    let applied = source.applied_size_of(original.front_material.unwrap()).unwrap();
    let expected_uv = original.uv_xform(skpwright::Side::Front, applied).unwrap();
    let imported = read(&encode_source(&source), Options::default()).unwrap();
    assert!(imported.warnings.iter().any(|w| w.contains("Non-planar faces are triangulated")));
    let doc = imported.document;
    doc.root.mesh.validate().unwrap();
    assert_eq!(doc.root.mesh.face_count(), 2);
    assert_eq!(doc.root.mesh.vertex_count(), expected_vertices.len());
    for (_, v) in doc.root.mesh.vertices() {
        assert!(expected_vertices.iter().any(|p| v.pos.distance(DVec3::from_array(*p)) < 1e-12));
    }
    for (id, face) in doc.root.mesh.faces() {
        assert!(face.hidden);
        assert_eq!(face.material, face.back_material);
        let texture = doc.materials[&face.material.unwrap()].texture.unwrap();
        let normal = doc.root.mesh.face_normal(id).unwrap();
        let mut rebase = None;
        for &vertex in &face.outer {
            let p = doc.root.mesh.pos(vertex);
            let expected = expected_uv.apply(p.to_array());
            let actual = rysmo_model::uv_at(p, normal, texture.size, &face.uv.unwrap());
            let shift = rysmo_math::DVec2::new(actual[0] - expected[0], actual[1] - (1. - expected[1]));
            assert!(shift.distance(shift.round()) < 1e-7);
            if let Some(previous) = rebase {
                assert!(shift.distance(previous) < 1e-7);
            }
            rebase = Some(shift);
        }
    }
    let diagonals: Vec<_> = doc.root.mesh.edges().filter(|(_, edge)| edge.soft).collect();
    assert_eq!(diagonals.len(), 1);
    assert!(diagonals[0].1.smooth);
    for version in [Version::V2017, Version::V2026] {
        let again = read(&export(&doc, version).unwrap().bytes, Options::default()).unwrap();
        assert_eq!(again.document.root.mesh.face_count(), 2);
        assert!(!again.warnings.iter().any(|w| w.contains("Non-planar")));
    }
}

#[test]
fn triangulating_nonplanar_faces_keeps_holes_open() {
    let mut source = skpwright::Model::parse(HOLE).unwrap();
    let run = source.geometry.iter_mut().find(|run| run.def_index.is_none()).unwrap();
    let index = run.mesh.faces.iter().find(|face| !face.holes.is_empty()).unwrap().outer[0] as usize;
    run.mesh.vertices[index][2] += 0.0001;
    let original = read(HOLE, Options::default()).unwrap().document;
    let expected_area: f64 = original.root.mesh.faces().map(|(id, _)| original.root.mesh.face_area(id)).sum();
    let imported = read(&encode_source(&source), Options::default()).unwrap();
    assert!(imported.warnings.iter().any(|w| w.contains("Non-planar faces are triangulated")));
    let mesh = &imported.document.root.mesh;
    mesh.validate().unwrap();
    assert!(mesh.face_count() > original.root.mesh.face_count());
    let projected_area: f64 = mesh
        .faces()
        .flat_map(|(id, _)| rysmo_mesh::triangulate_face(mesh, id))
        .map(|t| {
            let p = t.map(|v| mesh.pos(v));
            (p[1] - p[0]).cross(p[2] - p[0]).z.abs() / 2.
        })
        .sum();
    assert!((projected_area - expected_area).abs() < 1e-8);
}
