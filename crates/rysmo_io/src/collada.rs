//! COLLADA 1.4.1, Y-up metres, flat normals, per-occurrence world meshes and UVs.
//! Images remain original PNG/JPEG companion files. Model annotations are omitted.
use rysmo_mesh::MaterialId;
use rysmo_model::Document;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

pub struct ColladaExport {
    pub dae: String,
    pub images: Vec<(String, Vec<u8>)>,
}

// COLLADA 1.4.1 name attributes use xs:NCName, so XML escaping alone is insufficient.
fn xml_name(value: &str) -> String {
    let mut out = String::new();
    for (i, c) in value.chars().enumerate() {
        if c.is_ascii_alphabetic() || c == '_' || (i > 0 && (c.is_ascii_digit() || c == '-' || c == '.')) {
            out.push(c);
        } else {
            let _ = write!(out, "_u{:04X}_", c as u32);
        }
    }
    if out.is_empty() {
        out.push_str("Unnamed");
    }
    out
}
fn material(id: Option<MaterialId>) -> String {
    id.map(|id| format!("m-{}", id.0)).unwrap_or_else(|| "m-default".into())
}
fn y_up(p: rysmo_math::DVec3) -> [f64; 3] {
    [p.x, p.z, -p.y]
}
fn floats(out: &mut String, id: &str, values: &[f64], axes: &[&str]) {
    let _ = write!(out, "<source id=\"{id}\"><float_array id=\"{id}-array\" count=\"{}\">", values.len());
    for value in values {
        let _ = write!(out, "{value} ");
    }
    let _ = write!(
        out,
        "</float_array><technique_common><accessor source=\"#{id}-array\" count=\"{}\" stride=\"{}\">",
        values.len() / axes.len(),
        axes.len()
    );
    for axis in axes {
        let _ = write!(out, "<param name=\"{axis}\" type=\"float\"/>");
    }
    out.push_str("</accessor></technique_common></source>");
}
fn timestamp(seconds: u64) -> String {
    let mut days = seconds / 86400;
    let mut year = 1970;
    let leap = |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    while days >= if leap(year) { 366 } else { 365 } {
        days -= if leap(year) { 366 } else { 365 };
        year += 1;
    }
    let mut month = 1;
    for length in [31, if leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] {
        if days < length {
            break;
        }
        days -= length;
        month += 1;
    }
    format!(
        "{year:04}-{month:02}-{:02}T{:02}:{:02}:{:02}Z",
        days + 1,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

pub fn export(doc: &Document) -> ColladaExport {
    let geometries = crate::exchange::collect(doc);
    let materials: BTreeSet<_> = geometries.iter().flat_map(|g| g.triangles.iter().map(|t| t.material)).collect();
    let mut images = BTreeMap::new();
    for id in &materials {
        if let Some(texture) =
            id.and_then(|id| doc.materials[&id].texture).filter(|t| doc.images.contains_key(&t.image))
        {
            let image = &doc.images[&texture.image];
            let ext = if image.mime.contains("jpeg") { "jpg" } else { "png" };
            images.entry(texture.image).or_insert_with(|| {
                (
                    format!("texture-{}-{:08x}.{ext}", texture.image.0, crc32fast::hash(&image.bytes)),
                    image.bytes.as_ref().clone(),
                )
            });
        }
    }
    let now = timestamp(web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs());
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<COLLADA xmlns=\"http://www.collada.org/2005/11/COLLADASchema\" version=\"1.4.1\"><asset><contributor><authoring_tool>Rysmo</authoring_tool></contributor><created>{now}</created><modified>{now}</modified><unit name=\"meter\" meter=\"1\"/><up_axis>Y_UP</up_axis></asset>"
    );
    if !images.is_empty() {
        out.push_str("<library_images>");
        for (id, (file, _)) in &images {
            let _ = write!(out, "<image id=\"image-{}\"><init_from>{file}</init_from></image>", id.0);
        }
        out.push_str("</library_images>");
    }
    if !materials.is_empty() {
        out.push_str("<library_effects>");
        for id in &materials {
            let name = material(*id);
            let mat = id.and_then(|id| doc.materials.get(&id));
            let color = mat.map(|m| m.color).unwrap_or([1.; 4]);
            let texture = mat.and_then(|m| m.texture).filter(|t| images.contains_key(&t.image));
            let _ = write!(out, "<effect id=\"{name}-effect\"><profile_COMMON>");
            if let Some(t) = texture {
                let _ = write!(
                    out,
                    "<newparam sid=\"surface\"><surface type=\"2D\"><init_from>image-{}</init_from></surface></newparam><newparam sid=\"sampler\"><sampler2D><source>surface</source></sampler2D></newparam>",
                    t.image.0
                );
            }
            out.push_str("<technique sid=\"common\"><lambert><diffuse>");
            if texture.is_some() {
                out.push_str("<texture texture=\"sampler\" texcoord=\"UVSET0\"/>");
            } else {
                let _ = write!(out, "<color>{} {} {} 1</color>", color[0], color[1], color[2]);
            }
            out.push_str("</diffuse><transparent opaque=\"A_ONE\">");
            if texture.is_some() {
                out.push_str("<texture texture=\"sampler\" texcoord=\"UVSET0\"/>");
            } else {
                out.push_str("<color>1 1 1 1</color>");
            }
            let _ = write!(
                out,
                "</transparent><transparency><float>{}</float></transparency></lambert></technique><extra><technique profile=\"GOOGLEEARTH\"><double_sided>1</double_sided></technique></extra></profile_COMMON></effect>",
                color[3]
            );
        }
        out.push_str("</library_effects><library_materials>");
        for id in &materials {
            let name = material(*id);
            let label = id.and_then(|id| doc.materials.get(&id)).map(|m| m.name.as_str()).unwrap_or("Default");
            let _ = write!(
                out,
                "<material id=\"{name}\" name=\"{}\"><instance_effect url=\"#{name}-effect\"/></material>",
                xml_name(label)
            );
        }
        out.push_str("</library_materials>");
    }
    if !geometries.is_empty() {
        out.push_str("<library_geometries>");
        for (i, g) in geometries.iter().enumerate() {
            let _ = write!(out, "<geometry id=\"g-{i}\" name=\"{}\"><mesh>", xml_name(&g.name));
            let mut positions: Vec<_> = g.triangles.iter().flat_map(|t| t.points.into_iter().flat_map(y_up)).collect();
            positions.extend(g.lines.iter().flat_map(|l| l.iter().flat_map(|p| y_up(*p))));
            floats(&mut out, &format!("g-{i}-positions"), &positions, &["X", "Y", "Z"]);
            if !g.triangles.is_empty() {
                let normals: Vec<_> =
                    g.triangles.iter().flat_map(|t| std::iter::repeat_n(y_up(t.normal), 3).flatten()).collect();
                let uvs: Vec<_> =
                    g.triangles.iter().flat_map(|t| t.uv.into_iter().flat_map(|uv| [uv[0], -uv[1]])).collect();
                floats(&mut out, &format!("g-{i}-normals"), &normals, &["X", "Y", "Z"]);
                floats(&mut out, &format!("g-{i}-uvs"), &uvs, &["S", "T"]);
            }
            let _ = write!(
                out,
                "<vertices id=\"g-{i}-vertices\"><input semantic=\"POSITION\" source=\"#g-{i}-positions\"/></vertices>"
            );
            let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for (index, t) in g.triangles.iter().enumerate() {
                groups.entry(t.material).or_default().push(index);
            }
            for (id, indices) in groups {
                let _ = write!(
                    out,
                    "<triangles count=\"{}\" material=\"{}\"><input semantic=\"VERTEX\" source=\"#g-{i}-vertices\" offset=\"0\"/><input semantic=\"NORMAL\" source=\"#g-{i}-normals\" offset=\"1\"/><input semantic=\"TEXCOORD\" source=\"#g-{i}-uvs\" offset=\"2\" set=\"0\"/><p>",
                    indices.len(),
                    material(id)
                );
                for triangle in indices {
                    for v in 3 * triangle..3 * triangle + 3 {
                        let _ = write!(out, "{v} {v} {v} ");
                    }
                }
                out.push_str("</p></triangles>");
            }
            if !g.lines.is_empty() {
                let _ = write!(
                    out,
                    "<lines count=\"{}\"><input semantic=\"VERTEX\" source=\"#g-{i}-vertices\" offset=\"0\"/><p>",
                    g.lines.len()
                );
                for v in g.triangles.len() * 3..positions.len() / 3 {
                    let _ = write!(out, "{v} ");
                }
                out.push_str("</p></lines>");
            }
            out.push_str("</mesh></geometry>");
        }
        out.push_str("</library_geometries>");
    }
    out.push_str("<library_visual_scenes><visual_scene id=\"Scene\" name=\"Scene\">");
    if geometries.is_empty() {
        out.push_str("<node id=\"empty\" name=\"Empty_model\"/>");
    }
    for (i, g) in geometries.iter().enumerate() {
        let _ = write!(out, "<node id=\"node-{i}\" name=\"{}\"><instance_geometry url=\"#g-{i}\">", xml_name(&g.name));
        let bound: BTreeSet<_> = g.triangles.iter().map(|t| t.material).collect();
        if !bound.is_empty() {
            out.push_str("<bind_material><technique_common>");
            for id in bound {
                let name = material(id);
                let _ = write!(
                    out,
                    "<instance_material symbol=\"{name}\" target=\"#{name}\"><bind_vertex_input semantic=\"UVSET0\" input_semantic=\"TEXCOORD\" input_set=\"0\"/></instance_material>"
                );
            }
            out.push_str("</technique_common></bind_material>");
        }
        out.push_str("</instance_geometry></node>");
    }
    out.push_str(
        "</visual_scene></library_visual_scenes><scene><instance_visual_scene url=\"#Scene\"/></scene></COLLADA>\n",
    );
    ColladaExport { dae: out, images: images.into_values().collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};
    #[test]
    fn texture_references_uvs_and_escaped_material_names_are_preserved() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y, facing: None })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let image = editor
            .apply(Command::AddImage { name: "a<&.png".into(), mime: "image/png".into(), data: vec![1, 2, 3] })
            .unwrap()
            .image
            .unwrap();
        let material = editor
            .apply(Command::AddMaterial {
                name: "Brick <&\"".into(),
                color: [1., 0.5, 0., 0.5],
                texture: Some(rysmo_model::Texture { image, size: [0.5, 0.25] }),
            })
            .unwrap()
            .material;
        editor.apply(Command::Paint { faces: vec![face], material, back: false }).unwrap();
        let output = export(editor.doc());
        assert_eq!(output.images.len(), 1);
        assert_eq!(output.images[0].1, vec![1, 2, 3]);
        assert!(output.dae.contains(&output.images[0].0));
        assert!(output.dae.contains("Brick_u0020__u003C__u0026__u0022_"));
        assert!(output.dae.contains("input_semantic=\"TEXCOORD\" input_set=\"0\""));
        assert!(output.dae.contains("<up_axis>Y_UP</up_axis>"));
        assert!(output.dae.contains("<triangles count=\"2\""));
    }
    #[test]
    fn utc_timestamps_cover_epoch_and_leap_day() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(1709251199), "2024-02-29T23:59:59Z");
        let empty = export(&Document::new()).dae;
        assert!(!empty.contains("<library_geometries>"));
        assert!(empty.contains("<node id=\"empty\""));
    }
}
