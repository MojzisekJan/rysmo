//! Display units and length formatting and parsing.
//!
//! Lengths are stored in metres. Display units and precision never alter geometry, and
//! typed values may carry their own suffix (`mm`, `cm`, `m`, `in`, `ft`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Units {
    #[default]
    #[serde(rename = "mm")]
    Millimetres,
    #[serde(rename = "m")]
    Metres,
    #[serde(rename = "cm")]
    Centimetres,
    #[serde(rename = "in")]
    Inches,
    #[serde(rename = "ft")]
    Feet,
    #[serde(rename = "architectural")]
    Architectural,
    #[serde(rename = "fractional")]
    Fractional,
}

impl Units {
    pub fn suffix(self) -> &'static str {
        match self {
            Units::Millimetres => "mm",
            Units::Metres => "m",
            Units::Centimetres => "cm",
            Units::Inches | Units::Architectural | Units::Fractional => "in",
            Units::Feet => "ft",
        }
    }

    /// How many units make a metre. Dividing by this (rather than multiplying by a
    /// thousandth) keeps typed values like 1250 mm exactly 1.25 m.
    pub fn per_metre(self) -> f64 {
        match self {
            Units::Millimetres => 1000.0,
            Units::Metres => 1.0,
            Units::Centimetres => 100.0,
            Units::Inches | Units::Architectural | Units::Fractional => 1.0 / 0.0254,
            Units::Feet => 1.0 / 0.3048,
        }
    }

    pub fn is_fractional(self) -> bool {
        matches!(self, Self::Architectural | Self::Fractional)
    }

    pub fn precision_label(self, p: u8) -> String {
        if self.is_fractional() {
            if p == 0 { "1 inch".into() } else { format!("1/{} inch", 1u32 << p.min(6)) }
        } else {
            format!("{p} decimal places")
        }
    }

    fn decimals(self) -> usize {
        match self {
            Units::Millimetres => 1,
            Units::Metres => 4,
            Units::Centimetres | Units::Inches | Units::Feet | Units::Architectural | Units::Fractional => 3,
        }
    }
}

/// Formats a length in metres for display: `1250 mm`, `1.25 m`, and a
/// leading `~ ` when the shown value is rounded.
pub fn format_length(metres: f64, units: Units) -> String {
    format_length_with_precision(metres, units, None)
}

/// Explicit decimal precision changes presentation only. None preserves legacy defaults.
pub fn format_length_with_precision(metres: f64, units: Units, precision: Option<u8>) -> String {
    if units.is_fractional() {
        return format_fractional(metres, units, precision.unwrap_or(4));
    }
    let value = metres * units.per_metre();
    let decimals = precision.map(|p| p.min(6) as usize).unwrap_or_else(|| units.decimals());
    let factor = 10f64.powi(decimals as i32);
    let rounded = (value * factor).round() / factor;
    let mut text = format!("{rounded:.decimals$}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".into();
    }
    let approx = ((rounded - value) / units.per_metre()).abs() > 1e-9;
    format!("{}{} {}", if approx { "~ " } else { "" }, text, units.suffix())
}

fn format_fractional(metres: f64, units: Units, precision: u8) -> String {
    let denominator = 1u64 << precision.min(6);
    let inches = metres.abs() / 0.0254;
    let ticks = (inches * denominator as f64).round();
    if !ticks.is_finite() || ticks > u64::MAX as f64 {
        return format!("{metres} m");
    }
    let ticks = ticks as u64;
    let mut whole = ticks / denominator;
    let mut numerator = ticks % denominator;
    let mut divisor = denominator;
    while numerator != 0 && numerator.is_multiple_of(2) && divisor > 1 {
        numerator /= 2;
        divisor /= 2;
    }
    let feet = if units == Units::Architectural {
        let f = whole / 12;
        whole %= 12;
        Some(f)
    } else {
        None
    };
    let fraction = if numerator == 0 {
        String::new()
    } else if whole == 0 {
        format!("{numerator}/{divisor}")
    } else {
        format!("{whole} {numerator}/{divisor}")
    };
    let inch_text = if numerator == 0 { whole.to_string() } else { fraction };
    let rounded = ticks as f64 / denominator as f64 * 0.0254;
    let prefix = if (rounded - metres.abs()).abs() > 1e-9 { "~ " } else { "" };
    let sign = if metres < 0. && ticks != 0 { "-" } else { "" };
    match feet {
        Some(feet) if whole == 0 && numerator == 0 => format!("{prefix}{sign}{feet}'"),
        Some(feet) => format!("{prefix}{sign}{feet}' {inch_text}\""),
        None => format!("{prefix}{sign}{inch_text}\""),
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParseLengthError {
    #[error("empty value")]
    Empty,
    #[error("\"{0}\" is not a length")]
    NotALength(String),
}

/// Parses one length: a number with an optional unit suffix (`mm`, `cm`, `m`, `in`, `ft`). Without a
/// suffix the number is in `units`. A decimal comma is accepted (`1,25 m`). Returns
/// metres.
pub fn parse_length(text: &str, units: Units) -> Result<f64, ParseLengthError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(ParseLengthError::Empty);
    }
    let lower = t.to_ascii_lowercase().replace(',', ".");
    let invalid = || ParseLengthError::NotALength(t.to_string());
    // The leading sign applies to the whole feet-and-inches expression.
    let (sign, unsigned) = if let Some(rest) = lower.strip_prefix('-') {
        (-1., rest.trim())
    } else {
        (1., lower.strip_prefix('+').unwrap_or(&lower).trim())
    };
    let feet_split = unsigned.split_once('\'').or_else(|| unsigned.split_once("ft"));
    let value = if let Some((feet, inches)) = feet_split {
        let feet = number(feet).ok_or_else(invalid)?;
        let inches = inches.trim();
        let inches = inches.strip_prefix('-').unwrap_or(inches).trim();
        let inches = inches.strip_suffix('"').or_else(|| inches.strip_suffix("in")).unwrap_or(inches).trim();
        let inches = if inches.is_empty() { 0. } else { number(inches).ok_or_else(invalid)? };
        if feet < 0. || inches < 0. {
            return Err(invalid());
        }
        sign * (feet * 0.3048 + inches * 0.0254)
    } else {
        let (n, per_metre) = if let Some(n) = unsigned.strip_suffix("mm") {
            (n, 1000.)
        } else if let Some(n) = unsigned.strip_suffix("cm") {
            (n, 100.)
        } else if let Some(n) = unsigned.strip_suffix('m') {
            (n, 1.)
        } else if let Some(n) = unsigned.strip_suffix("in").or_else(|| unsigned.strip_suffix('"')) {
            (n, 1. / 0.0254)
        } else {
            (unsigned, units.per_metre())
        };
        sign * number(n).ok_or_else(invalid)? / per_metre
    };
    if value.is_finite() { Ok(value) } else { Err(invalid()) }
}

/// A decimal or a mixed fraction, such as 3 1/2. Reject ambiguous signs.
fn number(text: &str) -> Option<f64> {
    let text = text.trim();
    if let Some((left, denominator)) = text.split_once('/') {
        let denominator: f64 = denominator.trim().parse().ok()?;
        if !denominator.is_finite() || denominator <= 0. {
            return None;
        }
        let parts: Vec<_> = left.split_whitespace().collect();
        let (whole, numerator) = match parts.as_slice() {
            [n] => (0., n.parse::<f64>().ok()?),
            [w, n] => (w.parse::<f64>().ok()?, n.parse::<f64>().ok()?),
            _ => return None,
        };
        if !whole.is_finite() || !numerator.is_finite() || whole < 0. || numerator < 0. {
            return None;
        }
        Some(whole + numerator / denominator)
    } else {
        text.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_in_the_chosen_unit() {
        assert_eq!(format_length(1.25, Units::Millimetres), "1250 mm");
        assert_eq!(format_length(1.25, Units::Metres), "1.25 m");
        assert_eq!(format_length(0.0, Units::Metres), "0 m");
        assert_eq!(format_length(0.00012, Units::Millimetres), "~ 0.1 mm");
        assert_eq!(format_length(1.0 / 3.0, Units::Metres), "~ 0.3333 m");
        assert_eq!(format_length(-0.5, Units::Millimetres), "-500 mm");
    }

    #[test]
    fn parses_with_and_without_suffix() {
        let mm = Units::Millimetres;
        assert_eq!(parse_length("1250", mm), Ok(1.25));
        assert_eq!(parse_length("1250mm", Units::Metres), Ok(1.25));
        assert_eq!(parse_length(" 1.25 m ", mm), Ok(1.25));
        assert_eq!(parse_length("125cm", mm), Ok(1.25));
        assert_eq!(parse_length("1,25m", mm), Ok(1.25));
        assert_eq!(parse_length("2", Units::Metres), Ok(2.0));
        assert_eq!(parse_length("-300", mm), Ok(-0.3));
        assert!(parse_length("abc", mm).is_err());
        assert_eq!(parse_length("", mm), Err(ParseLengthError::Empty));
    }
}

#[cfg(test)]
mod precision_tests {
    use super::*;
    use crate::{Command, Editor};
    #[test]
    fn imperial_and_fractional_input_in_metric_projects() {
        for (input, metres) in [
            ("1in", 0.0254),
            ("1ft", 0.3048),
            ("5' 3 1/2\"", 1.6129),
            ("-5'-3 1/2\"", -1.6129),
            ("1/2in", 0.0127),
            ("3 1/2\"", 0.0889),
            ("2,5cm", 0.025),
            ("12\"", 0.3048),
        ] {
            assert!((parse_length(input, Units::Metres).unwrap() - metres).abs() < 1e-12, "{input}");
        }
        for input in ["1/0in", "1/2/3in", "NaN", "inf", "1e309ft", "3' NaN\"", "2 3 1/2in", "--3", "3' 2ft"] {
            assert!(parse_length(input, Units::Millimetres).is_err(), "{input}");
        }
        assert!((parse_length("12", Units::Inches).unwrap() - 0.3048).abs() < 1e-12);
        assert!((parse_length("2", Units::Feet).unwrap() - 0.6096).abs() < 1e-12);
        assert_eq!(format_length(0.0254, Units::Inches), "1 in");
        assert_eq!(format_length(0.3048, Units::Feet), "1 ft");
        assert_eq!(format_length(1.25, Units::Centimetres), "125 cm");
    }
    #[test]
    fn precision_is_transactional_and_never_rounds_geometry() {
        let mut e = Editor::new();
        e.apply(Command::DrawLines { lines: vec![[rysmo_math::DVec3::ZERO, rysmo_math::DVec3::X / 3.]], facing: None })
            .unwrap();
        let mesh = e.mesh().clone();
        e.apply(Command::SetLengthPrecision { precision: Some(0) }).unwrap();
        assert_eq!(e.doc().format_length(1. / 3.), "~ 333 mm");
        assert_eq!(e.mesh(), &mesh);
        let before = e.doc().clone();
        let depth = e.undo_depth();
        assert!(e.apply(Command::SetLengthPrecision { precision: Some(7) }).is_err());
        assert_eq!(e.doc(), &before);
        assert_eq!(e.undo_depth(), depth);
        e.undo();
        assert_eq!(e.doc().length_precision, None);
        e.redo();
        assert_eq!(e.doc().length_precision, Some(0));
        assert_eq!(format_length_with_precision(-0.00001, Units::Metres, Some(0)), "~ 0 m");
        assert_eq!(format_length_with_precision(1. / 3., Units::Metres, Some(6)), "~ 0.333333 m");
    }
}

/// Project-wide measurement presentation and cursor snapping. Distances are metres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeasurementSettings {
    pub show_unit_markers: bool,
    pub force_zero_inches: bool,
    pub area_units: Option<Units>,
    pub area_precision: u8,
    pub volume_units: Option<Units>,
    pub volume_precision: u8,
    pub length_snap: Option<f64>,
    pub angle_precision: u8,
    pub angle_snap: Option<f64>,
}
impl Default for MeasurementSettings {
    fn default() -> Self {
        Self {
            show_unit_markers: true,
            force_zero_inches: false,
            area_units: None,
            area_precision: 3,
            volume_units: None,
            volume_precision: 3,
            length_snap: None,
            angle_precision: 1,
            angle_snap: Some(15.),
        }
    }
}
impl MeasurementSettings {
    pub fn valid(self) -> bool {
        self.angle_precision <= 6
            && self.volume_precision <= 6
            && self.volume_units.is_none_or(|units| !units.is_fractional())
            && self.area_precision <= 6
            && self.area_units.is_none_or(|units| !units.is_fractional())
            && self.length_snap.is_none_or(|s| s.is_finite() && (1e-9..=1e12).contains(&s))
            && self.angle_snap.is_none_or(|s| s.is_finite() && (0.1..=180.).contains(&s))
    }
    pub fn snap_length(self, length: f64) -> f64 {
        self.length_snap.map_or(length, |s| (length / s).round() * s)
    }
    pub fn snap_angle(self, angle: f64, tolerance: f64) -> f64 {
        let Some(step) = self.angle_snap else { return angle };
        let snapped = (angle / step).round() * step;
        if (angle - snapped).abs() < tolerance.min(step * 0.2) { snapped } else { angle }
    }
    pub fn format_angle(self, degrees: f64) -> String {
        format!("{:.*}°", self.angle_precision.min(6) as usize, degrees)
    }
}

#[cfg(test)]
mod measurement_settings_tests {
    use crate::{Command, Editor, MeasurementSettings};
    #[test]
    fn settings_validate_preserve_exact_lengths_and_undo() {
        let mut e = Editor::new();
        let settings = MeasurementSettings {
            length_snap: Some(0.25),
            angle_snap: Some(30.),
            angle_precision: 3,
            show_unit_markers: false,
            ..Default::default()
        };
        e.apply(Command::SetMeasurementSettings { settings }).unwrap();
        assert_eq!(e.doc().format_length(1.), "1000");
        assert_eq!(settings.snap_length(-0.61), -0.5);
        assert_eq!(settings.snap_angle(29., 2.5), 30.);
        assert_eq!(settings.format_angle(12.34567), "12.346°");
        let before = e.doc().clone();
        let mut invalid = settings;
        invalid.length_snap = Some(0.);
        assert!(e.apply(Command::SetMeasurementSettings { settings: invalid }).is_err());
        assert_eq!(e.doc(), &before);
        e.undo();
        assert_eq!(e.doc().measurement, MeasurementSettings::default());
        e.redo();
        assert_eq!(e.doc(), &before);
    }
}

#[cfg(test)]
mod fractional_tests {
    use super::*;
    use crate::{Command, Editor};
    #[test]
    fn fractional_carry_sign_precision_and_exact_input() {
        assert_eq!(format_length_with_precision(0.0254 * 3.5, Units::Fractional, Some(4)), "3 1/2\"");
        assert_eq!(format_length_with_precision(-0.0254 * 65.375, Units::Architectural, Some(6)), "-5' 5 3/8\"");
        assert_eq!(format_length_with_precision(0.0254 * 11.9999, Units::Architectural, Some(6)), "~ 1'");
        assert_eq!(format_length_with_precision(0.0254 / 64., Units::Fractional, Some(6)), "1/64\"");
        for units in [Units::Architectural, Units::Fractional] {
            for inches in [-65.375, 0., 0.5, 12., 18.25] {
                let metres = inches * 0.0254;
                let text = format_length_with_precision(metres, units, Some(6));
                assert!((parse_length(&text, units).unwrap() - metres).abs() < 1e-12, "{text}");
            }
        }
    }
    #[test]
    fn architectural_zero_inches_and_undo() {
        let mut e = Editor::new();
        e.apply(Command::SetUnits { units: Units::Architectural }).unwrap();
        assert_eq!(e.doc().format_length(0.3048), "1'");
        assert_eq!(e.doc().format_area(0.3048 * 0.3048), "1.000 ft²");
        let mut settings = e.doc().measurement;
        settings.force_zero_inches = true;
        settings.show_unit_markers = false;
        e.apply(Command::SetMeasurementSettings { settings }).unwrap();
        assert_eq!(e.doc().format_length(0.3048), "1' 0\"");
        e.undo();
        assert_eq!(e.doc().format_length(0.3048), "1'");
    }
}

#[cfg(test)]
mod area_settings_tests {
    use super::*;
    use crate::{Command, Editor};
    #[test]
    fn independent_area_units_and_precision_do_not_change_geometry_and_undo() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle {
            corner: rysmo_math::DVec3::ZERO,
            u: rysmo_math::DVec3::X,
            v: rysmo_math::DVec3::Y,
            facing: None,
        })
        .unwrap();
        let mesh = e.mesh().clone();
        assert_eq!(e.doc().format_area(1.), "1.000 m²");
        let mut settings = e.doc().measurement;
        settings.area_units = Some(Units::Centimetres);
        settings.area_precision = 1;
        e.apply(Command::SetMeasurementSettings { settings }).unwrap();
        assert_eq!(e.doc().format_area(1.), "10000.0 cm²");
        assert_eq!(e.mesh(), &mesh);
        e.undo();
        assert_eq!(e.doc().format_area(1.), "1.000 m²");
        settings.area_units = Some(Units::Architectural);
        assert!(e.apply(Command::SetMeasurementSettings { settings }).is_err());
        assert_eq!(e.mesh(), &mesh);
    }
}
