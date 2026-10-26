//! Measurement systems, units and lengths as written in configuration files.
//!
//! - `system` ('metric | 'imperial) decides how lengths are presented: decimal metres or
//!   feet-and-inches, m² or ft², 1:100 or 1:96 paper scale.
//! - `units` is the unit of bare numbers (`size = 30`). It defaults to metres (metric) or
//!   feet (imperial), and it is also the unit the output geometry is expressed in.
//! - Any length may instead be a string carrying its own unit: `"98'-6"`, `"12 ft 6 in"`,
//!   `"9.5 m"`, `"950 mm"`. Strings are converted exactly, so mixing systems is explicit.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum System {
    #[default]
    Metric,
    Imperial,
}

impl System {
    /// Unit for bare numbers when the file doesn't set `units`.
    pub fn default_units(self) -> Units {
        match self {
            System::Metric => Units::M,
            System::Imperial => Units::Ft,
        }
    }

    /// Conventional plan scale denominator for printed output (1:100, or 1/8" = 1'-0").
    pub fn paper_scale(self) -> f64 {
        match self {
            System::Metric => 100.0,
            System::Imperial => 96.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl Units {
    pub fn symbol(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Cm => "cm",
            Units::M => "m",
            Units::In => "in",
            Units::Ft => "ft",
        }
    }

    /// Length of one unit in millimetres.
    pub fn to_mm(self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Cm => 10.0,
            Units::M => 1000.0,
            Units::In => 25.4,
            Units::Ft => 304.8,
        }
    }

    /// Distance below which two points count as the same: 0.01 mm, in this unit.
    pub fn tolerance(self) -> f64 {
        0.01 / self.to_mm()
    }

    pub fn system(self) -> System {
        match self {
            Units::Mm | Units::Cm | Units::M => System::Metric,
            Units::In | Units::Ft => System::Imperial,
        }
    }
}

/// A length as written in the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// A plain number, measured in the file's `units`.
    Bare(f64),
    /// A string with explicit units, already converted to millimetres.
    Mm(f64),
}

impl Default for Length {
    fn default() -> Self {
        Length::Bare(0.0)
    }
}

impl Length {
    /// The length expressed in `units`.
    pub fn to(self, units: Units) -> f64 {
        match self {
            Length::Bare(v) => v,
            Length::Mm(mm) => mm / units.to_mm(),
        }
    }

    /// Parse a length string: `"30"`, `"98'-6"`, `"98' 6 1/2\""`, `"12 ft 6 in"`, `"6\""`,
    /// `"9.5 m"`, `"950mm"`, `"-1.2 m"`. A string with no unit at all is a bare number.
    pub fn parse(input: &str) -> Result<Length, String> {
        parse_length(input).ok_or_else(|| {
            format!(
                "can't read \"{input}\" as a length; write a number, or a string such as \
                 \"98'-6\", \"12 ft 6 in\", \"9.5 m\" or \"950 mm\""
            )
        })
    }
}

impl<'de> Deserialize<'de> for Length {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LengthVisitor;

        impl Visitor<'_> for LengthVisitor {
            type Value = Length;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a length: a number, or a string like \"98'-6\" or \"9.5 m\"")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Length, E> {
                Ok(Length::Bare(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Length, E> {
                Ok(Length::Bare(v as f64))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Length, E> {
                Ok(Length::Bare(v as f64))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Length, E> {
                Length::parse(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(LengthVisitor)
    }
}

// ------------------------------------------------------------------------------------ parser

#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    Mm,
    Cm,
    M,
    Ft,
    In,
}

impl Unit {
    fn mm(self) -> f64 {
        match self {
            Unit::Mm => 1.0,
            Unit::Cm => 10.0,
            Unit::M => 1000.0,
            Unit::Ft => 304.8,
            Unit::In => 25.4,
        }
    }
}

struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn done(&self) -> bool {
        self.i >= self.s.len()
    }

    /// An unsigned decimal (`12`, `12.5`, `.5`) or fraction (`1/2`).
    fn number(&mut self) -> Option<f64> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
            self.i += 1;
        }
        if self.i == start {
            return None;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        let value: f64 = text.parse().ok()?;
        if self.peek() == Some(b'/') {
            let save = self.i;
            self.i += 1;
            let dstart = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            let den: f64 = std::str::from_utf8(&self.s[dstart..self.i]).ok()?.parse().ok().unwrap_or(0.0);
            if den == 0.0 {
                self.i = save;
                return None;
            }
            return Some(value / den);
        }
        Some(value)
    }

    /// A value with an optional mixed fraction: `6`, `6 1/2`, `1/2`.
    fn mixed(&mut self) -> Option<f64> {
        let whole = self.number()?;
        let save = self.i;
        self.skip_ws();
        let frac_start = self.i;
        if let Some(frac) = self.number() {
            let has_slash = self.s[frac_start..self.i].contains(&b'/');
            if has_slash && frac < 1.0 {
                return Some(whole + frac);
            }
        }
        self.i = save;
        Some(whole)
    }

    fn unit(&mut self) -> Option<Unit> {
        self.skip_ws();
        let rest = &self.s[self.i..];
        let words: [(&[u8], Unit); 12] = [
            (b"'", Unit::Ft),
            (b"\"", Unit::In),
            (b"''", Unit::In),
            (b"inches", Unit::In),
            (b"inch", Unit::In),
            (b"in", Unit::In),
            (b"feet", Unit::Ft),
            (b"foot", Unit::Ft),
            (b"ft", Unit::Ft),
            (b"mm", Unit::Mm),
            (b"cm", Unit::Cm),
            (b"m", Unit::M),
        ];
        // Longest match first so `''` beats `'` and `inches` beats `in`.
        let mut best: Option<(usize, Unit)> = None;
        for (word, unit) in words {
            if rest.starts_with(word) {
                let boundary = rest.get(word.len()).is_none_or(|c| !c.is_ascii_alphabetic());
                if boundary && best.is_none_or(|(len, _)| word.len() > len) {
                    best = Some((word.len(), unit));
                }
            }
        }
        let (len, unit) = best?;
        self.i += len;
        Some(unit)
    }
}

fn parse_length(input: &str) -> Option<Length> {
    // Typographic primes and minus signs, as pasted from drawings and spreadsheets.
    let normalised =
        input.trim().replace(['′', '’', '‘'], "'").replace(['″', '”', '“'], "\"").replace('−', "-");
    let mut c = Cursor { s: normalised.as_bytes(), i: 0 };

    c.skip_ws();
    let negative = c.peek() == Some(b'-');
    if negative {
        c.i += 1;
        c.skip_ws();
    }

    let mut total_mm = 0.0;
    let mut parts = 0;
    let mut previous: Option<Unit> = None;
    loop {
        c.skip_ws();
        if c.done() {
            break;
        }
        if parts > 0 && c.peek() == Some(b'-') {
            c.i += 1; // the dash in 30'-6"
            c.skip_ws();
        }
        let value = c.mixed()?;
        match c.unit() {
            Some(unit) => {
                // Only feet may be followed by another part (inches): 30' 6", not 1 m 20 cm or
                // 6" 30'. This grammar is mirrored by the `Length` pattern in the JSON Schema.
                if previous.is_some() && !(previous == Some(Unit::Ft) && unit == Unit::In) {
                    return None;
                }
                total_mm += value * unit.mm();
                previous = Some(unit);
            }
            None if parts == 0 => {
                c.skip_ws();
                if !c.done() {
                    return None;
                }
                let v = if negative { -value } else { value };
                return Some(Length::Bare(v));
            }
            // Architectural shorthand: 98'-6 means 98'-6".
            None if previous == Some(Unit::Ft) => {
                total_mm += value * Unit::In.mm();
                previous = Some(Unit::In);
            }
            None => return None,
        }
        parts += 1;
    }
    if parts == 0 {
        return None;
    }
    Some(Length::Mm(if negative { -total_mm } else { total_mm }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inches(s: &str) -> f64 {
        match Length::parse(s).unwrap() {
            Length::Mm(mm) => (mm / 25.4 * 1e9).round() / 1e9,
            Length::Bare(v) => panic!("{s} parsed as bare {v}"),
        }
    }

    fn mm(s: &str) -> f64 {
        Length::parse(s).unwrap().to(Units::Mm)
    }

    #[test]
    fn feet_and_inches() {
        assert_eq!(inches("98'-6\""), 98.0 * 12.0 + 6.0);
        assert_eq!(inches("98'-6"), 98.0 * 12.0 + 6.0);
        assert_eq!(inches("98' 6\""), 98.0 * 12.0 + 6.0);
        assert_eq!(inches("98'6\""), 98.0 * 12.0 + 6.0);
        assert_eq!(inches("30'"), 360.0);
        assert_eq!(inches("6 1/2\""), 6.5);
        assert_eq!(inches("1/2\""), 0.5);
        assert_eq!(inches("10' 3 3/4\""), 123.75);
        assert_eq!(inches("12 ft 6 in"), 150.0);
        assert_eq!(inches("12ft"), 144.0);
        assert_eq!(inches("2 feet 1 inch"), 25.0);
        assert_eq!(inches("98′-6″"), 1182.0);
        assert_eq!(inches("-2'-6\""), -30.0);
    }

    #[test]
    fn metric() {
        assert_eq!(mm("9.5 m"), 9500.0);
        assert_eq!(mm("950mm"), 950.0);
        assert_eq!(mm("12 cm"), 120.0);
        assert_eq!(mm("−1.2 m"), -1200.0);
    }

    #[test]
    fn bare_strings_follow_file_units() {
        assert_eq!(Length::parse("30").unwrap(), Length::Bare(30.0));
        assert_eq!(Length::parse(" 2.5 ").unwrap(), Length::Bare(2.5));
    }

    #[test]
    fn rejects_nonsense() {
        for bad in ["", "abc", "30 yd", "6\" 30'", "30 30", "1/0\"", "m", "30'-6\"x", "1 m 20 cm", "30 M"] {
            assert!(Length::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn converts_between_units() {
        assert!((Length::Mm(304.8).to(Units::Ft) - 1.0).abs() < 1e-12);
        assert_eq!(Length::Bare(3.0).to(Units::Ft), 3.0);
    }
}
