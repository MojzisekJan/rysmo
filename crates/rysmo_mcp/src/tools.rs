//! The MCP tools: their definitions and what each call does.

use rysmo_math::{DVec3, EPS};
use rysmo_model::{Command, DefinitionId, EdgeId, Element, FaceId, InstanceId, MaterialId, Outcome, TagId, Units};
use serde_json::{Value, json};

use crate::{Session, ToolOutput, describe};

type R = Result<Option<ToolOutput>, String>;

fn vec3_schema(desc: &str) -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": desc})
}

fn ids_schema(desc: &str) -> Value {
    json!({"type": "array", "items": {"type": "integer"}, "description": desc})
}

fn tool(name: &str, title: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({"type": "object", "properties": properties, "additionalProperties": false});
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    json!({"name": name, "title": title, "description": description, "inputSchema": schema})
}

/// Every tool, in a fixed order.
pub fn tool_definitions() -> Vec<Value> {
    let elements =
        ids_schema("Ids of edges, faces, groups/components (instances) or guides in the context being edited.");
    let normal = vec3_schema("Normal of the drawing plane. Default [0,0,1] (horizontal).");
    let face_plane =
        json!({"type": "integer", "description": "Draw in the plane of this face instead of giving a normal."});
    vec![
        tool(
            "new_document",
            "New document",
            "Start an empty model.",
            json!({"units": {"type": "string", "enum": ["mm", "cm", "m", "in", "ft"]}}),
            &[],
        ),
        tool("open_document", "Open document", "Open a .rysmo file.", json!({"path": {"type": "string"}}), &["path"]),
        tool(
            "save_document",
            "Save document",
            "Save the model as .rysmo (text, one element per line). Without a path, saves where it was opened or last saved.",
            json!({"path": {"type": "string"}}),
            &[],
        ),
        tool(
            "export",
            "Export",
            "Export the whole model, groups and components flattened, as Wavefront OBJ (with an MTL file next to it), binary glTF, or STL or 3MF in millimetres for 3D printing. STL and 3MF report problems that would spoil a print (holes, reversed or inner faces).",
            json!({"path": {"type": "string"}, "format": {"type": "string", "enum": ["obj", "glb", "stl", "3mf"]}}),
            &["path"],
        ),
        tool(
            "describe_model",
            "Describe model",
            "List the context being edited: faces (id, normal, area, centre, material), edges, groups and components, definitions, tags, materials, guides and bounds. Use the ids in other tools.",
            json!({
                "detail": {"type": "string", "enum": ["summary", "full"], "description": "summary: counts and bounds only. Default full."},
                "limit": {"type": "integer", "description": "At most this many faces and edges are listed. Default 500."}
            }),
            &[],
        ),
        tool(
            "get_document_text",
            "Document text",
            "The model as .rysmo text, exactly as save_document would write it.",
            json!({}),
            &[],
        ),
        tool(
            "draw_line",
            "Line",
            "Draw connected lines through points (metres, Z up). Lines stick to existing geometry; a closed planar loop makes a face.",
            json!({"points": {"type": "array", "items": vec3_schema("A point."), "minItems": 2}, "closed": {"type": "boolean", "description": "Join the last point back to the first."}}),
            &["points"],
        ),
        tool(
            "draw_rectangle",
            "Rectangle",
            "Draw a rectangle from a corner. Give width and depth (along the plane's axes, lined up with the model axes) or the two side vectors u and v.",
            json!({"corner": vec3_schema("First corner."), "width": {"type": "number"}, "depth": {"type": "number"},
                   "u": vec3_schema("First side."), "v": vec3_schema("Second side."), "normal": normal.clone(), "face": face_plane.clone()}),
            &["corner"],
        ),
        tool(
            "draw_circle",
            "Circle",
            "Draw a circle as a polygon of segments, 24 by default.",
            json!({"centre": vec3_schema("Centre."), "radius": {"type": "number"}, "segments": {"type": "integer"}, "normal": normal.clone(), "face": face_plane.clone()}),
            &["centre", "radius"],
        ),
        tool(
            "draw_polygon",
            "Polygon",
            "Draw a regular polygon inscribed in a circle of the given radius.",
            json!({"centre": vec3_schema("Centre."), "radius": {"type": "number"}, "sides": {"type": "integer"}, "normal": normal.clone(), "face": face_plane.clone()}),
            &["centre", "radius", "sides"],
        ),
        tool(
            "draw_arc",
            "Arc",
            "Draw a 2-point arc from start to end that bulges by `bulge` metres to the left of the chord, seen from the plane's normal. bulge = half the chord is a half circle.",
            json!({"start": vec3_schema("Start."), "end": vec3_schema("End."), "bulge": {"type": "number"}, "segments": {"type": "integer"}, "normal": normal, "face": face_plane}),
            &["start", "end", "bulge"],
        ),
        tool(
            "push_pull",
            "Push/Pull",
            "Move a face along its normal by distance (negative pushes in), building side faces. A face pushed through a solid cuts a hole.",
            json!({"face": {"type": "integer"}, "distance": {"type": "number"}, "keep_base": {"type": "boolean", "description": "Leave the starting face in place (Ctrl in the app)."}}),
            &["face", "distance"],
        ),
        tool(
            "move",
            "Move",
            "Move elements by an offset, or with copies > 0 make that many copies, each one offset further.",
            json!({"elements": elements.clone(), "offset": vec3_schema("Offset in metres."), "copies": {"type": "integer"}}),
            &["elements", "offset"],
        ),
        tool(
            "rotate",
            "Rotate",
            "Rotate elements by angle degrees about an axis through centre (right-handed). With copies > 0, make that many rotated copies.",
            json!({"elements": elements.clone(), "centre": vec3_schema("A point on the axis."), "axis": vec3_schema("Axis direction. Default [0,0,1]."), "angle": {"type": "number"}, "copies": {"type": "integer"}}),
            &["elements", "centre", "angle"],
        ),
        tool(
            "scale",
            "Scale",
            "Scale elements about an origin by one factor or one per axis. Negative factors mirror.",
            json!({"elements": elements.clone(), "origin": vec3_schema("Fixed point."), "factor": {"description": "A number or [x, y, z]."}}),
            &["elements", "origin", "factor"],
        ),
        tool(
            "offset",
            "Offset",
            "Offset a face's outline (positive inwards) or a chain of edges (positive to the left, seen from normal).",
            json!({"face": {"type": "integer"}, "edges": ids_schema("A chain of edges."), "normal": vec3_schema("Plane normal for an edge chain."), "distance": {"type": "number"}}),
            &["distance"],
        ),
        tool(
            "follow_me",
            "Follow Me",
            "Sweep a face along a chain of edges (the path).",
            json!({"face": {"type": "integer"}, "path": ids_schema("Edges of the path, any order.")}),
            &["face", "path"],
        ),
        tool(
            "erase",
            "Erase",
            "Erase elements. Erasing an edge between two coplanar faces merges them.",
            json!({"elements": elements.clone()}),
            &["elements"],
        ),
        tool("reverse_faces", "Reverse Faces", "Turn faces round.", json!({"faces": ids_schema("Faces.")}), &["faces"]),
        tool(
            "soften_edges",
            "Soften Edges",
            "Soften and smooth the edges whose faces meet at less than angle degrees (default 20); harden the others.",
            json!({"edges": ids_schema("Edges."), "angle": {"type": "number"}}),
            &["edges"],
        ),
        tool(
            "make_group",
            "Make Group",
            "Make a group of elements.",
            json!({"elements": elements.clone()}),
            &["elements"],
        ),
        tool(
            "make_component",
            "Make Component",
            "Make a component of elements. Its definition can be placed again with place_component.",
            json!({"elements": elements.clone(), "name": {"type": "string"}}),
            &["elements"],
        ),
        tool(
            "place_component",
            "Place Component",
            "Place another instance of a definition (see describe_model) with its origin at origin, turned about blue and scaled.",
            json!({"definition": {"type": "integer"}, "origin": vec3_schema("Where the definition's origin goes."), "rotation": {"type": "number", "description": "Degrees about the blue axis."}, "scale": {"type": "number"}}),
            &["definition", "origin"],
        ),
        tool(
            "explode",
            "Explode",
            "Replace a group or component instance by its geometry.",
            json!({"instance": {"type": "integer"}}),
            &["instance"],
        ),
        tool(
            "make_unique",
            "Make Unique",
            "Give an instance its own copy of its definition.",
            json!({"instance": {"type": "integer"}}),
            &["instance"],
        ),
        tool(
            "edit_component",
            "Edit Group/Component",
            "Open a group or component of the current context for editing. Tools then work inside it, in its coordinates.",
            json!({"instance": {"type": "integer"}}),
            &["instance"],
        ),
        tool(
            "close_component",
            "Close Group/Component",
            "Close the innermost open group or component.",
            json!({}),
            &[],
        ),
        tool(
            "add_material",
            "New Material",
            "Add a material: colour as sRGB 0..1, [r, g, b] or [r, g, b, opacity].",
            json!({"name": {"type": "string"}, "color": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 4}}),
            &["name", "color"],
        ),
        tool(
            "paint",
            "Paint",
            "Paint faces (front, or back with back: true) or groups/components with a material, given by id or name. material null removes paint.",
            json!({"elements": elements.clone(), "material": {"description": "Material id, name, or null."}, "back": {"type": "boolean"}}),
            &["elements", "material"],
        ),
        tool("add_tag", "Add Tag", "Add a tag (layer).", json!({"name": {"type": "string"}}), &["name"]),
        tool(
            "set_tag",
            "Tag",
            "Rename a tag or show/hide it.",
            json!({"tag": {"description": "Tag id or name."}, "name": {"type": "string"}, "visible": {"type": "boolean"}}),
            &["tag"],
        ),
        tool(
            "assign_tag",
            "Change Tag",
            "Put groups/components on a tag (id or name; null for untagged).",
            json!({"instances": ids_schema("Instances."), "tag": {"description": "Tag id, name, or null."}}),
            &["instances", "tag"],
        ),
        tool(
            "add_guide",
            "Guide",
            "Add a construction guide: a line through origin along dir, or a point.",
            json!({"origin": vec3_schema("Point on the line."), "dir": vec3_schema("Line direction."), "point": vec3_schema("A guide point.")}),
            &[],
        ),
        tool(
            "set_units",
            "Units",
            "Display units for lengths: mm, cm, m, in or ft. Coordinates stay in metres.",
            json!({"units": {"type": "string", "enum": ["mm", "cm", "m", "in", "ft"]}}),
            &["units"],
        ),
        tool("undo", "Undo", "Undo the last change.", json!({}), &[]),
        tool("redo", "Redo", "Redo the last undone change.", json!({}), &[]),
    ]
}

// ----- argument helpers -----

fn get<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.get(key).filter(|v| !v.is_null())
}

fn num(args: &Value, key: &str) -> Result<f64, String> {
    get(args, key).and_then(Value::as_f64).ok_or_else(|| format!("`{key}` must be a number"))
}

fn opt_num(args: &Value, key: &str) -> Result<Option<f64>, String> {
    match get(args, key) {
        None => Ok(None),
        Some(v) => v.as_f64().map(Some).ok_or_else(|| format!("`{key}` must be a number")),
    }
}

fn int(args: &Value, key: &str) -> Result<u64, String> {
    get(args, key).and_then(Value::as_u64).ok_or_else(|| format!("`{key}` must be an id (a positive integer)"))
}

fn vec3(v: &Value, key: &str) -> Result<DVec3, String> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| format!("`{key}` must be [x, y, z]"))?;
    let f = |i: usize| a[i].as_f64().ok_or_else(|| format!("`{key}` must hold numbers"));
    Ok(DVec3::new(f(0)?, f(1)?, f(2)?))
}

fn arg_vec3(args: &Value, key: &str) -> Result<DVec3, String> {
    vec3(get(args, key).ok_or_else(|| format!("`{key}` is missing"))?, key)
}

fn opt_vec3(args: &Value, key: &str) -> Result<Option<DVec3>, String> {
    get(args, key).map(|v| vec3(v, key)).transpose()
}

fn id_list(args: &Value, key: &str) -> Result<Vec<u64>, String> {
    let a = get(args, key).and_then(Value::as_array).ok_or_else(|| format!("`{key}` must be a list of ids"))?;
    a.iter().map(|v| v.as_u64().ok_or_else(|| format!("`{key}` must hold integer ids"))).collect()
}

/// Ids are unique in the document, so the kind of element follows from the id.
fn element(s: &Session, id: u64) -> Result<Element, String> {
    let active = s.editor.active();
    if active.mesh.face(FaceId(id)).is_some() {
        Ok(Element::Face(FaceId(id)))
    } else if active.mesh.edge(EdgeId(id)).is_some() {
        Ok(Element::Edge(EdgeId(id)))
    } else if active.instances.contains_key(&InstanceId(id)) {
        Ok(Element::Instance(InstanceId(id)))
    } else if active.guides.contains_key(&rysmo_model::GuideId(id)) {
        Ok(Element::Guide(rysmo_model::GuideId(id)))
    } else if active.texts.contains_key(&rysmo_model::TextId(id)) {
        Ok(Element::Text(rysmo_model::TextId(id)))
    } else if active.dimensions.contains_key(&rysmo_model::DimensionId(id)) {
        Ok(Element::Dimension(rysmo_model::DimensionId(id)))
    } else {
        Err(format!(
            "{id} is not an edge, face, group, component, guide, dimension or text of the context being edited (see describe_model)"
        ))
    }
}

fn elements(s: &Session, args: &Value, key: &str) -> Result<Vec<Element>, String> {
    id_list(args, key)?.into_iter().map(|id| element(s, id)).collect()
}

fn face(s: &Session, id: u64) -> Result<FaceId, String> {
    match element(s, id)? {
        Element::Face(f) => Ok(f),
        _ => Err(format!("{id} is not a face")),
    }
}

fn instance(s: &Session, id: u64) -> Result<InstanceId, String> {
    match element(s, id)? {
        Element::Instance(i) => Ok(i),
        _ => Err(format!("{id} is not a group or component")),
    }
}

fn edges(s: &Session, args: &Value, key: &str) -> Result<Vec<EdgeId>, String> {
    id_list(args, key)?
        .into_iter()
        .map(|id| match element(s, id)? {
            Element::Edge(e) => Ok(e),
            _ => Err(format!("{id} is not an edge")),
        })
        .collect()
}

/// The plane normal from `face` or `normal`, default up.
fn plane_normal(s: &Session, args: &Value) -> Result<DVec3, String> {
    if let Some(f) = get(args, "face") {
        let f = face(s, f.as_u64().ok_or("`face` must be an id")?)?;
        return s.editor.mesh().face_normal(f).ok_or_else(|| "the face is degenerate".to_string());
    }
    let n = opt_vec3(args, "normal")?.unwrap_or(DVec3::Z);
    if n.length() < EPS {
        return Err("`normal` must not be zero".into());
    }
    Ok(n.normalize())
}

/// Sides lined up with the model axes, as the Rectangle tool does.
fn plane_axes(n: DVec3) -> (DVec3, DVec3) {
    let axis = [DVec3::X, DVec3::Y, DVec3::Z]
        .into_iter()
        .min_by(|a, b| a.dot(n).abs().total_cmp(&b.dot(n).abs()))
        .unwrap_or(DVec3::X);
    let u = (axis - n * axis.dot(n)).normalize();
    (u, n.cross(u))
}

fn material(s: &Session, v: Option<&Value>) -> Result<Option<MaterialId>, String> {
    match v {
        None => Ok(None),
        Some(Value::String(name)) => {
            s.editor.doc().material_by_name(name).map(Some).ok_or_else(|| format!("no material named \"{name}\""))
        }
        Some(v) => {
            let id = MaterialId(v.as_u64().ok_or("`material` must be an id, a name or null")?);
            if s.editor.doc().materials.contains_key(&id) { Ok(Some(id)) } else { Err(format!("no material {id}")) }
        }
    }
}

fn tag(s: &Session, v: Option<&Value>) -> Result<Option<TagId>, String> {
    match v {
        None => Ok(None),
        Some(Value::String(name)) => {
            s.editor.doc().tag_by_name(name).map(Some).ok_or_else(|| format!("no tag named \"{name}\""))
        }
        Some(v) => {
            let id = TagId(v.as_u64().ok_or("`tag` must be an id, a name or null")?);
            if s.editor.doc().tags.contains_key(&id) { Ok(Some(id)) } else { Err(format!("no tag {id}")) }
        }
    }
}

fn kind(e: &Element) -> (&'static str, u64) {
    match e {
        Element::Edge(x) => ("edge", x.0),
        Element::Face(x) => ("face", x.0),
        Element::Instance(x) => ("instance", x.0),
        Element::Guide(x) => ("guide", x.0),
        Element::Dimension(x) => ("dimension", x.0),
        Element::Text(x) => ("text", x.0),
        Element::Section(x) => ("section", *x),
    }
}

/// Runs a command and reports what it made.
fn run(s: &mut Session, cmd: Command) -> Result<Option<ToolOutput>, String> {
    let label = cmd.label();
    let out: Outcome = s.editor.apply(cmd).map_err(|e| format!("{label} failed: {e}"))?;
    Ok(Some(report(label, &out)))
}

fn report(label: &str, out: &Outcome) -> ToolOutput {
    let faces: Vec<u64> =
        out.created.iter().filter_map(|e| if let Element::Face(f) = e { Some(f.0) } else { None }).collect();
    let edges: Vec<u64> =
        out.created.iter().filter_map(|e| if let Element::Edge(x) = e { Some(x.0) } else { None }).collect();
    let instances: Vec<u64> =
        out.created.iter().filter_map(|e| if let Element::Instance(x) = e { Some(x.0) } else { None }).collect();
    let guides: Vec<u64> =
        out.created.iter().filter_map(|e| if let Element::Guide(x) = e { Some(x.0) } else { None }).collect();
    let mut parts = vec![format!("{label} done.")];
    if !faces.is_empty() {
        parts.push(format!("Faces: {faces:?}."));
    }
    if !edges.is_empty() {
        parts.push(format!("Edges: {edges:?}."));
    }
    if !instances.is_empty() {
        parts.push(format!("Instances: {instances:?}."));
    }
    if !guides.is_empty() {
        parts.push(format!("Guides: {guides:?}."));
    }
    if let Some(m) = out.material {
        parts.push(format!("Material {m}."));
    }
    if let Some(t) = out.tag {
        parts.push(format!("Tag {t}."));
    }
    if !out.changed {
        parts.push("Nothing changed.".into());
    }
    ToolOutput {
        text: parts.join(" "),
        data: json!({
            "changed": out.changed,
            "faces": faces, "edges": edges, "instances": instances, "guides": guides,
            "created": out.created.iter().map(|e| { let (k, id) = kind(e); json!({"kind": k, "id": id}) }).collect::<Vec<_>>(),
            "material": out.material.map(|m| m.0),
            "tag": out.tag.map(|t| t.0),
        }),
    }
}

fn ring(centre: DVec3, n: DVec3, radius: f64, sides: u32) -> Vec<DVec3> {
    let (u, v) = plane_axes(n);
    (0..sides)
        .map(|i| {
            let a = i as f64 / sides as f64 * std::f64::consts::TAU;
            centre + (u * a.cos() + v * a.sin()) * radius
        })
        .collect()
}

fn units(args: &Value) -> Result<Option<Units>, String> {
    match get(args, "units").and_then(Value::as_str) {
        None => Ok(None),
        Some("mm") => Ok(Some(Units::Millimetres)),
        Some("m") => Ok(Some(Units::Metres)),
        Some("cm") => Ok(Some(Units::Centimetres)),
        Some("in") => Ok(Some(Units::Inches)),
        Some("ft") => Ok(Some(Units::Feet)),
        Some("architectural") => Ok(Some(Units::Architectural)),
        Some("fractional") => Ok(Some(Units::Fractional)),
        Some(other) => Err(format!("unknown units \"{other}\"; use mm, cm, m, in, ft, architectural or fractional")),
    }
}

/// Calls a tool. `Ok(None)` means there is no such tool.
pub fn call(s: &mut Session, name: &str, args: &Value) -> R {
    if !args.is_object() {
        return Err("arguments must be an object".into());
    }
    match name {
        "new_document" => {
            let mut doc = rysmo_model::Document::new();
            if let Some(u) = units(args)? {
                doc.units = u;
            }
            s.editor.replace_document(doc);
            s.path = None;
            Ok(Some(ToolOutput { text: "New empty model.".into(), data: json!({}) }))
        }
        "open_document" => {
            let path = get(args, "path").and_then(Value::as_str).ok_or("`path` is missing")?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            let doc = rysmo_io::load(&text).map_err(|e| format!("cannot open {path}: {e}"))?;
            s.editor.replace_document(doc);
            s.path = Some(path.into());
            let summary = describe::summary(&s.editor);
            Ok(Some(ToolOutput { text: format!("Opened {path}. {}", summary.0), data: summary.1 }))
        }
        "save_document" => {
            let path = match get(args, "path").and_then(Value::as_str) {
                Some(p) => std::path::PathBuf::from(p),
                None => s.path.clone().ok_or("no path: give one, the model was never saved")?,
            };
            std::fs::write(&path, rysmo_io::save(s.editor.doc()))
                .map_err(|e| format!("cannot save {}: {e}", path.display()))?;
            s.editor.mark_saved();
            s.path = Some(path.clone());
            Ok(Some(ToolOutput {
                text: format!("Saved {}.", path.display()),
                data: json!({"path": path.display().to_string()}),
            }))
        }
        "export" => {
            let path = std::path::PathBuf::from(get(args, "path").and_then(Value::as_str).ok_or("`path` is missing")?);
            let format = get(args, "format")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| path.extension().map(|e| e.to_string_lossy().to_lowercase()))
                .ok_or("give `format` (obj, glb, stl or 3mf)")?;
            let mut notes = String::new();
            match format.as_str() {
                "obj" => {
                    let mtl = path.with_extension("mtl");
                    let name =
                        mtl.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "model.mtl".into());
                    let out = rysmo_io::obj::export(s.editor.doc(), &name);
                    let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                    std::fs::write(&path, out.obj)
                        .and_then(|_| std::fs::write(&mtl, out.mtl))
                        .and_then(|_| out.images.iter().try_for_each(|(n, b)| std::fs::write(dir.join(n), b)))
                        .map_err(|e| e.to_string())?;
                }
                "glb" => std::fs::write(&path, rysmo_io::gltf::export(s.editor.doc())).map_err(|e| e.to_string())?,
                "stl" | "3mf" => {
                    let check = rysmo_io::print::check(s.editor.doc());
                    if !check.is_printable() {
                        notes = format!(
                            " Not ready to print: {} open edges, {} bad edges, inside out: [{}].",
                            check.open_edges.len(),
                            check.bad_edges.len(),
                            check.inside_out.join(", ")
                        );
                    }
                    let bytes = if format == "stl" {
                        rysmo_io::print::stl(s.editor.doc())
                    } else {
                        rysmo_io::print::three_mf(s.editor.doc())?
                    };
                    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                }
                other => return Err(format!("unknown format \"{other}\"; use obj, glb, stl or 3mf")),
            }
            Ok(Some(ToolOutput {
                text: format!("Exported {}.{notes}", path.display()),
                data: json!({"path": path.display().to_string()}),
            }))
        }
        "describe_model" => {
            let limit = get(args, "limit").and_then(Value::as_u64).unwrap_or(500) as usize;
            let summary_only = get(args, "detail").and_then(Value::as_str) == Some("summary");
            let (text, data) =
                if summary_only { describe::summary(&s.editor) } else { describe::full(&s.editor, limit) };
            Ok(Some(ToolOutput { text, data }))
        }
        "get_document_text" => {
            let text = rysmo_io::save(s.editor.doc());
            Ok(Some(ToolOutput { text: text.clone(), data: json!({"text": text}) }))
        }
        "draw_line" => {
            let pts = get(args, "points").and_then(Value::as_array).ok_or("`points` must be a list of [x, y, z]")?;
            let pts: Vec<DVec3> = pts.iter().map(|p| vec3(p, "points")).collect::<Result<_, _>>()?;
            if pts.len() < 2 {
                return Err("give at least two points".into());
            }
            let mut lines: Vec<[DVec3; 2]> = pts.windows(2).map(|w| [w[0], w[1]]).collect();
            if get(args, "closed").and_then(Value::as_bool).unwrap_or(false) && pts.len() > 2 {
                lines.push([pts[pts.len() - 1], pts[0]]);
            }
            run(s, Command::DrawLines { lines, facing: None })
        }
        "draw_rectangle" => {
            let corner = arg_vec3(args, "corner")?;
            let (u, v) = match (opt_vec3(args, "u")?, opt_vec3(args, "v")?) {
                (Some(u), Some(v)) => (u, v),
                _ => {
                    let n = plane_normal(s, args)?;
                    let (a, b) = plane_axes(n);
                    (a * num(args, "width")?, b * num(args, "depth")?)
                }
            };
            let facing = get(args, "face").is_some().then(|| plane_normal(s, args)).transpose()?;
            run(s, Command::DrawRectangle { corner, u, v, facing })
        }
        "draw_circle" => {
            let centre = arg_vec3(args, "centre")?;
            let normal = plane_normal(s, args)?;
            let segments = get(args, "segments").and_then(Value::as_u64).unwrap_or(24) as u32;
            run(s, Command::DrawCircle { centre, normal, radius: num(args, "radius")?, segments, start: None })
        }
        "draw_polygon" => {
            let centre = arg_vec3(args, "centre")?;
            let n = plane_normal(s, args)?;
            let sides = int(args, "sides")? as u32;
            if sides < 3 {
                return Err("a polygon needs at least 3 sides".into());
            }
            let pts = ring(centre, n, num(args, "radius")?, sides);
            let lines = (0..pts.len()).map(|i| [pts[i], pts[(i + 1) % pts.len()]]).collect();
            let mut out = run(s, Command::DrawLines { lines, facing: Some(n) })?;
            if let Some(o) = &mut out {
                o.text = o.text.replacen("Line done.", "Polygon done.", 1);
            }
            Ok(out)
        }
        "draw_arc" => {
            let normal = plane_normal(s, args)?;
            let segments = get(args, "segments").and_then(Value::as_u64).unwrap_or(12) as u32;
            run(
                s,
                Command::DrawArc {
                    start: arg_vec3(args, "start")?,
                    end: arg_vec3(args, "end")?,
                    normal,
                    bulge: num(args, "bulge")?,
                    segments,
                },
            )
        }
        "push_pull" => {
            let f = face(s, int(args, "face")?)?;
            let keep_base = get(args, "keep_base").and_then(Value::as_bool).unwrap_or(false);
            run(s, Command::PushPull { face: f, distance: num(args, "distance")?, keep_base })
        }
        "move" => {
            let el = elements(s, args, "elements")?;
            let offset = arg_vec3(args, "offset")?;
            match get(args, "copies").and_then(Value::as_u64).unwrap_or(0) {
                0 => run(s, Command::Move { elements: el, offset }),
                n => run(s, Command::Copy { elements: el, offset, copies: n as u32 }),
            }
        }
        "rotate" => {
            let el = elements(s, args, "elements")?;
            let axis = opt_vec3(args, "axis")?.unwrap_or(DVec3::Z);
            let copies = get(args, "copies").and_then(Value::as_u64).unwrap_or(0) as u32;
            run(
                s,
                Command::Rotate {
                    elements: el,
                    centre: arg_vec3(args, "centre")?,
                    axis,
                    angle: num(args, "angle")?,
                    copies,
                },
            )
        }
        "scale" => {
            let el = elements(s, args, "elements")?;
            let f = get(args, "factor").ok_or("`factor` is missing")?;
            let factors = match f.as_f64() {
                Some(k) => DVec3::splat(k),
                None => vec3(f, "factor")?,
            };
            run(s, Command::Scale { elements: el, origin: arg_vec3(args, "origin")?, factors })
        }
        "offset" => {
            let distance = num(args, "distance")?;
            if let Some(f) = get(args, "face") {
                let f = face(s, f.as_u64().ok_or("`face` must be an id")?)?;
                run(s, Command::OffsetFace { face: f, distance })
            } else {
                let chain = edges(s, args, "edges")?;
                let normal = opt_vec3(args, "normal")?.unwrap_or(DVec3::Z);
                run(s, Command::OffsetEdges { edges: chain, normal, distance })
            }
        }
        "follow_me" => {
            let f = face(s, int(args, "face")?)?;
            let path = edges(s, args, "path")?;
            run(s, Command::FollowMe { face: f, path })
        }
        "erase" => {
            let el = elements(s, args, "elements")?;
            run(s, Command::Erase { elements: el })
        }
        "reverse_faces" => {
            let faces = id_list(args, "faces")?.into_iter().map(|id| face(s, id)).collect::<Result<_, _>>()?;
            run(s, Command::ReverseFaces { faces })
        }
        "soften_edges" => {
            let e = edges(s, args, "edges")?;
            run(s, Command::SoftenEdges { edges: e, angle: opt_num(args, "angle")?.unwrap_or(20.0) })
        }
        "make_group" => {
            let el = elements(s, args, "elements")?;
            run(s, Command::MakeGroup { elements: el })
        }
        "make_component" => {
            let el = elements(s, args, "elements")?;
            let name = get(args, "name").and_then(Value::as_str).map(str::to_string);
            let mut out = run(s, Command::MakeComponent { elements: el, name })?;
            // Say which definition it made, for place_component.
            if let Some(o) = &mut out
                && let Some(i) = o.data["instances"].get(0).and_then(Value::as_u64)
                && let Some(inst) = s.editor.active().instances.get(&InstanceId(i))
            {
                o.text.push_str(&format!(" Definition {}.", inst.definition));
                o.data["definition"] = json!(inst.definition.0);
            }
            Ok(out)
        }
        "place_component" => {
            let definition = DefinitionId(int(args, "definition")?);
            let rotation = opt_num(args, "rotation")?.unwrap_or(0.0);
            let scale = opt_num(args, "scale")?.unwrap_or(1.0);
            run(s, Command::PlaceInstance { definition, origin: arg_vec3(args, "origin")?, rotation, scale })
        }
        "explode" => {
            let i = instance(s, int(args, "instance")?)?;
            run(s, Command::Explode { instance: i })
        }
        "make_unique" => {
            let i = instance(s, int(args, "instance")?)?;
            run(s, Command::MakeUnique { instance: i })
        }
        "edit_component" => {
            let i = instance(s, int(args, "instance")?)?;
            s.editor.enter(i).map_err(|e| e.to_string())?;
            let (text, data) = describe::summary(&s.editor);
            Ok(Some(ToolOutput {
                text: format!("Editing {i}. Tools now work inside it, in its coordinates. {text}"),
                data,
            }))
        }
        "close_component" => {
            if !s.editor.exit() {
                return Err("nothing is open: already editing the model itself".into());
            }
            Ok(Some(ToolOutput {
                text: "Closed.".into(),
                data: json!({"context": s.editor.context().iter().map(|i| i.0).collect::<Vec<_>>()}),
            }))
        }
        "add_material" => {
            let name = get(args, "name").and_then(Value::as_str).ok_or("`name` is missing")?.to_string();
            let c = get(args, "color").and_then(Value::as_array).ok_or("`color` must be [r, g, b] or [r, g, b, a]")?;
            let c: Vec<f32> = c
                .iter()
                .map(|x| x.as_f64().map(|f| f.clamp(0.0, 1.0) as f32))
                .collect::<Option<_>>()
                .ok_or("`color` must hold numbers")?;
            if !(3..=4).contains(&c.len()) {
                return Err("`color` must be [r, g, b] or [r, g, b, a]".into());
            }
            run(
                s,
                Command::AddMaterial {
                    name,
                    color: [c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0)],
                    texture: None,
                },
            )
        }
        "paint" => {
            let el = elements(s, args, "elements")?;
            let m = material(s, args.get("material").filter(|v| !v.is_null()))?;
            let back = get(args, "back").and_then(Value::as_bool).unwrap_or(false);
            let faces: Vec<FaceId> =
                el.iter().filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).collect();
            let insts: Vec<InstanceId> =
                el.iter().filter_map(|e| if let Element::Instance(i) = e { Some(*i) } else { None }).collect();
            if faces.is_empty() && insts.is_empty() {
                return Err("paint faces or groups/components".into());
            }
            let mut last = None;
            if !faces.is_empty() {
                last = run(s, Command::Paint { faces, material: m, back })?;
            }
            if !insts.is_empty() {
                last = run(s, Command::PaintInstances { instances: insts, material: m })?;
            }
            Ok(last)
        }
        "add_tag" => {
            let name = get(args, "name").and_then(Value::as_str).ok_or("`name` is missing")?.to_string();
            run(s, Command::AddTag { name })
        }
        "set_tag" => {
            let t = tag(s, get(args, "tag"))?.ok_or("`tag` is missing")?;
            let name = get(args, "name").and_then(Value::as_str).map(str::to_string);
            let visible = get(args, "visible").and_then(Value::as_bool);
            run(s, Command::SetTag { tag: t, name, visible })
        }
        "assign_tag" => {
            let instances =
                id_list(args, "instances")?.into_iter().map(|id| instance(s, id)).collect::<Result<_, _>>()?;
            let t = tag(s, args.get("tag").filter(|v| !v.is_null()))?;
            run(s, Command::AssignTag { instances, tag: t })
        }
        "add_guide" => match (opt_vec3(args, "point")?, opt_vec3(args, "origin")?, opt_vec3(args, "dir")?) {
            (Some(p), _, _) => run(s, Command::AddGuidePoint { point: p }),
            (None, Some(origin), Some(dir)) => run(s, Command::AddGuideLine { origin, dir }),
            _ => Err("give `point`, or `origin` and `dir`".into()),
        },
        "set_units" => {
            let u = units(args)?.ok_or("`units` is missing")?;
            run(s, Command::SetUnits { units: u })
        }
        "undo" => {
            let label = s.editor.undo_label().map(str::to_string);
            if !s.editor.undo() {
                return Err("nothing to undo".into());
            }
            Ok(Some(ToolOutput {
                text: format!("Undid {}.", label.clone().unwrap_or_default()),
                data: json!({"undone": label}),
            }))
        }
        "redo" => {
            let label = s.editor.redo_label().map(str::to_string);
            if !s.editor.redo() {
                return Err("nothing to redo".into());
            }
            Ok(Some(ToolOutput {
                text: format!("Redid {}.", label.clone().unwrap_or_default()),
                data: json!({"redone": label}),
            }))
        }
        _ => Ok(None),
    }
}

/// A length in the document's display units, for text replies.
pub(crate) fn len(s: &rysmo_model::Editor, m: f64) -> String {
    s.doc().format_length(m)
}
