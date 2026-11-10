use rysmo_math::DVec3;
use rysmo_model::{Command, Editor, Element};
use serde_json::json;
#[test]
fn native_and_import_reject_exponential_hierarchies() {
    let mut definitions = Vec::new();
    let mut instances = Vec::new();
    let mut next = 100u64;
    for id in 1..=17u64 {
        definitions.push(json!({"id":id,"name":format!("level-{id}")}));
        if id < 17 {
            for _ in 0..2 {
                instances.push(json!({"id":next,"in":id,"def":id+1,"o":[0,0,0],"x":[1,0,0],"y":[0,1,0],"z":[0,0,1]}));
                next += 1;
            }
        }
    }
    instances.push(json!({"id":next,"def":1,"o":[0,0,0],"x":[1,0,0],"y":[0,1,0],"z":[0,0,1]}));
    let text = json!({"format":"rysmo","version":20,"definitions":definitions,"instances":instances}).to_string();
    assert!(crate::load(&text).unwrap_err().to_string().contains("Expanded model"));
    let error = crate::import::read("test.rysmo", text.as_bytes(), Default::default()).err().unwrap();
    assert!(error.to_string().contains("Expanded model"));
}
#[test]
fn tiny_invertible_instance_transforms_survive_save_and_reopen() {
    let text = r#"{"format":"rysmo","version":20,"definitions":[{"id":1,"name":"part"}],"vertices":[{"id":3,"in":1,"p":[0,0,0]},{"id":4,"in":1,"p":[1000000000,0,0]}],"edges":[{"id":5,"in":1,"v":[3,4]}],"instances":[{"id":2,"def":1,"o":[0,0,0],"x":[1,0,0],"y":[0,1,0],"z":[0,0,1]}]}"#;
    let mut editor = Editor::with_document(crate::load(text).unwrap());
    for _ in 0..3 {
        editor
            .apply(Command::Scale {
                elements: vec![Element::Instance(rysmo_model::InstanceId(2))],
                origin: DVec3::ZERO,
                factors: DVec3::splat(0.0001),
            })
            .unwrap();
    }
    let transform = editor.doc().root.instances[&rysmo_model::InstanceId(2)].transform;
    assert!(transform.inverse().is_finite());
    let (lo, hi) = editor.doc().definition_bounds(None).unwrap();
    assert!((hi.distance(lo) - 0.001).abs() < 1e-12);
    let saved = crate::save(editor.doc());
    let loaded = crate::load(&saved).unwrap();
    assert_eq!(loaded.root.instances[&rysmo_model::InstanceId(2)].transform, transform);
    assert_eq!(loaded.definition_bounds(None), editor.doc().definition_bounds(None));
}
#[test]
fn independent_obj_exports_keep_each_others_textures() {
    fn export(bytes: Vec<u8>, mtl: &str) -> crate::obj::ObjExport {
        let mut editor = Editor::new();
        let image = editor
            .apply(Command::AddImage { name: "wood.png".into(), mime: "image/png".into(), data: bytes })
            .unwrap()
            .image
            .unwrap();
        editor
            .apply(Command::AddMaterial {
                name: "wood".into(),
                color: [1.; 4],
                texture: Some(rysmo_model::Texture { image, size: [1., 1.] }),
            })
            .unwrap();
        crate::obj::export(editor.doc(), mtl)
    }
    let a = export(vec![1, 2, 3], "house.mtl");
    let b = export(vec![4, 5, 6], "other.mtl");
    assert_ne!(a.images[0].0, b.images[0].0);
    assert_ne!(a.images[0].1, b.images[0].1);
    let directory = std::env::temp_dir().join(format!(
        "rysmo-obj-companions-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let a_path = directory.join("house.obj");
    crate::obj::write(&a_path, &a).unwrap();
    crate::obj::write(&directory.join("other.obj"), &b).unwrap();
    assert_eq!(std::fs::read(directory.join(&a.images[0].0)).unwrap(), a.images[0].1);
    assert_eq!(std::fs::read(directory.join(&b.images[0].0)).unwrap(), b.images[0].1);
    crate::obj::write(&a_path, &a).unwrap();
    // Even a hash collision or unrelated existing companion cannot be overwritten.
    std::fs::write(directory.join(&b.images[0].0), b"unrelated").unwrap();
    let previous = std::fs::read(&a_path).unwrap();
    assert!(crate::obj::write(&a_path, &b).is_err());
    assert_eq!(std::fs::read(&a_path).unwrap(), previous);
    assert_eq!(std::fs::read(directory.join(&b.images[0].0)).unwrap(), b"unrelated");
    std::fs::remove_dir_all(directory).unwrap();
}
