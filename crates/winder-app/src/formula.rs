//! Formulas in number fields: type `channel / od * 0.7` into turns per pass,
//! or `5.8 * 1.05` into the DCR, and the field takes the result. Names stand
//! for the design as it is when you press Enter.

use std::cell::RefCell;
use winder_core::Design;

thread_local! {
    static VARS: RefCell<Vec<(&'static str, f64, &'static str)>> = const { RefCell::new(Vec::new()) };
}

/// Make the design's sizes available to formulas typed this frame.
pub fn set_design(d: &Design) {
    let od = d.wire_spec().map_or(0.071, |w| w.od_mm);
    let b = &d.bobbin;
    let vars = vec![
        ("channel", b.channel_height_mm, "flange to flange (mm)"),
        ("core_w", b.core_width_mm, "core width (mm)"),
        ("core_l", b.core_length_mm, "core length (mm)"),
        ("depth", b.build_depth_mm, "build depth (mm)"),
        ("od", od, "wire diameter with insulation (mm)"),
        (
            "side",
            ((b.channel_height_mm - od) / od).max(1.0),
            "turns side by side across the channel",
        ),
        ("turns", d.total_turns() as f64, "total turns"),
        ("rpm", d.speed.rpm, "spindle speed"),
        ("pi", std::f64::consts::PI, "π"),
    ];
    VARS.with(|v| *v.borrow_mut() = vars);
}

/// The names a formula can use, with what they stand for and their value.
pub fn names() -> Vec<(&'static str, f64, &'static str)> {
    VARS.with(|v| v.borrow().clone())
}

/// What a number field reads: a plain number or a formula. Units typed
/// after it (mm, kΩ, %…) are ignored, as the field shows them.
pub fn parse(text: &str) -> Option<f64> {
    let mut t = text.trim();
    // A unit after a number, as the field shows it ("12 mm"), but not a
    // name ("turns" alone).
    for unit in [
        "of a turn",
        "turns",
        "RPM",
        "kΩ",
        "Ω/m",
        "mm",
        "°C",
        "pF",
        "kHz",
        "%",
        "g",
        "×",
    ] {
        if let Some(rest) = t.strip_suffix(unit).map(str::trim_end)
            && rest.ends_with(|c: char| c.is_ascii_digit() || c == ')' || c == '.')
        {
            t = rest;
            break;
        }
    }
    let vars = names();
    let mut p = Parser {
        s: t.as_bytes(),
        i: 0,
        vars: &vars,
    };
    let v = p.sum()?;
    p.ws();
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    vars: &'a [(&'static str, f64, &'static str)],
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        loop {
            if self.eat(b'+') {
                v += self.product()?;
            } else if self.eat(b'-') {
                v -= self.product()?;
            } else {
                return Some(v);
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                v /= self.unary()?;
            } else {
                return Some(v);
            }
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.eat(b'^') {
            // Right-associative (2^3^2 = 2^9), and above a sign (-2^2 = -4).
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.power()
    }

    fn atom(&mut self) -> Option<f64> {
        self.ws();
        if self.eat(b'(') {
            let v = self.sum()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        let c = *self.s.get(self.i)?;
        if c.is_ascii_digit() || c == b'.' {
            while self
                .s
                .get(self.i)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            {
                self.i += 1;
            }
            // 1e-3
            if self.s.get(self.i).is_some_and(|c| *c == b'e' || *c == b'E')
                && self
                    .s
                    .get(self.i + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == b'-' || *c == b'+')
            {
                self.i += 2;
                while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            return std::str::from_utf8(&self.s[start..self.i])
                .ok()?
                .parse()
                .ok();
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while self
                .s
                .get(self.i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                self.i += 1;
            }
            let name = std::str::from_utf8(&self.s[start..self.i]).ok()?;
            if self.eat(b'(') {
                let mut args = vec![self.sum()?];
                while self.eat(b',') {
                    args.push(self.sum()?);
                }
                if !self.eat(b')') {
                    return None;
                }
                return match (name, args.as_slice()) {
                    ("sqrt", [a]) => Some(a.sqrt()),
                    ("round", [a]) => Some(a.round()),
                    ("floor", [a]) => Some(a.floor()),
                    ("ceil", [a]) => Some(a.ceil()),
                    ("abs", [a]) => Some(a.abs()),
                    ("min", [a, b]) => Some(a.min(*b)),
                    ("max", [a, b]) => Some(a.max(*b)),
                    _ => None,
                };
            }
            return self
                .vars
                .iter()
                .find(|(n, _, _)| *n == name)
                .map(|(_, v, _)| *v);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_formulas() {
        set_design(&Design::example("strat").unwrap());
        assert_eq!(parse("12.5"), Some(12.5));
        assert_eq!(parse("12.5 mm"), Some(12.5));
        assert_eq!(parse("2 + 3 * 4"), Some(14.0));
        assert_eq!(parse("(2 + 3) * 4"), Some(20.0));
        assert_eq!(parse("-2^2"), Some(-4.0));
        assert_eq!(parse("2^3^2"), Some(512.0));
        assert_eq!(parse("max(3, 7) / 2"), Some(3.5));
        assert_eq!(parse("1e-3 * 1000"), Some(1.0));
        let side = (11.0 - 0.071) / 0.071;
        assert!((parse("side * 0.7").unwrap() - side * 0.7).abs() < 1e-9);
        assert!((parse("channel / od").unwrap() - 11.0 / 0.071).abs() < 1e-9);
        assert_eq!(parse("round(turns / 1000)"), Some(8.0));
        assert_eq!(parse("turns"), Some(8000.0));
        assert_eq!(parse("8000 turns"), Some(8000.0));
        assert_eq!(parse("35 %"), Some(35.0));
        assert_eq!(parse("2 +"), None);
        assert_eq!(parse("nope * 2"), None);
        assert_eq!(parse("1 / 0"), None);
    }
}
