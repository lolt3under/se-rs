//! The awk scalar value: a number or a string, with awk's coercion rules.

/// An awk scalar. Awk has exactly one scalar type that behaves as a number or a
/// string depending on context; `Value` models that with explicit coercions.
#[derive(Clone, Debug)]
pub enum Value {
    Num(f64),
    Str(String),
}

impl Value {
    /// The uninitialised value (`""`, which is also numeric 0).
    pub fn uninit() -> Value {
        Value::Str(String::new())
    }

    /// Numeric coercion: a number is itself; a string contributes its leading
    /// numeric prefix (strtod-style), or 0 if it has none.
    pub fn num(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::Str(s) => num_prefix(s),
        }
    }

    /// String coercion using `convfmt` for non-integral numbers.
    pub fn to_str(&self, convfmt: &str) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Num(n) => fmt_num(*n, convfmt),
        }
    }

    /// Awk truth: a number is true iff non-zero; a string is true iff non-empty,
    /// except a string that is wholly numeric uses its numeric truth (so the
    /// field value "0" is false, matching awk).
    pub fn truthy(&self) -> bool {
        match self {
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => match full_num(s) {
                Some(n) => n != 0.0,
                None => !s.is_empty(),
            },
        }
    }

    /// Whether this value should compare numerically (a number, or a string that
    /// is entirely a number).
    pub fn is_numeric(&self) -> bool {
        match self {
            Value::Num(_) => true,
            Value::Str(s) => full_num(s).is_some(),
        }
    }
}

/// Format a number the way awk prints one: integral values print without a
/// decimal point; others use ~6 significant digits (`%.6g`-ish).
pub fn fmt_num(n: f64, _convfmt: &str) -> String {
    if n.is_nan() {
        return "nan".to_string();
    }
    if n.is_infinite() {
        return if n < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if n == n.trunc() && n.abs() < 1e16 {
        return format!("{}", n as i64);
    }
    // Approximate %.6g: six significant digits, trailing zeros trimmed.
    let mut s = format!("{:.6}", n);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// Parse the longest leading numeric prefix of `s` as f64 (0.0 if none),
/// mirroring awk's string→number coercion.
fn num_prefix(s: &str) -> f64 {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let mut saw_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        saw_digit = true;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            saw_digit = true;
        }
    }
    if saw_digit && i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let mut exp_digit = false;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
            exp_digit = true;
        }
        if exp_digit {
            i = j;
        }
    }
    if !saw_digit {
        return 0.0;
    }
    t[..i].parse::<f64>().unwrap_or(0.0)
}

/// `Some(n)` iff the entire trimmed string is a valid number.
fn full_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}
