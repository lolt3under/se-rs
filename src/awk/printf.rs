//! A bounded `printf`/`sprintf` implementation for the awk sub-language.
//!
//! Supports `d i o u x X e E f g G c s %` with the `- + space 0 #` flags, a
//! numeric width, and a `.precision`. `*` width/precision is not supported.

use super::value::Value;
use anyhow::{Result, anyhow};

struct Spec {
    minus: bool,
    plus: bool,
    space: bool,
    zero: bool,
    hash: bool,
    width: usize,
    precision: Option<usize>,
    conv: char,
}

pub fn sprintf(fmt: &str, args: &[Value]) -> Result<String> {
    let b = fmt.as_bytes();
    let mut i = 0;
    let mut out = String::new();
    let mut argi = 0;

    while i < b.len() {
        if b[i] != b'%' {
            out.push(b[i] as char);
            i += 1;
            continue;
        }
        // parse a conversion spec
        i += 1;
        if i < b.len() && b[i] == b'%' {
            out.push('%');
            i += 1;
            continue;
        }
        let mut spec = Spec {
            minus: false,
            plus: false,
            space: false,
            zero: false,
            hash: false,
            width: 0,
            precision: None,
            conv: ' ',
        };
        // flags
        while i < b.len() {
            match b[i] {
                b'-' => spec.minus = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'0' => spec.zero = true,
                b'#' => spec.hash = true,
                _ => break,
            }
            i += 1;
        }
        // width
        while i < b.len() && b[i].is_ascii_digit() {
            spec.width = spec.width * 10 + (b[i] - b'0') as usize;
            i += 1;
        }
        // precision
        if i < b.len() && b[i] == b'.' {
            i += 1;
            let mut p = 0;
            while i < b.len() && b[i].is_ascii_digit() {
                p = p * 10 + (b[i] - b'0') as usize;
                i += 1;
            }
            spec.precision = Some(p);
        }
        if i >= b.len() {
            return Err(anyhow!("awk: truncated printf format"));
        }
        spec.conv = b[i] as char;
        i += 1;

        let arg = || -> Result<&Value> {
            args.get(argi)
                .ok_or_else(|| anyhow!("awk: not enough arguments for printf"))
        };

        let formatted = match spec.conv {
            'd' | 'i' => {
                let v = arg()?;
                argi += 1;
                fmt_int(v.num() as i64, &spec)
            }
            'o' => {
                let v = arg()?;
                argi += 1;
                fmt_radix(v.num() as i64 as u64, 8, false, &spec)
            }
            'u' => {
                let v = arg()?;
                argi += 1;
                fmt_radix(v.num() as i64 as u64, 10, false, &spec)
            }
            'x' => {
                let v = arg()?;
                argi += 1;
                fmt_radix(v.num() as i64 as u64, 16, false, &spec)
            }
            'X' => {
                let v = arg()?;
                argi += 1;
                fmt_radix(v.num() as i64 as u64, 16, true, &spec)
            }
            'c' => {
                let v = arg()?;
                argi += 1;
                let ch = match v {
                    Value::Num(n) => char::from_u32(*n as u32).unwrap_or('\u{fffd}').to_string(),
                    Value::Str(s) => s.chars().next().map(|c| c.to_string()).unwrap_or_default(),
                };
                pad(&ch, "", &spec, false)
            }
            's' => {
                let v = arg()?;
                argi += 1;
                let mut s = v.to_str("%.6g");
                if let Some(p) = spec.precision {
                    s = s.chars().take(p).collect();
                }
                pad(&s, "", &spec, false)
            }
            'f' | 'F' => {
                let v = arg()?;
                argi += 1;
                fmt_float(
                    v.num(),
                    spec.precision.unwrap_or(6),
                    &spec,
                    FloatKind::Fixed,
                )
            }
            'e' | 'E' => {
                let v = arg()?;
                argi += 1;
                let kind = FloatKind::Exp(spec.conv == 'E');
                fmt_float(v.num(), spec.precision.unwrap_or(6), &spec, kind)
            }
            'g' | 'G' => {
                let v = arg()?;
                argi += 1;
                fmt_float(
                    v.num(),
                    spec.precision.unwrap_or(6),
                    &spec,
                    FloatKind::General(spec.conv == 'G'),
                )
            }
            other => return Err(anyhow!("awk: unsupported printf conversion '%{}'", other)),
        };
        out.push_str(&formatted);
    }
    Ok(out)
}

enum FloatKind {
    Fixed,
    Exp(bool),     // uppercase?
    General(bool), // uppercase?
}

fn sign_prefix(neg: bool, spec: &Spec) -> &'static str {
    if neg {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    }
}

fn fmt_int(n: i64, spec: &Spec) -> String {
    let neg = n < 0;
    let mag = (n.unsigned_abs()).to_string();
    let body = match spec.precision {
        Some(p) if mag.len() < p => format!("{}{}", "0".repeat(p - mag.len()), mag),
        _ => mag,
    };
    pad(&body, sign_prefix(neg, spec), spec, true)
}

fn fmt_radix(n: u64, radix: u32, upper: bool, spec: &Spec) -> String {
    let mut body = match radix {
        8 => format!("{:o}", n),
        16 if upper => format!("{:X}", n),
        16 => format!("{:x}", n),
        _ => format!("{}", n),
    };
    if let Some(p) = spec.precision {
        if body.len() < p {
            body = format!("{}{}", "0".repeat(p - body.len()), body);
        }
    }
    let prefix = if spec.hash && n != 0 {
        match radix {
            16 if upper => "0X",
            16 => "0x",
            8 => "0",
            _ => "",
        }
    } else {
        ""
    };
    pad(&format!("{}{}", prefix, body), "", spec, true)
}

fn fmt_float(x: f64, prec: usize, spec: &Spec, kind: FloatKind) -> String {
    let neg = x.is_sign_negative() && (x != 0.0 || x.is_sign_negative());
    let mag = x.abs();
    let body = match kind {
        FloatKind::Fixed => format!("{:.*}", prec, mag),
        FloatKind::Exp(upper) => fmt_exp(mag, prec, upper),
        FloatKind::General(upper) => {
            // %g: use %e if exponent < -4 or >= precision, else %f; trim zeros.
            let p = prec.max(1);
            let exp = if mag == 0.0 {
                0
            } else {
                mag.abs().log10().floor() as i32
            };
            let mut s = if exp < -4 || exp >= p as i32 {
                fmt_exp(mag, p - 1, upper)
            } else {
                format!("{:.*}", (p as i32 - 1 - exp).max(0) as usize, mag)
            };
            if !spec.hash && s.contains('.') {
                // trim trailing zeros (and the dot) outside the exponent
                if let Some(epos) = s.find(['e', 'E']) {
                    let (m, e) = s.split_at(epos);
                    let m = trim_zeros(m);
                    s = format!("{}{}", m, e);
                } else {
                    s = trim_zeros(&s);
                }
            }
            s
        }
    };
    pad(&body, sign_prefix(neg, spec), spec, true)
}

fn fmt_exp(mag: f64, prec: usize, upper: bool) -> String {
    // Rust's {:e} gives "1.5e2"; normalise to C's "1.500000e+02".
    let s = format!("{:.*e}", prec, mag);
    let (mantissa, exp) = match s.split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse::<i32>().unwrap_or(0)),
        None => (s, 0),
    };
    let e = if upper { 'E' } else { 'e' };
    format!(
        "{}{}{}{:02}",
        mantissa,
        e,
        if exp < 0 { '-' } else { '+' },
        exp.abs()
    )
}

fn trim_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    t.trim_end_matches('.').to_string()
}

/// Pad `body` (with optional numeric `sign`) to the spec's width, honouring
/// left-justify (`-`) and zero-fill (`0`).
fn pad(body: &str, sign: &str, spec: &Spec, numeric: bool) -> String {
    let content_len = sign.len() + body.chars().count();
    if content_len >= spec.width {
        return format!("{}{}", sign, body);
    }
    let fill = spec.width - content_len;
    if spec.minus {
        format!("{}{}{}", sign, body, " ".repeat(fill))
    } else if spec.zero && numeric && spec.precision.is_none() {
        format!("{}{}{}", sign, "0".repeat(fill), body)
    } else {
        format!("{}{}{}", " ".repeat(fill), sign, body)
    }
}
