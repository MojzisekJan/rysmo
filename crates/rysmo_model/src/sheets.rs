//! Persisted paper-space layouts; model geometry is evaluated only at preview/export.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    Top,
    Front,
    Right,
    Isometric,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub name: String,
    pub projection: Projection,
    pub rect_mm: [f64; 4],
    pub scale: f64,
    pub hidden_lines: bool,
    pub dimensions: bool,
    pub annotations: bool,
    #[serde(default)]
    pub scene: Option<u64>,
}
impl Default for Viewport {
    fn default() -> Self {
        Self {
            name: "View".into(),
            projection: Projection::Top,
            rect_mm: [12., 12., 130., 150.],
            scale: 50.,
            hidden_lines: true,
            dimensions: true,
            annotations: true,
            scene: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub text: String,
    pub position_mm: [f64; 2],
    pub size_mm: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub page_mm: [f64; 2],
    pub title_block: String,
    pub viewports: Vec<Viewport>,
    #[serde(default)]
    pub notes: Vec<Note>,
}
impl Default for Sheet {
    fn default() -> Self {
        Self {
            name: "Sheet 1".into(),
            page_mm: [297., 210.],
            title_block: "Project | Drawing | Revision".into(),
            viewports: vec![
                Viewport::default(),
                Viewport {
                    name: "Front".into(),
                    projection: Projection::Front,
                    rect_mm: [155., 12., 130., 150.],
                    ..Default::default()
                },
            ],
            notes: Vec::new(),
        }
    }
}
fn valid_text(text: &str) -> bool {
    text.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
}
pub fn validate(sheets: &[Sheet]) -> Result<(), String> {
    if sheets.len() > 32 || sheets.iter().map(|s| s.viewports.len()).sum::<usize>() > 128 {
        return Err("Use up to 32 sheets and 128 total viewports".into());
    }
    for sheet in sheets {
        if sheet.name.trim().is_empty()
            || sheet.name.len() > 128
            || !valid_text(&sheet.name)
            || !valid_text(&sheet.title_block)
            || sheet.title_block.lines().count() > 4
            || sheet.title_block.len() > 2048
            || sheet.page_mm.iter().any(|n| !n.is_finite() || !(50. ..=2000.).contains(n))
            || sheet.viewports.len() > 16
            || sheet.notes.len() > 64
        {
            return Err("Invalid sheet name, page size or content limits".into());
        }
        for viewport in &sheet.viewports {
            let [x, y, w, h] = viewport.rect_mm;
            if !valid_text(&viewport.name)
                || viewport.name.len() > 128
                || !viewport.scale.is_finite()
                || !(0.001..=1e6).contains(&viewport.scale)
                || [x, y, w, h].iter().any(|n| !n.is_finite())
                || x < 5.
                || y < 5.
                || w < 30.
                || h < 30.
                || x + w > sheet.page_mm[0] - 5.
                || y + h > sheet.page_mm[1] - 25.
            {
                return Err(
                    "Viewports need finite scale and at least 30 mm size inside the page frame/title block".into()
                );
            }
        }
        for note in &sheet.notes {
            if !valid_text(&note.text)
                || note.text.len() > 4096
                || !note.size_mm.is_finite()
                || !(1. ..=20.).contains(&note.size_mm)
                || note.position_mm.iter().enumerate().any(|(i, n)| !n.is_finite() || *n < 0. || *n > sheet.page_mm[i])
            {
                return Err("Invalid paper-space note".into());
            }
        }
    }
    Ok(())
}
