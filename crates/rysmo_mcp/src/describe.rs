//! `describe_model`: what an agent needs to know to refer to things.

use rysmo_math::DVec3;
use rysmo_model::{Editor, Guide};
use serde_json::{Value, json};

use crate::tools::len;

fn r(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn v3(p: DVec3) -> Value {
    json!([r(p.x), r(p.y), r(p.z)])
}

fn context(editor: &Editor) -> Value {
    let doc = editor.doc();
    let names: Vec<Value> = (1..=editor.context().len())
        .filter_map(|k| doc.resolve(&editor.context()[..k]).ok().flatten())
        .map(|d| json!({"definition": d.0, "name": doc.definitions.get(&d).map(|x| x.name.clone())}))
        .collect();
    json!({"path": editor.context().iter().map(|i| i.0).collect::<Vec<_>>(), "open": names})
}

/// Counts, bounds and where editing happens.
pub fn summary(editor: &Editor) -> (String, Value) {
    let doc = editor.doc();
    let active = editor.active();
    let mesh = &active.mesh;
    let bounds = doc.definition_bounds(editor.active_definition());
    let mut text = format!(
        "{} faces, {} edges, {} groups/components, {} guides in {}; {} definitions, {} materials, {} tags in the document.",
        mesh.face_count(),
        mesh.edge_count(),
        active.instances.len(),
        active.guides.len(),
        if editor.context().is_empty() { "the model".to_string() } else { format!("the open {}", active.name) },
        doc.definitions.len(),
        doc.materials.len(),
        doc.tags.len(),
    );
    if let Some((lo, hi)) = bounds {
        let s = hi - lo;
        text.push_str(&format!(" Size {} x {} x {}.", len(editor, s.x), len(editor, s.y), len(editor, s.z)));
    }
    let data = json!({
        "context": context(editor),
        "units": doc.units.suffix(),
        "counts": {"faces": mesh.face_count(), "edges": mesh.edge_count(), "vertices": mesh.vertex_count(),
                   "instances": active.instances.len(), "guides": active.guides.len(),
                   "definitions": doc.definitions.len(), "materials": doc.materials.len(), "tags": doc.tags.len()},
        "bounds": bounds.map(|(lo, hi)| json!({"min": v3(lo), "max": v3(hi)})),
        "undo": editor.undo_label(),
    });
    (text, data)
}

/// Everything in the context being edited, with ids.
pub fn full(editor: &Editor, limit: usize) -> (String, Value) {
    let (text, mut data) = summary(editor);
    let doc = editor.doc();
    let active = editor.active();
    let mesh = &active.mesh;
    let faces: Vec<Value> = mesh
        .faces()
        .take(limit)
        .map(|(id, f)| {
            let pts = mesh.loop_points(&f.outer);
            let centre = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
            json!({
                "id": id.0,
                "normal": mesh.face_normal(id).map(v3),
                "area": r(mesh.face_area(id)),
                "centre": v3(centre),
                "vertices": pts.iter().map(|p| v3(*p)).collect::<Vec<_>>(),
                "holes": f.holes.len(),
                "material": f.material.map(|m| m.0),
                "back_material": f.back_material.map(|m| m.0),
            })
        })
        .collect();
    let edges: Vec<Value> = mesh
        .edges()
        .take(limit)
        .map(|(id, e)| {
            let (a, b) = mesh.edge_points(id);
            let mut j = json!({"id": id.0, "from": v3(a), "to": v3(b), "faces": mesh.faces_of_edge(id).iter().map(|f| f.0).collect::<Vec<_>>()});
            if e.soft {
                j["soft"] = json!(true);
            }
            if e.hidden {
                j["hidden"] = json!(true);
            }
            j
        })
        .collect();
    let instances: Vec<Value> = active
        .instances
        .iter()
        .map(|(id, i)| {
            let def = doc.definitions.get(&i.definition);
            let bounds = doc.definition_bounds(Some(i.definition)).map(|(lo, hi)| {
                let c = rysmo_model::corners(lo, hi).map(|p| i.transform.transform_point3(p));
                let (mut a, mut b) = (c[0], c[0]);
                for p in c {
                    a = a.min(p);
                    b = b.max(p);
                }
                json!({"min": v3(a), "max": v3(b)})
            });
            json!({
                "id": id.0, "definition": i.definition.0, "name": def.map(|d| d.name.clone()), "group": def.is_some_and(|d| d.group),
                "origin": v3(i.transform.w_axis.truncate()), "x_axis": v3(i.transform.x_axis.truncate()),
                "y_axis": v3(i.transform.y_axis.truncate()), "z_axis": v3(i.transform.z_axis.truncate()),
                "tag": i.tag.map(|t| t.0), "material": i.material.map(|m| m.0), "bounds": bounds,
            })
        })
        .collect();
    let definitions: Vec<Value> = doc
        .definitions
        .iter()
        .map(|(id, d)| json!({"id": id.0, "name": d.name, "group": d.group, "faces": d.mesh.face_count(), "instances": doc.instance_count(*id)}))
        .collect();
    let guides: Vec<Value> = active
        .guides
        .iter()
        .map(|(id, g)| match g {
            Guide::Line { origin, dir, .. } => json!({"id": id.0, "line": {"origin": v3(*origin), "dir": v3(*dir)}}),
            Guide::Point { p, origin, .. } => json!({"id": id.0, "point": v3(*p), "origin": origin.map(v3)}),
        })
        .collect();
    let materials: Vec<Value> =
        doc.materials.iter().map(|(id, m)| json!({"id": id.0, "name": m.name, "color": m.color})).collect();
    let tags: Vec<Value> =
        doc.tags.iter().map(|(id, t)| json!({"id": id.0, "name": t.name, "visible": t.visible})).collect();
    let truncated = mesh.face_count() > limit || mesh.edge_count() > limit;
    data["faces"] = json!(faces);
    data["edges"] = json!(edges);
    data["instances"] = json!(instances);
    data["definitions"] = json!(definitions);
    data["guides"] = json!(guides);
    data["materials"] = json!(materials);
    data["tags"] = json!(tags);
    data["truncated"] = json!(truncated);
    let mut text = text;
    text.push('\n');
    text.push_str(&serde_json::to_string(&data).expect("plain data"));
    (text, data)
}
