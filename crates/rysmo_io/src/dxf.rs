//! ASCII DXF 2004: world-space, Z-up metre 3DFACE triangles and loose LINE edges.
//! Triangle diagonals are marked invisible. Materials become true-colour faces;
//! texture images, annotations and editable CAD solids are not represented.
use rysmo_model::Document;
use std::fmt::{Display, Write};

fn code(out: &mut String, group: u16, value: impl Display) {
    let _ = write!(out, "{group}\r\n{value}\r\n");
}
fn point(out: &mut String, index: u16, p: rysmo_math::DVec3) {
    for (group, value) in [(10 + index, p.x), (20 + index, p.y), (30 + index, p.z)] {
        code(out, group, value);
    }
}

pub fn export(doc: &Document) -> String {
    let geometries = crate::exchange::collect(doc);
    let mut out = String::new();
    code(&mut out, 0, "SECTION");
    code(&mut out, 2, "HEADER");
    code(&mut out, 9, "$ACADVER");
    code(&mut out, 1, "AC1018");
    code(&mut out, 9, "$INSUNITS");
    code(&mut out, 70, 6);
    code(&mut out, 9, "$MEASUREMENT");
    code(&mut out, 70, 1);
    code(&mut out, 0, "ENDSEC");
    code(&mut out, 0, "SECTION");
    code(&mut out, 2, "TABLES");
    code(&mut out, 0, "TABLE");
    code(&mut out, 2, "LAYER");
    code(&mut out, 5, "1");
    code(&mut out, 100, "AcDbSymbolTable");
    code(&mut out, 70, geometries.len().max(1));
    for i in 0..geometries.len().max(1) {
        code(&mut out, 0, "LAYER");
        code(&mut out, 100, "AcDbSymbolTableRecord");
        code(&mut out, 100, "AcDbLayerTableRecord");
        code(&mut out, 2, format!("Rysmo_{i}"));
        code(&mut out, 70, 0);
        code(&mut out, 62, 7);
        code(&mut out, 6, "CONTINUOUS");
    }
    code(&mut out, 0, "ENDTAB");
    code(&mut out, 0, "ENDSEC");
    code(&mut out, 0, "SECTION");
    code(&mut out, 2, "ENTITIES");
    for (i, geometry) in geometries.iter().enumerate() {
        for triangle in &geometry.triangles {
            code(&mut out, 0, "3DFACE");
            code(&mut out, 100, "AcDbEntity");
            code(&mut out, 8, format!("Rysmo_{i}"));
            if let Some(material) = triangle.material {
                let rgb = doc.materials[&material].color.map(|c| (c.clamp(0., 1.) * 255.).round() as u32);
                code(&mut out, 420, (rgb[0] << 16) | (rgb[1] << 8) | rgb[2]);
            }
            code(&mut out, 100, "AcDbFace");
            for k in 0..4 {
                point(&mut out, k, triangle.points[(k as usize).min(2)]);
            }
            let mut flags = 4;
            for (edge, bit) in [1, 2, 8].into_iter().enumerate() {
                if !triangle.boundary[edge] {
                    flags |= bit;
                }
            }
            code(&mut out, 70, flags);
        }
        for line in &geometry.lines {
            code(&mut out, 0, "LINE");
            code(&mut out, 100, "AcDbEntity");
            code(&mut out, 8, format!("Rysmo_{i}"));
            code(&mut out, 100, "AcDbLine");
            point(&mut out, 0, line[0]);
            point(&mut out, 1, line[1]);
        }
    }
    code(&mut out, 0, "ENDSEC");
    code(&mut out, 0, "EOF");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};

    #[test]
    fn rectangle_exports_two_faces_and_hides_the_shared_diagonal() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(0., 0., 7.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 3.,
                facing: None,
            })
            .unwrap();
        let out = export(editor.doc());
        let lines: Vec<_> = out.lines().collect();
        let pairs: Vec<_> = lines.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect();
        assert!(pairs.windows(2).any(|p| p == [("9", "$INSUNITS"), ("70", "6")]));
        assert_eq!(pairs.iter().filter(|p| **p == ("0", "3DFACE")).count(), 2);
        assert!(!pairs.contains(&("0", "LINE")));
        let faces = out.split("0\r\n3DFACE\r\n").skip(1);
        for face in faces {
            let values: Vec<_> = face.lines().collect();
            let pairs: Vec<_> = values.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect();
            let flags: u8 = pairs.iter().find(|p| p.0 == "70").unwrap().1.parse().unwrap();
            assert_eq!(flags.count_ones(), 2);
            assert!(pairs.contains(&("30", "7")));
        }
    }
}
