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

#[test]
fn malformed_native_inputs_never_panic() {
    let valid = include_str!("../../../testdata/dimensions-preview.rysmo");
    let original: serde_json::Value = serde_json::from_str(valid).unwrap();
    let mut cases =
        vec![String::new(), "null".into(), "[]".into(), "{".into(), format!("{}0{}", "[".repeat(256), "]".repeat(256))];
    // Truncation at every UTF-8 boundary samples partial strings, numbers and arrays.
    for at in (0..valid.len()).step_by(37).filter(|at| valid.is_char_boundary(*at)) {
        cases.push(valid[..at].to_owned());
    }
    for field in original.as_object().unwrap().keys() {
        for replacement in
            [serde_json::Value::Null, serde_json::json!([]), serde_json::json!({}), serde_json::json!(1e308)]
        {
            let mut altered = original.clone();
            altered[field] = replacement;
            cases.push(altered.to_string());
        }
    }
    for section in ["vertices", "edges", "faces", "dimensions"] {
        if let Some(items) = original[section].as_array() {
            for index in 0..items.len().min(8) {
                for field in items[index].as_object().unwrap().keys() {
                    for value in [
                        serde_json::json!(u64::MAX),
                        serde_json::json!([]),
                        serde_json::json!([999999]),
                        serde_json::Value::Null,
                    ] {
                        let mut altered = original.clone();
                        altered[section][index][field] = value;
                        cases.push(altered.to_string());
                    }
                }
            }
        }
    }
    for (index, source) in cases.iter().enumerate() {
        let result = std::panic::catch_unwind(|| crate::load(source));
        assert!(result.is_ok(), "loader panicked on mutation {index}: {source}");
        if let Ok(doc) = result.unwrap() {
            doc.root.mesh.validate().unwrap();
            let saved = crate::save(&doc);
            assert!(crate::load(&saved).is_ok(), "accepted mutation {index} cannot reopen");
        }
    }
}

#[test]
fn native_faces_reject_short_holes_and_overflowing_planes() {
    let mut editor = rysmo_model::Editor::new();
    editor
        .apply(rysmo_model::Command::DrawRectangle {
            corner: rysmo_math::DVec3::ZERO,
            u: rysmo_math::DVec3::X,
            v: rysmo_math::DVec3::Y,
            facing: None,
        })
        .unwrap();
    let source: serde_json::Value = serde_json::from_str(&crate::save(editor.doc())).unwrap();
    for hole in
        [serde_json::json!([]), serde_json::json!([source["faces"][0]["outer"][0], source["faces"][0]["outer"][1]])]
    {
        let mut invalid = source.clone();
        invalid["faces"][0]["holes"] = serde_json::json!([hole]);
        assert!(crate::load(&invalid.to_string()).is_err(), "short face hole was accepted");
    }
    let mut invalid = source;
    for vertex in invalid["vertices"].as_array_mut().unwrap() {
        for coordinate in vertex["p"].as_array_mut().unwrap() {
            *coordinate = serde_json::json!(coordinate.as_f64().unwrap() * 1e308);
        }
    }
    assert!(crate::load(&invalid.to_string()).is_err(), "overflowing face plane was accepted");
}

#[test]
fn native_edges_reject_duplicates_and_cross_context_references() {
    let base = serde_json::json!({"format":"rysmo", "version":20,
        "vertices":[{"id":1,"p":[0,0,0]},{"id":2,"p":[1,0,0]}],
        "edges":[{"id":3,"v":[1,2]}]});
    assert!(crate::load(&base.to_string()).is_ok());
    for vertices in [serde_json::json!([1, 2]), serde_json::json!([2, 1])] {
        let mut invalid = base.clone();
        invalid["edges"].as_array_mut().unwrap().push(serde_json::json!({"id":4,"v":vertices}));
        assert!(crate::load(&invalid.to_string()).is_err());
    }
    let mut invalid = base;
    invalid["definitions"] = serde_json::json!([{"id":5,"name":"Other context"}]);
    invalid["vertices"][0]["in"] = serde_json::json!(5);
    assert!(crate::load(&invalid.to_string()).is_err());
}

#[test]
fn sparse_unordered_records_preserve_ids_and_reject_duplicates() {
    let vertices: Vec<_> = (0..1024u64).map(|i| serde_json::json!({"id":i*256+1,"p":[i,0,0]})).collect();
    let mut source = serde_json::json!({"format":"rysmo","version":20,"vertices":vertices});
    let expected = crate::save(&crate::load(&source.to_string()).unwrap());
    source["vertices"].as_array_mut().unwrap().reverse();
    assert_eq!(crate::save(&crate::load(&source.to_string()).unwrap()), expected);
    source["vertices"].as_array_mut().unwrap().rotate_left(317);
    assert_eq!(crate::save(&crate::load(&source.to_string()).unwrap()), expected);
    let duplicate = source["vertices"][456].clone();
    source["vertices"].as_array_mut().unwrap().push(duplicate);
    assert!(crate::load(&source.to_string()).unwrap_err().to_string().contains("used twice"));
}
