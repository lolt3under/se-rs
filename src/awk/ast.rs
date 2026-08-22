//! Abstract syntax for the awk sub-language.

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Str(String),
    Var(String),
    /// `$expr` — the field at the given (1-based) index; `$0` is the record.
    Field(Box<Expr>),
    /// `name[i, j, ...]` — associative array element.
    Index(String, Vec<Expr>),
    /// `name(args)` — builtin function call.
    Call(String, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// String concatenation (awk's juxtaposition).
    Concat(Box<Expr>, Box<Expr>),
    Logical(LogOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `key in arrayname`.
    In(Box<Expr>, String),
    Assign(AssignOp, LValue, Box<Expr>),
    /// Pre/post increment or decrement; `delta` is ±1.
    Incr {
        lvalue: LValue,
        delta: f64,
        pre: bool,
    },
}

#[derive(Clone, Debug)]
pub enum LValue {
    Var(String),
    Field(Box<Expr>),
    Index(String, Vec<Expr>),
}

#[derive(Clone, Copy, Debug)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
}

#[derive(Clone, Copy, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Clone, Copy, Debug)]
pub enum LogOp {
    And,
    Or,
}

#[derive(Clone, Copy, Debug)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    /// `print e1, e2, ...` (empty list prints `$0`).
    Print(Vec<Expr>),
    /// `printf fmt, args...`.
    Printf(Vec<Expr>),
    Expr(Expr),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        post: Option<Box<Stmt>>,
        body: Box<Stmt>,
    },
    ForIn {
        var: String,
        array: String,
        body: Box<Stmt>,
    },
    Block(Vec<Stmt>),
    /// `delete arr[i]` or `delete arr` (empty indices = whole array).
    Delete {
        array: String,
        indices: Vec<Expr>,
    },
    Break,
    Continue,
    Next,
}

/// A parsed awk program: the `BEGIN` block(s), the per-record body, and the
/// `END` block(s).
#[derive(Clone, Debug, Default)]
pub struct Program {
    pub begin: Vec<Stmt>,
    pub main: Vec<Stmt>,
    pub end: Vec<Stmt>,
}
