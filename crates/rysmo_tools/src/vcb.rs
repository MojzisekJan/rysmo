//! Parsing what the user types into the VCB (the Measurements box).
//!
//! Typing goes to the VCB without clicking it, and Enter applies it:
//!
//! - a length: `1250`, `1250mm`, `1.25m`, `125cm` (no suffix means the display unit);
//! - two lengths for a rectangle: `2000,1000` or `2000;1000`;
//! - copies after a Move with Ctrl: `5x`, `x5` or `*5` for five copies at the same
//!   spacing, `/5` for five copies dividing the distance;
//! - sides of a circle: `24s`.
//!
//! When a tool wants one value, a comma is a decimal comma (`1,25m`). When it wants two,
//! a semicolon separates them if there is one, otherwise the comma does.

use rysmo_model::{Units, parse_length};

#[derive(Clone, Debug, PartialEq)]
pub enum VcbInput {
    /// Lengths in metres.
    Lengths(Vec<f64>),
    /// `5x`: this many copies at the same spacing.
    Copies(u32),
    /// `/5`: this many copies dividing the distance.
    Divide(u32),
    /// `24s`: this many sides.
    Segments(u32),
}

/// Parses VCB text. `values` is how many lengths the tool takes (1 or 2).
pub fn parse_vcb(text: &str, units: Units, values: usize) -> Result<VcbInput, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("Type a value first.".into());
    }
    let count = |s: &str| -> Result<u32, String> {
        let n: u32 = s.trim().parse().map_err(|_| format!("\"{t}\" is not a count"))?;
        if n == 0 { Err("The count must be at least 1.".into()) } else { Ok(n) }
    };
    let lower = t.to_ascii_lowercase();
    if let Some(n) = lower.strip_suffix('x').filter(|n| n.trim().chars().all(|c| c.is_ascii_digit())) {
        return count(n).map(VcbInput::Copies);
    }
    if let Some(n) = lower.strip_prefix('x').or_else(|| lower.strip_prefix('*')) {
        return count(n).map(VcbInput::Copies);
    }
    if let Some(n) = lower.strip_prefix('/') {
        return count(n).map(VcbInput::Divide);
    }
    if let Some(n) = lower.strip_suffix('s') {
        let n = count(n)?;
        if n < 3 {
            return Err("A circle needs at least 3 sides.".into());
        }
        return Ok(VcbInput::Segments(n));
    }
    let parts: Vec<&str> = if values >= 2 {
        if t.contains(';') { t.split(';').collect() } else { t.split(',').collect() }
    } else if t.contains(';') {
        t.split(';').collect()
    } else {
        vec![t]
    };
    let mut lengths = Vec::new();
    for p in parts {
        if p.trim().is_empty() {
            // "2000," keeps the second value as it is; reported as NaN for the tool.
            lengths.push(f64::NAN);
            continue;
        }
        lengths.push(parse_length(p, units).map_err(|e| e.to_string())?);
    }
    Ok(VcbInput::Lengths(lengths))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MM: Units = Units::Millimetres;

    #[test]
    fn single_lengths() {
        assert_eq!(parse_vcb("1250", MM, 1), Ok(VcbInput::Lengths(vec![1.25])));
        assert_eq!(parse_vcb("1.5m", MM, 1), Ok(VcbInput::Lengths(vec![1.5])));
        assert_eq!(parse_vcb("1,5m", MM, 1), Ok(VcbInput::Lengths(vec![1.5])));
        assert_eq!(parse_vcb("2", Units::Metres, 1), Ok(VcbInput::Lengths(vec![2.0])));
    }

    #[test]
    fn two_lengths() {
        assert_eq!(parse_vcb("2000,1000", MM, 2), Ok(VcbInput::Lengths(vec![2.0, 1.0])));
        assert_eq!(parse_vcb("2m; 1,5m", MM, 2), Ok(VcbInput::Lengths(vec![2.0, 1.5])));
        assert_eq!(parse_vcb("2m,1m", MM, 2), Ok(VcbInput::Lengths(vec![2.0, 1.0])));
        match parse_vcb(",500", MM, 2) {
            Ok(VcbInput::Lengths(v)) => assert!(v[0].is_nan() && v[1] == 0.5),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn copies_and_sides() {
        assert_eq!(parse_vcb("5x", MM, 1), Ok(VcbInput::Copies(5)));
        assert_eq!(parse_vcb("x5", MM, 1), Ok(VcbInput::Copies(5)));
        assert_eq!(parse_vcb("*3", MM, 1), Ok(VcbInput::Copies(3)));
        assert_eq!(parse_vcb("/4", MM, 1), Ok(VcbInput::Divide(4)));
        assert_eq!(parse_vcb("24s", MM, 1), Ok(VcbInput::Segments(24)));
        assert_eq!(parse_vcb("24S", MM, 1), Ok(VcbInput::Segments(24)));
    }

    #[test]
    fn errors() {
        assert!(parse_vcb("", MM, 1).is_err());
        assert!(parse_vcb("abc", MM, 1).is_err());
        assert!(parse_vcb("0x", MM, 1).is_err());
        assert!(parse_vcb("2s", MM, 1).is_err());
        assert!(parse_vcb("/0", MM, 1).is_err());
    }
}

#[cfg(test)]
mod imperial_tests {
    use super::*;
    #[test]
    fn mixed_units_and_fractions_preserve_vcb_delimiters() {
        let VcbInput::Lengths(v) = parse_vcb("5' 3 1/2\"; 125cm", Units::Millimetres, 2).unwrap() else { panic!() };
        assert!((v[0] - 1.6129).abs() < 1e-12);
        assert_eq!(v[1], 1.25);
        assert_eq!(parse_vcb("/5", Units::Feet, 1), Ok(VcbInput::Divide(5)));
        assert!(parse_vcb("1/2in", Units::Metres, 1).is_ok());
    }
}
