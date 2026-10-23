//! 3D printing: a printability check and STL and 3MF export.
//!
//! Both formats get the visible faces as triangles in millimetres, Z up, with groups
//! and components flattened; 3MF keeps each group or component as its own object.
//! Loose lines, materials and textures are left out.
use rysmo_math::DVec3;
use rysmo_model::Document;
use std::collections::BTreeMap;
use std::fmt::Write;

use crate::exchange::{Geometry, collect};

/// Model metres to print millimetres.
const MM: f64 = 1000.;

/// What stands between the model and a clean print.
#[derive(Clone, Debug, Default, PartialEq)]
#[must_use]
pub struct PrintCheck {
    /// Edges with a face on one side only: holes in the surface.
    pub open_edges: Vec<[DVec3; 2]>,
    /// Edges shared by more than two faces, or by two faces facing opposite ways.
    pub bad_edges: Vec<[DVec3; 2]>,
    /// Groups, components or the model whose faces all face inwards.
    pub inside_out: Vec<String>,
    /// Loose lines, which are not printed.
    pub loose_lines: usize,
    pub triangles: usize,
    /// The size of the box around everything, in millimetres.
    pub size_mm: DVec3,
    /// The enclosed volume in cubic millimetres; only meaningful for closed objects.
    pub volume_mm3: f64,
}

impl PrintCheck {
    /// Closed, consistently facing objects, ready for a slicer.
    pub fn is_printable(&self) -> bool {
        self.triangles > 0 && self.open_edges.is_empty() && self.bad_edges.is_empty() && self.inside_out.is_empty()
    }
}

/// Positions are welded on a 0.1 µm grid, finer than the modelling tolerance.
fn key(p: DVec3) -> [i64; 3] {
    (p * 1e7).round().to_array().map(|c| c as i64)
}

/// Union-find over welded positions.
#[derive(Default)]
struct Parts(BTreeMap<[i64; 3], [i64; 3]>);

impl Parts {
    fn root(&mut self, mut p: [i64; 3]) -> [i64; 3] {
        while let Some(&up) = self.0.get(&p).filter(|up| **up != p) {
            let grand = self.0.get(&up).copied().unwrap_or(up);
            self.0.insert(p, grand);
            p = up;
        }
        p
    }
    fn join(&mut self, a: [i64; 3], b: [i64; 3]) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            self.0.insert(a, b);
        }
    }
}

pub fn check(doc: &Document) -> PrintCheck {
    check_geometries(&collect(doc))
}

fn check_geometries(geometries: &[Geometry]) -> PrintCheck {
    let mut report = PrintCheck::default();
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for geometry in geometries {
        report.loose_lines += geometry.lines.len();
        report.triangles += geometry.triangles.len();
        let mut directed: BTreeMap<([i64; 3], [i64; 3]), usize> = BTreeMap::new();
        let mut positions = BTreeMap::new();
        // Each connected part must enclose a positive volume on its own.
        let mut parts = Parts::default();
        for triangle in &geometry.triangles {
            let [a, b, c] = triangle.points.map(key);
            parts.join(a, b);
            parts.join(a, c);
        }
        let mut volumes: BTreeMap<[i64; 3], (f64, usize)> = BTreeMap::new();
        for triangle in &geometry.triangles {
            let [a, b, c] = triangle.points;
            let part = volumes.entry(parts.root(key(a))).or_default();
            part.0 += a.dot(b.cross(c)) / 6.;
            part.1 += 1;
            for i in 0..3 {
                let (p, q) = (triangle.points[i], triangle.points[(i + 1) % 3]);
                lo = lo.min(p);
                hi = hi.max(p);
                positions.insert(key(p), p);
                *directed.entry((key(p), key(q))).or_default() += 1;
            }
        }
        for (&(a, b), &forward) in &directed {
            let backward = directed.get(&(b, a)).copied().unwrap_or(0);
            if (forward, backward) == (1, 1) || (backward > 0 && a > b) {
                continue;
            }
            let segment = [positions[&a], positions[&b]];
            if forward + backward == 1 {
                report.open_edges.push(segment);
            } else {
                report.bad_edges.push(segment);
            }
        }
        if volumes.values().any(|&(volume, count)| volume < 0. && count >= 4) {
            report.inside_out.push(geometry.name.clone());
        }
        report.volume_mm3 += volumes.values().map(|v| v.0.abs()).sum::<f64>() * MM * MM * MM;
    }
    if report.triangles > 0 {
        report.size_mm = (hi - lo) * MM;
    }
    report
}

/// Binary STL in millimetres.
pub fn stl(doc: &Document) -> Vec<u8> {
    let triangles: Vec<_> = collect(doc).into_iter().flat_map(|g| g.triangles).collect();
    let mut out = Vec::with_capacity(84 + triangles.len() * 50);
    let mut header = [b' '; 80];
    header[..22].copy_from_slice(b"Rysmo STL, millimetres");
    out.extend_from_slice(&header);
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for triangle in &triangles {
        for p in std::iter::once(triangle.normal).chain(triangle.points.map(|p| p * MM)) {
            for c in p.to_array() {
                out.extend_from_slice(&(c as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A 3MF package in millimetres, one object per group, component or the model itself.
pub fn three_mf(doc: &Document) -> Result<Vec<u8>, String> {
    let mut model = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n\
         <metadata name=\"Application\">Rysmo</metadata>\n<resources>\n",
    );
    let mut ids = Vec::new();
    for geometry in collect(doc).into_iter().filter(|g| !g.triangles.is_empty()) {
        let id = ids.len() + 1;
        ids.push(id);
        let mut index = BTreeMap::new();
        let mut vertices = String::new();
        let mut triangles = String::new();
        for triangle in &geometry.triangles {
            let v = triangle.points.map(|p| {
                let next = index.len();
                *index.entry(key(p)).or_insert_with(|| {
                    let p = p * MM;
                    let _ = writeln!(vertices, "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", p.x, p.y, p.z);
                    next
                })
            });
            if v[0] != v[1] && v[1] != v[2] && v[2] != v[0] {
                let _ = writeln!(triangles, "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", v[0], v[1], v[2]);
            }
        }
        let _ = write!(
            model,
            "<object id=\"{id}\" name=\"{}\" type=\"model\">\n<mesh>\n<vertices>\n{vertices}</vertices>\n\
             <triangles>\n{triangles}</triangles>\n</mesh>\n</object>\n",
            escape(&geometry.name)
        );
    }
    if ids.is_empty() {
        return Err("There are no faces to export.".into());
    }
    model.push_str("</resources>\n<build>\n");
    for id in ids {
        let _ = writeln!(model, "<item objectid=\"{id}\"/>");
    }
    model.push_str("</build>\n</model>\n");
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n\
        <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n\
        <Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n\
        </Types>\n";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n\
        <Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" \
        Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>\n\
        </Relationships>\n";
    crate::archive::zip_tree(&[
        ("[Content_Types].xml".into(), types.as_bytes().to_vec()),
        ("_rels/.rels".into(), rels.as_bytes().to_vec()),
        ("3D/3dmodel.model".into(), model.into_bytes()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::{Command, Editor};

    fn box_model() -> Editor {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 0.02, v: DVec3::Y * 0.01, facing: None })
            .unwrap();
        let face = editor.doc().root.mesh.faces().next().unwrap().0;
        editor.apply(Command::PushPull { face, distance: 0.005, keep_base: false }).unwrap();
        editor
    }

    #[test]
    fn a_pulled_box_is_printable_with_its_size_and_volume_in_millimetres() {
        let report = check(box_model().doc());
        assert!(report.is_printable(), "{report:?}");
        assert_eq!(report.triangles, 12);
        assert!((report.size_mm - DVec3::new(20., 10., 5.)).length() < 1e-9);
        assert!((report.volume_mm3 - 1000.).abs() < 1e-6);
    }

    #[test]
    fn a_missing_face_leaves_open_edges_and_a_reversed_one_bad_edges() {
        let editor = box_model();
        let mut open = editor.doc().clone();
        let top = open.root.mesh.faces().max_by(|a, b| {
            let z = |id| open.root.mesh.face_normal(id).unwrap().z;
            z(a.0).total_cmp(&z(b.0))
        });
        let top = top.unwrap().0;
        let mut reversed = open.clone();
        open.root_mut().mesh.remove_face(top);
        let report = check(&open);
        assert!(!report.is_printable());
        assert_eq!(report.open_edges.len(), 4);
        assert!(report.bad_edges.is_empty());
        reversed.root_mut().mesh.reverse_faces(&[top]);
        let report = check(&reversed);
        assert!(report.open_edges.is_empty());
        assert_eq!(report.bad_edges.len(), 4);
    }

    #[test]
    fn a_flat_rectangle_and_loose_lines_are_reported() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::splat(5.), DVec3::splat(6.)]], facing: None }).unwrap();
        let report = check(editor.doc());
        assert_eq!(report.open_edges.len(), 4);
        assert_eq!(report.loose_lines, 1);
        assert!(!report.is_printable());
        assert!(!check(Editor::new().doc()).is_printable());
    }

    #[test]
    fn one_box_turned_inside_out_is_found_next_to_a_good_one() {
        let mut editor = box_model();
        let doc = editor.doc().clone();
        let faces: Vec<_> = doc.root.mesh.faces().map(|(id, _)| id).collect();
        editor
            .apply(Command::Copy {
                elements: faces.iter().map(|f| rysmo_model::Element::Face(*f)).collect(),
                offset: DVec3::X * 0.05,
                copies: 1,
            })
            .unwrap();
        let mut doc = editor.doc().clone();
        assert_eq!(check(&doc).triangles, 24);
        doc.root_mut().mesh.reverse_faces(&faces);
        let report = check(&doc);
        assert!(report.open_edges.is_empty() && report.bad_edges.is_empty(), "{report:?}");
        assert_eq!(report.inside_out, ["Model"]);
    }

    #[test]
    fn stl_has_one_record_per_triangle_in_millimetres() {
        let bytes = stl(box_model().doc());
        assert_eq!(bytes.len(), 84 + 12 * 50);
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 12);
        let floats: Vec<f32> = bytes[84..]
            .chunks(50)
            .flat_map(|r| r[12..48].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())))
            .collect();
        let max = floats.iter().copied().fold(f32::MIN, f32::max);
        assert!((max - 20.).abs() < 1e-4);
    }

    #[test]
    fn three_mf_shares_vertices_and_names_its_parts() {
        let bytes = three_mf(box_model().doc()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("[Content_Types].xml") && text.contains("_rels/.rels"));
        assert!(text.contains("unit=\"millimeter\""));
        assert_eq!(text.matches("<vertex ").count(), 8);
        assert_eq!(text.matches("<triangle ").count(), 12);
        assert!(text.contains("<item objectid=\"1\"/>"));
        assert!(three_mf(Editor::new().doc()).is_err());
    }
}
