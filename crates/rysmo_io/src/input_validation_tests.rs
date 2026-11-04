#[test]
fn excessive_native_identifier_returns_error() {
    let text = r#"{"format":"rysmo","version":2,"tags":[{"id":18446744073709551615,"name":"tag"}]}"#;
    assert!(crate::load(text).is_err());
}
#[test]
fn singular_native_instance_returns_error() {
    let text = r#"{"format":"rysmo","version":2,"definitions":[{"id":1,"name":"part"}],"instances":[{"id":2,"def":1,"o":[0,0,0],"x":[0,0,0],"y":[0,1,0],"z":[0,0,1]}]}"#;
    assert!(crate::load(&text.replace("\"x\":[0,0,0]", "\"x\":[1,0,0]")).is_ok());
    assert!(crate::load(text).is_err());
}
#[test]
fn native_texture_requires_positive_dimensions() {
    let text = r#"{"format":"rysmo","version":3,"images":[{"id":1,"name":"image","mime":"image/png","data":"AQ=="}],"materials":[{"id":2,"name":"material","color":[1,1,1,1],"texture":{"image":1,"size":[0,1]}}]}"#;
    assert!(crate::load(&text.replace("[0,1]", "[1,1]")).is_ok());
    assert!(crate::load(text).is_err());
}

#[test]
fn maximum_geometry_ids_and_scene_references_return_errors() {
    for id in [u64::MAX, u64::MAX - 1] {
        let source = serde_json::json!({"format":"rysmo","version":2,"vertices":[{"id":id,"p":[0,0,0]}]});
        assert!(crate::load(&source.to_string()).is_err());
    }
    let mut doc = rysmo_model::Document::new();
    let mut scene = rysmo_model::scenes::Scene::capture(&doc, Default::default(), "View".into());
    scene.id = 1;
    scene.tags.insert(rysmo_model::TagId(u64::MAX), true);
    doc.scenes.push(scene);
    assert!(crate::load(&crate::save(&doc)).is_err());
}
#[test]
fn native_loading_rejects_negative_texture_dimensions() {
    let text = r#"{"format":"rysmo","version":3,"images":[{"id":1,"name":"image","mime":"image/png","data":"AQ=="}],"materials":[{"id":2,"name":"material","color":[1,1,1,1],"texture":{"image":1,"size":[-1,1]}}]}"#;
    assert!(crate::load(text).is_err());
}
#[test]
fn invertible_mirrored_instances_remain_supported() {
    let text = r#"{"format":"rysmo","version":2,"definitions":[{"id":1,"name":"part"}],"instances":[{"id":2,"def":1,"o":[2,3,4],"x":[-2,0,0],"y":[0,3,0],"z":[0,0,4]}]}"#;
    let doc = crate::load(text).unwrap();
    let round_trip = crate::load(&crate::save(&doc)).unwrap();
    assert_eq!(round_trip, doc);
}

#[test]
fn native_guides_reject_directions_that_overflow_normalization() {
    let text = r#"{"format":"rysmo","version":2,"guides":[{"id":1,"line":{"origin":[0,0,0],"dir":[1e308,1e308,0]}}]}"#;
    assert!(crate::load(&text.replace("1e308", "1")).is_ok());
    assert!(crate::load(text).is_err());
}

#[test]
fn saving_large_finite_coordinates_keeps_them_finite() {
    let source = r#"{"format":"rysmo","version":2,"guides":[{"id":1,"point":{"p":[1e308,0,0]}}]}"#;
    let doc = crate::load(source).unwrap();
    let saved = crate::save(&doc);
    let loaded = crate::load(&saved).expect("Saving finite coordinates must not replace them with null");
    assert_eq!(loaded, doc);
}
