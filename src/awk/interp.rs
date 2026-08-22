//! Tree-walking evaluator for the awk sub-language.

use super::ast::*;
use super::printf::sprintf;
use super::value::Value;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::io::Write;

/// Non-local control flow produced by `break`/`continue`/`next`.
enum Flow {
    Normal,
    Break,
    Continue,
    Next,
}

/// Mutable interpreter state, persisting across all records of a run.
pub struct Interp {
    vars: HashMap<String, Value>,
    arrays: HashMap<String, HashMap<String, Value>>,
    nr: f64,
    field0: String,
    fields: Vec<String>,
    rng: u64,
    seed: f64,
}

impl Default for Interp {
    fn default() -> Self {
        Self::new()
    }
}

impl Interp {
    pub fn new() -> Self {
        Interp {
            vars: HashMap::new(),
            arrays: HashMap::new(),
            nr: 0.0,
            field0: String::new(),
            fields: Vec::new(),
            rng: 0x2545_F491_4F6C_DD1D,
            seed: 0.0,
        }
    }

    // -- special variables --------------------------------------------------

    fn special(&self, name: &str, default: &str) -> String {
        self.vars
            .get(name)
            .map(|v| v.to_str(self.convfmt_raw()))
            .unwrap_or_else(|| default.to_string())
    }

    fn convfmt_raw(&self) -> &str {
        "%.6g"
    }

    fn fs(&self) -> String {
        self.special("FS", " ")
    }
    fn ofs(&self) -> String {
        self.special("OFS", " ")
    }
    fn ors(&self) -> String {
        self.special("ORS", "\n")
    }
    fn subsep(&self) -> String {
        self.special("SUBSEP", "\u{1c}")
    }

    // -- record / fields ----------------------------------------------------

    /// Load a new record (one incoming view) and bump `NR`.
    pub fn set_record(&mut self, record: &[u8]) {
        let mut s = String::from_utf8_lossy(record).into_owned();
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        self.field0 = s;
        self.resplit();
        self.nr += 1.0;
    }

    fn resplit(&mut self) {
        let fs = self.fs();
        self.fields = split_record(&self.field0, &fs);
    }

    fn rebuild_record(&mut self) {
        let ofs = self.ofs();
        self.field0 = self.fields.join(&ofs);
    }

    fn get_field(&self, i: usize) -> Value {
        if i == 0 {
            Value::Str(self.field0.clone())
        } else if i <= self.fields.len() {
            Value::Str(self.fields[i - 1].clone())
        } else {
            Value::uninit()
        }
    }

    fn set_field(&mut self, i: usize, v: Value) {
        let s = v.to_str(self.convfmt_raw());
        if i == 0 {
            self.field0 = s;
            self.resplit();
            return;
        }
        if i > self.fields.len() {
            self.fields.resize(i, String::new());
        }
        self.fields[i - 1] = s;
        self.rebuild_record();
    }

    // -- program execution --------------------------------------------------

    pub fn run(&mut self, stmts: &[Stmt], out: &mut dyn Write) -> Result<()> {
        for s in stmts {
            match self.exec(s, out)? {
                Flow::Normal => {}
                Flow::Next => break,
                Flow::Break | Flow::Continue => {
                    return Err(anyhow!("awk: break/continue outside a loop"));
                }
            }
        }
        Ok(())
    }

    fn exec(&mut self, stmt: &Stmt, out: &mut dyn Write) -> Result<Flow> {
        match stmt {
            Stmt::Block(stmts) => {
                for s in stmts {
                    match self.exec(s, out)? {
                        Flow::Normal => {}
                        other => return Ok(other),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Expr(e) => {
                self.eval(e, out)?;
                Ok(Flow::Normal)
            }
            Stmt::Print(args) => {
                let ofs = self.ofs();
                let ors = self.ors();
                let mut parts = Vec::with_capacity(args.len());
                if args.is_empty() {
                    parts.push(self.field0.clone());
                } else {
                    for a in args {
                        parts.push(self.eval(a, out)?.to_str(self.convfmt_raw()));
                    }
                }
                out.write_all(parts.join(&ofs).as_bytes())?;
                out.write_all(ors.as_bytes())?;
                Ok(Flow::Normal)
            }
            Stmt::Printf(args) => {
                let fmt = self.eval(&args[0], out)?.to_str(self.convfmt_raw());
                let mut vals = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    vals.push(self.eval(a, out)?);
                }
                out.write_all(sprintf(&fmt, &vals)?.as_bytes())?;
                Ok(Flow::Normal)
            }
            Stmt::If(cond, then, els) => {
                if self.eval(cond, out)?.truthy() {
                    self.exec(then, out)
                } else if let Some(e) = els {
                    self.exec(e, out)
                } else {
                    Ok(Flow::Normal)
                }
            }
            Stmt::While(cond, body) => {
                while self.eval(cond, out)?.truthy() {
                    match self.exec(body, out)? {
                        Flow::Break => break,
                        Flow::Next => return Ok(Flow::Next),
                        _ => {}
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
            } => {
                if let Some(i) = init {
                    self.exec(i, out)?;
                }
                loop {
                    if let Some(c) = cond {
                        if !self.eval(c, out)?.truthy() {
                            break;
                        }
                    }
                    match self.exec(body, out)? {
                        Flow::Break => break,
                        Flow::Next => return Ok(Flow::Next),
                        _ => {}
                    }
                    if let Some(p) = post {
                        self.exec(p, out)?;
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::ForIn { var, array, body } => {
                let keys: Vec<String> = self
                    .arrays
                    .get(array)
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                for k in keys {
                    self.vars.insert(var.clone(), Value::Str(k));
                    match self.exec(body, out)? {
                        Flow::Break => break,
                        Flow::Next => return Ok(Flow::Next),
                        _ => {}
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Delete { array, indices } => {
                if indices.is_empty() {
                    self.arrays.remove(array);
                } else {
                    let key = self.array_key(indices, out)?;
                    if let Some(m) = self.arrays.get_mut(array) {
                        m.remove(&key);
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue => Ok(Flow::Continue),
            Stmt::Next => Ok(Flow::Next),
        }
    }

    // -- expression evaluation ---------------------------------------------

    fn eval(&mut self, e: &Expr, out: &mut dyn Write) -> Result<Value> {
        match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Var(name) => Ok(self.get_var(name)),
            Expr::Field(idx) => {
                let i = self.eval(idx, out)?.num();
                if i < 0.0 {
                    return Err(anyhow!("awk: negative field index"));
                }
                Ok(self.get_field(i as usize))
            }
            Expr::Index(name, idx) => {
                let key = self.array_key(idx, out)?;
                Ok(self
                    .arrays
                    .get(name)
                    .and_then(|m| m.get(&key))
                    .cloned()
                    .unwrap_or_else(Value::uninit))
            }
            Expr::Call(name, args) => self.call(name, args, out),
            Expr::Unary(op, x) => {
                let v = self.eval(x, out)?;
                Ok(match op {
                    UnOp::Neg => Value::Num(-v.num()),
                    UnOp::Pos => Value::Num(v.num()),
                    UnOp::Not => Value::Num(if v.truthy() { 0.0 } else { 1.0 }),
                })
            }
            Expr::Binary(op, a, b) => {
                let va = self.eval(a, out)?;
                let vb = self.eval(b, out)?;
                eval_binary(*op, va, vb)
            }
            Expr::Concat(a, b) => {
                let sa = self.eval(a, out)?.to_str(self.convfmt_raw());
                let sb = self.eval(b, out)?.to_str(self.convfmt_raw());
                Ok(Value::Str(sa + &sb))
            }
            Expr::Logical(op, a, b) => {
                let la = self.eval(a, out)?.truthy();
                let r = match op {
                    LogOp::And => la && self.eval(b, out)?.truthy(),
                    LogOp::Or => la || self.eval(b, out)?.truthy(),
                };
                Ok(Value::Num(if r { 1.0 } else { 0.0 }))
            }
            Expr::Ternary(c, a, b) => {
                if self.eval(c, out)?.truthy() {
                    self.eval(a, out)
                } else {
                    self.eval(b, out)
                }
            }
            Expr::In(key, array) => {
                let k = self.eval(key, out)?.to_str(self.convfmt_raw());
                let present = self.arrays.get(array).is_some_and(|m| m.contains_key(&k));
                Ok(Value::Num(if present { 1.0 } else { 0.0 }))
            }
            Expr::Assign(op, lv, rhs) => {
                let rv = self.eval(rhs, out)?;
                let nv = if matches!(op, AssignOp::Set) {
                    rv
                } else {
                    let cur = self.load_lvalue(lv, out)?.num();
                    let r = rv.num();
                    Value::Num(match op {
                        AssignOp::Add => cur + r,
                        AssignOp::Sub => cur - r,
                        AssignOp::Mul => cur * r,
                        AssignOp::Div => {
                            if r == 0.0 {
                                return Err(anyhow!("division by zero"));
                            }
                            cur / r
                        }
                        AssignOp::Mod => {
                            if r == 0.0 {
                                return Err(anyhow!("division by zero in %"));
                            }
                            cur % r
                        }
                        AssignOp::Pow => cur.powf(r),
                        AssignOp::Set => unreachable!(),
                    })
                };
                self.store_lvalue(lv, nv.clone(), out)?;
                Ok(nv)
            }
            Expr::Incr { lvalue, delta, pre } => {
                let old = self.load_lvalue(lvalue, out)?.num();
                let new = Value::Num(old + delta);
                self.store_lvalue(lvalue, new.clone(), out)?;
                Ok(if *pre { new } else { Value::Num(old) })
            }
        }
    }

    fn get_var(&self, name: &str) -> Value {
        match name {
            "NR" => Value::Num(self.nr),
            "NF" => Value::Num(self.fields.len() as f64),
            _ => self.vars.get(name).cloned().unwrap_or_else(Value::uninit),
        }
    }

    fn load_lvalue(&mut self, lv: &LValue, out: &mut dyn Write) -> Result<Value> {
        match lv {
            LValue::Var(name) => Ok(self.get_var(name)),
            LValue::Field(idx) => {
                let i = self.eval(idx, out)?.num();
                Ok(self.get_field(i.max(0.0) as usize))
            }
            LValue::Index(name, idx) => {
                let key = self.array_key(idx, out)?;
                Ok(self
                    .arrays
                    .get(name)
                    .and_then(|m| m.get(&key))
                    .cloned()
                    .unwrap_or_else(Value::uninit))
            }
        }
    }

    fn store_lvalue(&mut self, lv: &LValue, v: Value, out: &mut dyn Write) -> Result<()> {
        match lv {
            LValue::Var(name) => {
                if name == "NF" {
                    let n = v.num().max(0.0) as usize;
                    self.fields.resize(n, String::new());
                    self.rebuild_record();
                } else {
                    self.vars.insert(name.clone(), v);
                }
                Ok(())
            }
            LValue::Field(idx) => {
                let i = self.eval(idx, out)?.num();
                if i < 0.0 {
                    return Err(anyhow!("awk: negative field index"));
                }
                self.set_field(i as usize, v);
                Ok(())
            }
            LValue::Index(name, idx) => {
                let key = self.array_key(idx, out)?;
                self.arrays.entry(name.clone()).or_default().insert(key, v);
                Ok(())
            }
        }
    }

    fn array_key(&mut self, idx: &[Expr], out: &mut dyn Write) -> Result<String> {
        let subsep = self.subsep();
        let mut parts = Vec::with_capacity(idx.len());
        for e in idx {
            parts.push(self.eval(e, out)?.to_str(self.convfmt_raw()));
        }
        Ok(parts.join(&subsep))
    }

    // -- builtin functions --------------------------------------------------

    fn call(&mut self, name: &str, args: &[Expr], out: &mut dyn Write) -> Result<Value> {
        let arity_err = || anyhow!("awk: wrong number of arguments to {}()", name);
        match name {
            "sin" | "cos" | "exp" | "log" | "sqrt" | "int" => {
                if args.len() != 1 {
                    return Err(arity_err());
                }
                let x = self.eval(&args[0], out)?.num();
                Ok(Value::Num(match name {
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "exp" => x.exp(),
                    "log" => x.ln(),
                    "sqrt" => x.sqrt(),
                    "int" => x.trunc(),
                    _ => unreachable!(),
                }))
            }
            "atan2" => {
                if args.len() != 2 {
                    return Err(arity_err());
                }
                let y = self.eval(&args[0], out)?.num();
                let x = self.eval(&args[1], out)?.num();
                Ok(Value::Num(y.atan2(x)))
            }
            "rand" => {
                if !args.is_empty() {
                    return Err(arity_err());
                }
                Ok(Value::Num(self.next_rand()))
            }
            "srand" => {
                let prev = self.seed;
                let new = if args.is_empty() {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as f64)
                        .unwrap_or(0.0)
                } else {
                    self.eval(&args[0], out)?.num()
                };
                self.seed = new;
                self.rng = (new.to_bits()) ^ 0x2545_F491_4F6C_DD1D;
                if self.rng == 0 {
                    self.rng = 0x9E37_79B9_7F4A_7C15;
                }
                Ok(Value::Num(prev))
            }
            "length" => {
                let s = if args.is_empty() {
                    self.field0.clone()
                } else {
                    self.eval(&args[0], out)?.to_str(self.convfmt_raw())
                };
                Ok(Value::Num(s.chars().count() as f64))
            }
            "substr" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(arity_err());
                }
                let s: Vec<char> = self
                    .eval(&args[0], out)?
                    .to_str(self.convfmt_raw())
                    .chars()
                    .collect();
                let m = self.eval(&args[1], out)?.num();
                let start = (m.trunc() as i64).max(1) as usize; // 1-based
                let len = s.len();
                let from = start.saturating_sub(1);
                let take = if args.len() == 3 {
                    let n = self.eval(&args[2], out)?.num();
                    // characters from position m for n chars, clamped
                    let end = (m + n - 1.0).trunc() as i64;
                    (end.max(0) as usize).min(len).saturating_sub(from)
                } else {
                    len.saturating_sub(from)
                };
                Ok(Value::Str(s.iter().skip(from).take(take).collect()))
            }
            "index" => {
                if args.len() != 2 {
                    return Err(arity_err());
                }
                let s = self.eval(&args[0], out)?.to_str(self.convfmt_raw());
                let t = self.eval(&args[1], out)?.to_str(self.convfmt_raw());
                let pos = match s.find(&t) {
                    Some(byte_idx) => s[..byte_idx].chars().count() + 1,
                    None => 0,
                };
                Ok(Value::Num(pos as f64))
            }
            "split" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(arity_err());
                }
                let s = self.eval(&args[0], out)?.to_str(self.convfmt_raw());
                let arr_name = match &args[1] {
                    Expr::Var(n) => n.clone(),
                    _ => return Err(anyhow!("awk: split() 2nd argument must be an array name")),
                };
                let fs = if args.len() == 3 {
                    self.eval(&args[2], out)?.to_str(self.convfmt_raw())
                } else {
                    self.fs()
                };
                let parts = split_record(&s, &fs);
                let map: HashMap<String, Value> = parts
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ((i + 1).to_string(), Value::Str(p.clone())))
                    .collect();
                let n = map.len();
                self.arrays.insert(arr_name, map);
                Ok(Value::Num(n as f64))
            }
            "tolower" => {
                if args.len() != 1 {
                    return Err(arity_err());
                }
                Ok(Value::Str(
                    self.eval(&args[0], out)?
                        .to_str(self.convfmt_raw())
                        .to_lowercase(),
                ))
            }
            "toupper" => {
                if args.len() != 1 {
                    return Err(arity_err());
                }
                Ok(Value::Str(
                    self.eval(&args[0], out)?
                        .to_str(self.convfmt_raw())
                        .to_uppercase(),
                ))
            }
            "sprintf" => {
                if args.is_empty() {
                    return Err(arity_err());
                }
                let fmt = self.eval(&args[0], out)?.to_str(self.convfmt_raw());
                let mut vals = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    vals.push(self.eval(a, out)?);
                }
                Ok(Value::Str(sprintf(&fmt, &vals)?))
            }
            _ => Err(anyhow!("awk: unknown function '{}'", name)),
        }
    }

    /// xorshift64* — deterministic without `srand`, matching awk's fixed default
    /// sequence; returns a value in [0, 1).
    fn next_rand(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // top 53 bits → [0,1)
        (r >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Split a record into fields per awk's `FS` rules: `" "` (default) splits on
/// runs of whitespace with leading/trailing trimmed; a single character splits
/// on each occurrence (keeping empties); any longer `FS` is a literal separator.
fn split_record(s: &str, fs: &str) -> Vec<String> {
    if fs == " " {
        s.split_whitespace().map(|w| w.to_string()).collect()
    } else if fs.is_empty() {
        s.chars().map(|c| c.to_string()).collect()
    } else if fs.chars().count() == 1 {
        let c = fs.chars().next().unwrap();
        if s.is_empty() {
            Vec::new()
        } else {
            s.split(c).map(|w| w.to_string()).collect()
        }
    } else if s.is_empty() {
        Vec::new()
    } else {
        s.split(fs).map(|w| w.to_string()).collect()
    }
}

fn eval_binary(op: BinOp, a: Value, b: Value) -> Result<Value> {
    use BinOp::*;
    Ok(match op {
        Add => Value::Num(a.num() + b.num()),
        Sub => Value::Num(a.num() - b.num()),
        Mul => Value::Num(a.num() * b.num()),
        Div => {
            let d = b.num();
            if d == 0.0 {
                return Err(anyhow!("division by zero"));
            }
            Value::Num(a.num() / d)
        }
        Mod => {
            let d = b.num();
            if d == 0.0 {
                return Err(anyhow!("division by zero in %"));
            }
            Value::Num(a.num() % d)
        }
        Pow => Value::Num(a.num().powf(b.num())),
        Lt | Le | Gt | Ge | Eq | Ne => {
            let cmp = compare(&a, &b);
            let r = match op {
                Lt => cmp < 0,
                Le => cmp <= 0,
                Gt => cmp > 0,
                Ge => cmp >= 0,
                Eq => cmp == 0,
                Ne => cmp != 0,
                _ => unreachable!(),
            };
            Value::Num(if r { 1.0 } else { 0.0 })
        }
    })
}

/// Awk comparison: numeric if both operands are numeric (a number or a numeric
/// string), otherwise string comparison. Returns -1/0/1.
fn compare(a: &Value, b: &Value) -> i32 {
    if a.is_numeric() && b.is_numeric() {
        let (x, y) = (a.num(), b.num());
        if x < y {
            -1
        } else if x > y {
            1
        } else {
            0
        }
    } else {
        let (x, y) = (a.to_str("%.6g"), b.to_str("%.6g"));
        match x.cmp(&y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}
