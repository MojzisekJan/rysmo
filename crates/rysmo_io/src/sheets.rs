//! Regenerated paper-space viewports and multi-page vector output.
use crate::drawing::{self, Drawing, Label, Line, Options, Orientation};
use rysmo_math::DVec2;
use rysmo_model::{
    Document,
    sheets::{Projection, Sheet},
};
pub fn build(doc: &Document, sheet: &Sheet) -> Result<Drawing, String> {
    rysmo_model::sheets::validate(std::slice::from_ref(sheet))?;
    let mut result =
        Drawing { page_mm: sheet.page_mm, lines: Vec::new(), fills: Vec::new(), labels: Vec::new(), scale: 1. };
    let rect = |x: f64, y: f64, w: f64, h: f64| {
        let points = [DVec2::new(x, y), DVec2::new(x + w, y), DVec2::new(x + w, y + h), DVec2::new(x, y + h)];
        (0..4).map(move |i| Line { points: [points[i], points[(i + 1) % 4]], width: 0.18 })
    };
    result.lines.extend(rect(5., 5., sheet.page_mm[0] - 10., sheet.page_mm[1] - 10.));
    result.lines.extend(rect(5., sheet.page_mm[1] - 23., sheet.page_mm[0] - 10., 18.));
    for (i, text) in sheet.title_block.lines().enumerate() {
        result.labels.push(Label {
            at: DVec2::new(9., sheet.page_mm[1] - 17. + i as f64 * 4.),
            text: text.into(),
            size: 3.,
        });
    }
    result.labels.push(Label { at: DVec2::new(9., 9.), text: sheet.name.clone(), size: 3. });
    for viewport in &sheet.viewports {
        let [x, y, w, h] = viewport.rect_mm;
        let options = Options {
            page_mm: [w, h],
            margin_mm: 2.,
            scale: viewport.scale,
            orientation: match viewport.projection {
                Projection::Top => Orientation::Top,
                Projection::Front => Orientation::Front,
                Projection::Right => Orientation::Right,
                Projection::Isometric => Orientation::Isometric,
            },
            hidden_lines: viewport.hidden_lines,
            dimensions: viewport.dimensions,
            annotations: viewport.annotations,
            title: viewport.name.clone(),
        };
        let mut scene_doc;
        let source = if let Some(id) = viewport.scene {
            let scene = doc
                .scenes
                .iter()
                .find(|s| s.id == id)
                .ok_or_else(|| format!("Missing scene for viewport {}", viewport.name))?;
            scene_doc = doc.clone();
            scene.restore(&mut scene_doc);
            &scene_doc
        } else {
            doc
        };
        let drawing = drawing::build(source, &options).map_err(|error| format!("{}: {error}", viewport.name))?;
        let offset = DVec2::new(x, y);
        result.lines.extend(
            drawing.lines.into_iter().map(|line| Line { points: line.points.map(|p| p + offset), width: line.width }),
        );
        result.fills.extend(drawing.fills.into_iter().map(|polygon| polygon.into_iter().map(|p| p + offset).collect()));
        result.labels.extend(drawing.labels.into_iter().map(|label| Label { at: label.at + offset, ..label }));
        result.lines.extend(rect(x, y, w, h));
        if result.lines.len() > 200_000 {
            return Err("Sheet exceeds 200000 vector lines".into());
        }
    }
    for note in &sheet.notes {
        for (i, text) in note.text.lines().enumerate() {
            result.labels.push(Label {
                at: DVec2::from_array(note.position_mm) + DVec2::Y * i as f64 * note.size_mm * 1.3,
                text: text.into(),
                size: note.size_mm,
            });
        }
    }
    for label in &result.labels {
        if label.at.x < 0.
            || label.at.y < label.size
            || label.at.y > sheet.page_mm[1]
            || label.at.x + drawing::text_width(&label.text, label.size) + 1.2 > sheet.page_mm[0]
        {
            return Err(format!("Paper text does not fit on sheet {}: {}", sheet.name, label.text));
        }
    }
    Ok(result)
}
pub fn pdf(doc: &Document, sheets: &[Sheet]) -> Result<Vec<u8>, String> {
    rysmo_model::sheets::validate(sheets)?;
    if sheets.is_empty() {
        return Err("Add at least one drawing sheet".into());
    }
    let mut drawings = Vec::new();
    let mut lines = 0usize;
    for sheet in sheets {
        let drawing = build(doc, sheet)?;
        lines = lines.saturating_add(drawing.lines.len());
        if lines > 500_000 {
            return Err("Drawing set exceeds 500000 lines".into());
        }
        drawings.push(drawing);
    }
    Ok(drawing::pdf::write_pages(&drawings))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sheet_layouts_roundtrip_and_pdf_has_multiple_pages_with_unicode() {
        let mut doc = Document::new();
        let a = Sheet { name: "Přízemí".into(), viewports: Vec::new(), ..Default::default() };
        let mut b = a.clone();
        b.name = "Detail".into();
        b.page_mm = [420., 297.];
        doc.sheets = vec![a, b];
        let loaded = crate::load(&crate::save(&doc)).unwrap();
        assert_eq!(loaded.sheets, doc.sheets);
        let bytes = pdf(&loaded, &loaded.sheets).unwrap();
        let source = String::from_utf8_lossy(&bytes);
        assert!(source.contains("/Count 2"));
        assert!(source.contains("/Type /Pages"));
        assert!(source.contains("/ToUnicode"));
        assert!(build(&loaded, &loaded.sheets[0]).unwrap().svg().contains("Přízemí"));
    }
    #[test]
    fn invalid_page_geometry_and_xml_controls_fail_before_export() {
        let mut sheet = Sheet::default();
        sheet.viewports[0].rect_mm[0] = -1.;
        assert!(build(&Document::new(), &sheet).is_err());
        sheet = Sheet::default();
        sheet.title_block = "Bad\u{1}".into();
        assert!(build(&Document::new(), &sheet).is_err());
    }
}
