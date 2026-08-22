//! The awk interpreter used by `@{ ... }`.
//!
//! Each incoming view is one record. A line-oriented caller selects lines
//! before the action:
//!
//! ```text
//! se 'x/.*\n?/ @{ sum += $1 END { print sum } }' nums.txt
//! ```
//!
//! Supported: `$0..$NF`, `NR`, `NF`, `FS`/`OFS`/`ORS`/`SUBSEP`; arithmetic,
//! comparison, `&&`/`||`/`!`, ternary, assignment (incl. `+= -= *= /= %= ^=`),
//! `++`/`--`, string concatenation; `if/else`, `while`, `for(;;)`,
//! `for(k in a)`, `delete`, `break`/`continue`/`next`; `print`/`printf`;
//! associative arrays; and builtins `sin cos atan2 exp log sqrt int rand srand
//! length substr index split sprintf tolower toupper`. `BEGIN { … }` and
//! `END { … }` blocks run before the first and after the last record.
//!
//! Regex literals, `~`/`!~`, `getline`, output redirection, and user-defined
//! functions are not implemented. Matching belongs in the surrounding
//! structural pipeline.

mod ast;
mod interp;
mod lexer;
mod parser;
mod printf;
mod value;

pub use ast::Program;
pub use interp::Interp;

use anyhow::Result;
use std::io::Write;

impl Program {
    /// Parse awk source (the text inside `@{ … }`).
    pub fn parse(src: &str) -> Result<Program> {
        parser::parse_program(src)
    }

    /// Run the `BEGIN` block(s) once, before any record.
    pub fn run_begin(&self, ip: &mut Interp, out: &mut dyn Write) -> Result<()> {
        ip.run(&self.begin, out)
    }

    /// Load `record` as the current record and run the per-record body.
    pub fn run_record(&self, ip: &mut Interp, record: &[u8], out: &mut dyn Write) -> Result<()> {
        ip.set_record(record);
        ip.run(&self.main, out)
    }

    /// Run the `END` block(s) once, after the last record.
    pub fn run_end(&self, ip: &mut Interp, out: &mut dyn Write) -> Result<()> {
        ip.run(&self.end, out)
    }
}

#[cfg(test)]
mod tests {
    use super::{Interp, Program};

    /// Run `src` over whitespace-separated `records`, returning captured stdout.
    fn run(src: &str, records: &[&str]) -> String {
        let prog = Program::parse(src).expect("parse");
        let mut ip = Interp::new();
        let mut out: Vec<u8> = Vec::new();
        prog.run_begin(&mut ip, &mut out).unwrap();
        for r in records {
            prog.run_record(&mut ip, r.as_bytes(), &mut out).unwrap();
        }
        prog.run_end(&mut ip, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn fields_and_nr_nf() {
        assert_eq!(run("{ print NR, NF, $1, $3 }", &["a b c"]), "1 3 a c\n");
    }

    #[test]
    fn arithmetic_and_sum_with_end() {
        assert_eq!(
            run("{ s += $1 } END { print s }", &["10", "20", "12"]),
            "42\n"
        );
    }

    #[test]
    fn begin_block_runs_first() {
        assert_eq!(
            run("BEGIN{ print \"start\" } { print $1 }", &["x"]),
            "start\nx\n"
        );
    }

    #[test]
    fn math_builtins() {
        assert_eq!(
            run("BEGIN{ print sqrt(16), int(3.9), log(1) }", &[]),
            "4 3 0\n"
        );
    }

    #[test]
    fn associative_array_and_for_in() {
        // Count word frequencies, then print the count for "a".
        let out = run(
            "{ c[$1]++ } END { print c[\"a\"], c[\"b\"] }",
            &["a", "b", "a", "a"],
        );
        assert_eq!(out, "3 1\n");
    }

    #[test]
    fn for_in_iterates_all_keys() {
        let out = run(
            "{ seen[$1]=1 } END { n=0; for (k in seen) n++; print n }",
            &["x", "y", "x"],
        );
        assert_eq!(out, "2\n");
    }

    #[test]
    fn string_builtins_and_concat() {
        assert_eq!(
            run("{ print toupper($1) \"=\" length($1) }", &["abc"]),
            "ABC=3\n"
        );
        assert_eq!(run("{ print substr($1, 2, 3) }", &["abcdef"]), "bcd\n");
    }

    #[test]
    fn printf_formats() {
        assert_eq!(
            run(
                "{ printf \"%5.2f|%-3s|%03d\\n\", $1, $2, $3 }",
                &["3.14159 hi 7"]
            ),
            " 3.14|hi |007\n"
        );
    }

    #[test]
    fn control_flow_if_ternary() {
        assert_eq!(
            run("{ print ($1 > 5 ? \"big\" : \"small\") }", &["8", "2"]),
            "big\nsmall\n"
        );
    }

    #[test]
    fn fs_split() {
        assert_eq!(run("BEGIN{FS=\":\"} { print $2 }", &["a:b:c"]), "b\n");
    }
}
