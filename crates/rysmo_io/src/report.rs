//! Bounded world-space quantities and deterministic spreadsheet-safe CSV.
use rysmo_math::DMat4;
use rysmo_model::{DefinitionId, Document, InstanceId, Mesh};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone)]
pub struct Options {
    pub visible_only: bool,
    pub summaries: bool,
    pub group_attribute: Option<String>,
}
impl Default for Options {
    fn default() -> Self {
        Self { visible_only: true, summaries: true, group_attribute: None }
    }
}
#[derive(Clone, Default)]
pub struct Row {
    pub kind: String,
    pub path: String,
    pub name: String,
    pub definition: String,
    pub count: usize,
    pub edges: usize,
    pub faces: usize,
    pub length: f64,
    pub area: f64,
    pub volume: Option<f64>,
    pub note: String,
    pub attributes: rysmo_model::component_data::Attributes,
}
pub struct Report {
    pub rows: Vec<Row>,
}
struct Builder<'a> {
    doc: &'a Document,
    options: Options,
    rows: Vec<Row>,
    path: BTreeSet<DefinitionId>,
    remaining: usize,
    work: usize,
    meshes: BTreeMap<Option<DefinitionId>, Mesh>,
    volumes: BTreeMap<Option<DefinitionId>, Result<f64, &'static str>>,
}
impl Builder<'_> {
    fn visit(
        &mut self,
        owner: Option<DefinitionId>,
        transform: DMat4,
        path: Vec<InstanceId>,
        name: String,
        attributes: rysmo_model::component_data::Attributes,
    ) -> Result<(), String> {
        rysmo_model::component_data::validate_attributes(&attributes).map_err(|error| error.to_string())?;
        self.remaining = self.remaining.checked_sub(1).ok_or("Report exceeds 100000 occurrences")?;
        if path.len() > 32 || !transform.is_finite() || !transform.inverse().is_finite() {
            return Err("Report hierarchy is too deep or contains invalid transforms".into());
        }
        let def = self.doc.def(owner).ok_or("Missing report definition")?;
        if !self.meshes.contains_key(&owner) {
            let mesh = self.doc.effective_mesh(owner).into_owned();
            if mesh.vertex_count().saturating_add(mesh.edge_count()).saturating_add(mesh.face_count()) > 1_000_000 {
                return Err("Report mesh exceeds the geometry budget".into());
            }
            self.meshes.insert(owner, mesh);
        }
        let mesh = &self.meshes[&owner];
        let budget = mesh.vertex_count().saturating_add(mesh.edge_count()).saturating_add(mesh.face_count());
        self.work = self.work.checked_sub(budget).ok_or("Report exceeds 2000000 evaluated geometry elements")?;
        let mut row = Row {
            kind: if owner.is_none() {
                "root"
            } else if def.group {
                "group"
            } else {
                "component"
            }
            .into(),
            path: if path.is_empty() {
                "/".into()
            } else {
                format!("/{}", path.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join("/"))
            },
            name,
            attributes,
            definition: owner.map(|id| id.0.to_string()).unwrap_or_default(),
            count: 1,
            ..Default::default()
        };
        let mut excluded = false;
        for (id, edge) in mesh.edges() {
            if self.options.visible_only && edge.hidden {
                excluded = true;
                continue;
            }
            let (a, b) = mesh.edge_points(id);
            let length = transform.transform_vector3(b - a).length();
            if !length.is_finite() {
                return Err("Non-finite report edge length".into());
            }
            row.edges += 1;
            row.length += length;
        }
        for (id, face) in mesh.faces() {
            if self.options.visible_only && face.hidden {
                excluded = true;
                continue;
            }
            let corners = face.loops().map(|outline| outline.len()).sum::<usize>();
            self.work = self.work.checked_sub(corners).ok_or("Report face exceeds the geometry budget")?;
            if corners > 16384 {
                return Err("A report face exceeds 16384 corners".into());
            }
            let triangles = rysmo_mesh::triangulate_face(mesh, id);
            if triangles.is_empty() {
                return Err("Cannot triangulate a report face".into());
            }
            row.faces += 1;
            for triangle in triangles {
                let [a, b, c] = triangle.map(|id| mesh.pos(id));
                let area = transform.transform_vector3(b - a).cross(transform.transform_vector3(c - a)).length() * 0.5;
                if !area.is_finite() {
                    return Err("Non-finite report face area".into());
                }
                row.area += area;
            }
        }
        if row.edges == 0 && row.faces == 0 {
            row.note = "No own geometry; nested quantities are listed separately".into();
        } else if excluded {
            row.note = "Volume unavailable: hidden geometry excluded".into();
        } else {
            let volume = *self.volumes.entry(owner).or_insert_with(|| rysmo_model::mesh_volume(mesh));
            match volume {
                Ok(volume) => {
                    let volume = volume * transform.determinant().abs();
                    if !volume.is_finite() {
                        return Err("Non-finite report volume".into());
                    }
                    row.volume = Some(volume);
                }
                Err(reason) => row.note = format!("Volume unavailable: {reason}"),
            }
        }
        self.rows.push(row);
        for (id, instance) in &def.instances {
            if self.options.visible_only && (instance.hidden || !self.doc.tag_visible(instance.tag)) {
                continue;
            }
            if !self.path.insert(instance.definition) {
                return Err("Cyclic report hierarchy".into());
            }
            let mut child = path.clone();
            child.push(*id);
            let name = if instance.name.is_empty() {
                self.doc.definitions.get(&instance.definition).map(|def| def.name.clone()).unwrap_or_default()
            } else {
                instance.name.clone()
            };
            self.visit(
                Some(instance.definition),
                transform * instance.transform,
                child,
                name,
                rysmo_model::component_data::attributes(self.doc, instance),
            )?;
            self.path.remove(&instance.definition);
        }
        Ok(())
    }
}
pub fn build(doc: &Document, options: Options) -> Result<Report, String> {
    let mut builder = Builder {
        doc,
        options: options.clone(),
        rows: Vec::new(),
        path: BTreeSet::new(),
        remaining: 100_000,
        work: 2_000_000,
        meshes: BTreeMap::new(),
        volumes: BTreeMap::new(),
    };
    builder.visit(None, DMat4::IDENTITY, Vec::new(), "Model".into(), Default::default())?;
    let mut summaries: BTreeMap<String, Row> = BTreeMap::new();
    let mut total = Row {
        kind: "total".into(),
        name: "All occurrence geometry (without summary rows)".into(),
        volume: Some(0.),
        ..Default::default()
    };
    for row in &builder.rows {
        add(&mut total, row);
        if row.kind == "component" && options.summaries {
            let summary = summaries.entry(row.definition.clone()).or_insert_with(|| Row {
                kind: "component_summary".into(),
                name: doc
                    .definitions
                    .get(&DefinitionId(row.definition.parse().expect("internally generated definition ID")))
                    .map(|def| def.name.clone())
                    .unwrap_or_else(|| row.name.clone()),
                definition: row.definition.clone(),
                volume: Some(0.),
                ..Default::default()
            });
            add(summary, row);
        }
    }
    builder.rows.extend(summaries.into_values());
    if let Some(key) = &options.group_attribute {
        let mut groups: BTreeMap<String, Row> = BTreeMap::new();
        for row in builder.rows.iter().filter(|r| matches!(r.kind.as_str(), "component" | "group")) {
            let label = row.attributes.get(key).map(attribute_text).unwrap_or_else(|| "(unset)".into());
            let identity = row
                .attributes
                .get(key)
                .map(|attribute| serde_json::to_string(attribute).expect("validated report attribute"))
                .unwrap_or_default();
            let group = groups.entry(identity).or_insert_with(|| Row {
                kind: "attribute_summary".into(),
                name: format!("{key}: {label}"),
                volume: Some(0.),
                ..Default::default()
            });
            add(group, row);
        }
        builder.rows.extend(groups.into_values());
    }
    builder.rows.push(total);
    if builder.rows.iter().any(|row| {
        !row.length.is_finite()
            || !row.area.is_finite()
            || row.volume.is_some_and(|v| !v.is_finite())
            || row
                .attributes
                .values()
                .any(|a| matches!(a.value,rysmo_model::component_data::Value::Number(n) if !n.is_finite()))
    }) {
        return Err("Non-finite report totals".into());
    }
    Ok(Report { rows: builder.rows })
}
fn add(target: &mut Row, row: &Row) {
    use rysmo_model::component_data::{Attribute, Value};
    for (key, value) in &row.attributes {
        use std::collections::btree_map::Entry;
        match target.attributes.entry(key.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(value.clone());
            }
            Entry::Occupied(mut entry) => {
                let previous = entry.get_mut();
                let equal = previous == value;
                match (&mut previous.value, &value.value) {
                    (Value::Number(total), Value::Number(n)) if previous.unit == value.unit => {
                        let sum = *total + n;
                        if sum.is_finite() {
                            *total = sum;
                        } else {
                            previous.value = Value::Text("Total exceeds numeric range".into());
                            previous.unit.clear();
                        }
                    }
                    _ if equal => {}
                    _ => {
                        *previous = Attribute {
                            value: Value::Text("Mixed values or units; total unavailable".into()),
                            unit: String::new(),
                        };
                    }
                }
            }
        }
    }
    target.count += usize::from(row.kind != "root") * row.count;
    target.edges += row.edges;
    target.faces += row.faces;
    target.length += row.length;
    target.area += row.area;
    if row.edges > 0 || row.faces > 0 {
        target.volume = target.volume.zip(row.volume).map(|(a, b)| a + b);
    }
    if target.volume.is_none() {
        target.note = "Some own geometry has no available volume; nested assemblies are not double-counted".into();
    }
}
fn attribute_text(attribute: &rysmo_model::component_data::Attribute) -> String {
    use rysmo_model::component_data::Value;
    match &attribute.value {
        Value::Text(text) => text.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(v) => v.to_string(),
    }
}
fn cell(text: &str) -> String {
    let start = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let text = if start.starts_with(['=', '+', '-', '@']) { format!("'{text}") } else { text.into() };
    format!("\"{}\"", text.replace('"', "\"\""))
}
impl Report {
    pub fn csv(&self) -> String {
        let mut csv = String::from(
            "kind,path,name,definition_id,occurrence_count,own_edge_count,own_face_count,own_edge_length_m,own_face_area_m2,own_volume_m3,note\r\n",
        );
        let keys: BTreeSet<_> = self.rows.iter().flat_map(|row| row.attributes.keys().cloned()).collect();
        if !keys.is_empty() {
            csv.truncate(csv.len() - 2);
            for key in &keys {
                csv.push(',');
                csv.push_str(&cell(&format!("attribute:{key}")));
                csv.push(',');
                csv.push_str(&cell(&format!("unit:{key}")));
            }
            csv.push_str("\r\n");
        }
        for row in &self.rows {
            let volume = row.volume.map(|v| format!("{v:.9}")).unwrap_or_default();
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{:.9},{:.9},{},{}\r\n",
                cell(&row.kind),
                cell(&row.path),
                cell(&row.name),
                cell(&row.definition),
                row.count,
                row.edges,
                row.faces,
                row.length,
                row.area,
                volume,
                cell(&row.note)
            ));
            if !keys.is_empty() {
                csv.truncate(csv.len() - 2);
                for key in &keys {
                    csv.push(',');
                    match row.attributes.get(key) {
                        Some(attribute) if matches!(attribute.value, rysmo_model::component_data::Value::Number(_)) => {
                            csv.push_str(&attribute_text(attribute))
                        }
                        value => csv.push_str(&cell(&value.map(attribute_text).unwrap_or_default())),
                    }
                    csv.push(',');
                    csv.push_str(&cell(row.attributes.get(key).map(|a| a.unit.as_str()).unwrap_or("")));
                }
                csv.push_str("\r\n");
            }
        }
        csv
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor, Element};
    fn cube() -> Editor {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let distance = e.mesh().face_normal(face).unwrap().z;
        e.apply(Command::PushPull { face, distance, keep_base: false }).unwrap();
        e
    }
    #[test]
    fn cube_quantities_and_scaled_occurrences_do_not_double_count_summaries() {
        let mut e = cube();
        let elements = e
            .mesh()
            .faces()
            .map(|(id, _)| Element::Face(id))
            .chain(e.mesh().edges().map(|(id, _)| Element::Edge(id)))
            .collect();
        let made = e.apply(Command::MakeComponent { elements, name: Some("Box".into()) }).unwrap();
        let Element::Instance(id) = made.created[0] else { panic!() };
        let mut doc = e.doc().clone();
        let mut copy = doc.root.instances[&id].clone();
        copy.transform = DMat4::from_scale(DVec3::new(-2., 3., 4.));
        let copy_id = InstanceId(doc.reserve_id());
        doc.root_mut().instances.insert(copy_id, copy);
        let before = doc.clone();
        let report = build(&doc, Options::default()).unwrap();
        assert_eq!(doc, before);
        let summary = report.rows.iter().find(|r| r.kind == "component_summary").unwrap();
        assert_eq!(summary.count, 2);
        assert!((summary.volume.unwrap() - 25.).abs() < 1e-8);
        assert!((summary.area - 58.).abs() < 1e-8);
        assert!((summary.length - 48.).abs() < 1e-8);
        let total = report.rows.last().unwrap();
        assert_eq!(total.volume, summary.volume);
        assert_eq!(total.area, summary.area);
    }
    #[test]
    fn native_roundtrip_retains_point_edits_and_csv_is_stable() {
        let mut e = Editor::new();
        let out = e
            .apply(Command::DrawLines {
                lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::X, DVec3::new(2., 1., 0.)]],
                facing: None,
            })
            .unwrap();
        let edges =
            out.created.into_iter().filter_map(|e| if let Element::Edge(id) = e { Some(id) } else { None }).collect();
        e.apply(Command::WeldEdges { edges }).unwrap();
        let curve = e.mesh().curves().next().unwrap().0;
        let vertex = rysmo_model::curve_points::vertices(e.mesh(), curve).unwrap()[1];
        e.apply(Command::SetCurvePoint { curve, vertex, position: DVec3::new(1., 2., 3.) }).unwrap();
        let loaded = crate::load(&crate::save(e.doc())).unwrap();
        assert_eq!(loaded, e.doc().clone());
        assert_eq!(
            build(&loaded, Options::default()).unwrap().csv(),
            build(e.doc(), Options::default()).unwrap().csv()
        );
    }
    #[test]
    fn quoting_unicode_and_spreadsheet_formulas_are_safe() {
        let report =
            Report { rows: vec![Row { name: "  =HYPERLINK(\"bad\")\nŽluťoučký".into(), ..Default::default() }] };
        let csv = report.csv();
        assert!(csv.contains("\"'  =HYPERLINK(\"\"bad\"\")\nŽluťoučký\""));
        assert!(csv.ends_with("\r\n"));
        for text in ["=1+1", "+1", "-1", "@SUM(1)", "\t=1", "\u{feff}=1"] {
            assert!(cell(text).starts_with("\"'"));
        }
    }
    #[test]
    fn hidden_faces_and_open_mesh_have_explicitly_unavailable_volumes() {
        let mut e = cube();
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::SetGeometryHidden { elements: vec![Element::Face(face)], hidden: true }).unwrap();
        let report = build(e.doc(), Options::default()).unwrap();
        assert!(report.rows[0].volume.is_none());
        assert!(report.rows[0].note.contains("hidden"));
        assert!(
            (build(e.doc(), Options { visible_only: false, summaries: true, group_attribute: None }).unwrap().rows[0]
                .volume
                .unwrap()
                - 1.)
                .abs()
                < 1e-8
        );
    }
    #[test]
    fn cyclic_hierarchy_is_rejected() {
        let mut e = cube();
        let elements = e
            .mesh()
            .faces()
            .map(|(id, _)| Element::Face(id))
            .chain(e.mesh().edges().map(|(id, _)| Element::Edge(id)))
            .collect();
        let made = e.apply(Command::MakeComponent { elements, name: None }).unwrap();
        let Element::Instance(id) = made.created[0] else { panic!() };
        let mut doc = e.doc().clone();
        let instance = doc.root.instances[&id].clone();
        let child = InstanceId(doc.reserve_id());
        std::sync::Arc::make_mut(doc.definitions.get_mut(&instance.definition).unwrap())
            .instances
            .insert(child, instance);
        assert!(build(&doc, Options::default()).err().unwrap().contains("Cyclic"));
    }
    #[test]
    fn typed_attributes_override_defaults_and_totals_reject_mixed_units() {
        use rysmo_model::component_data::{Attribute, Value};
        let doc = rysmo_model::parametric::Spec::default().document().unwrap();
        let mut doc = doc;
        let id = *doc.root.instances.keys().next().unwrap();
        let definition = doc.root.instances[&id].definition;
        let value = |n, unit: &str| Attribute { value: Value::Number(n), unit: unit.into() };
        doc.def_mut(Some(definition)).unwrap().component.attributes.insert("Cost".into(), value(10., "EUR"));
        doc.root_mut().instances.get_mut(&id).unwrap().attributes.insert("Cost".into(), value(-3., "EUR"));
        let second = rysmo_model::InstanceId(doc.reserve_id());
        let mut copy = doc.root.instances[&id].clone();
        copy.attributes.clear();
        doc.root_mut().instances.insert(second, copy);
        let options = Options { group_attribute: Some("Cost".into()), ..Default::default() };
        let report = build(&doc, options.clone()).unwrap();
        let total = report.rows.iter().find(|r| r.kind == "total").unwrap();
        assert_eq!(total.attributes["Cost"], value(7., "EUR"));
        assert!(report.csv().contains(",-3,\"EUR\""));
        assert_eq!(report.rows.iter().filter(|r| r.kind == "attribute_summary").count(), 2);
        doc.root_mut().instances.get_mut(&second).unwrap().attributes.insert("Cost".into(), value(5., "USD"));
        let report = build(&doc, options).unwrap();
        let total = report.rows.iter().find(|r| r.kind == "total").unwrap();
        assert!(matches!(&total.attributes["Cost"].value,Value::Text(text) if text.contains("unavailable")));
    }
}
