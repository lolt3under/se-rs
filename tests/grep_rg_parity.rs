//! Live-derived grep / ripgrep parity regression tests.
//!
//! Every expected value in this file was produced by running the REAL reference
//! tool (BSD `/usr/bin/grep` or ripgrep 15.2.0) on the given input and capturing
//! its bytes. See `tests/scripts/compat_live.sh` for the interactive harness and
//! `/tmp/gen_cases.py` (dev-only) for how these were generated. They are
//! therefore reference-derived, byte-exact, and non-tautological: they assert
//! that `se`'s equivalent program matches what grep/ripgrep actually output,
//! trailing newline included.
//!
//! Distinct from `tests/compatibility.rs`, whose expected values are pinned to
//! specific upstream GNU/ripgrep commit snapshots. This file tracks the locally
//! installed tools and specifically guards the fused line-filter fast paths
//! (`LineFilterCommand` for case-sensitive literals, `RegexLineFilterCommand`
//! for case-insensitive literals) against regressions.

use std::io::Write;
use std::process::{Command, Output, Stdio};

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum Family {
    Grep,
    Ripgrep,
}

#[derive(Clone, Copy)]
struct Case {
    id: &'static str,
    #[allow(dead_code)]
    family: Family,
    #[allow(dead_code)]
    origin: &'static str,
    program: &'static str,
    input: &'static [u8],
    expected_stdout: &'static [u8],
}

const fn ok(
    id: &'static str,
    family: Family,
    origin: &'static str,
    program: &'static str,
    input: &'static [u8],
    expected_stdout: &'static [u8],
) -> Case {
    Case {
        id,
        family,
        origin,
        program,
        input,
        expected_stdout,
    }
}

// All expected_stdout values below were captured from the real reference tool.
static CASES: &[Case] = &[
    ok(
        "gi.multi_match_dedup",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/ab/i p",
        b"AB ab Ab\nno\n",
        b"AB ab Ab\n",
    ),
    ok(
        "gi.blank_lines",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/hit/i p",
        b"a\n\nHIT\n\nb\n",
        b"HIT\n",
    ),
    ok(
        "gi.match_at_start",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/err/i p",
        b"ERR x\nyy\n",
        b"ERR x\n",
    ),
    ok(
        "gi.match_at_end",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/err/i p",
        b"x ERR\nyy\n",
        b"x ERR\n",
    ),
    ok(
        "gi.all_lines_match",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/z/i p",
        b"Za\nzB\nZZ\n",
        b"Za\nzB\nZZ\n",
    ),
    ok(
        "gi.no_lines_match",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/q/i p",
        b"aa\nbb\n",
        b"",
    ),
    ok(
        "gi.mixed_multiline",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/warn/i p",
        b"INFO a\nWARN b\ninfo c\nWarn d\n",
        b"WARN b\nWarn d\n",
    ),
    ok(
        "gi.unicode_fold_e",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/café/i p",
        b"caf\xc3\xa9\nCAF\xc3\x89\ntea\n",
        b"caf\xc3\xa9\nCAF\xc3\x89\n",
    ),
    ok(
        "gi.unicode_kelvin",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/kelvin/i p",
        b"Kelvin\nkelvin\n",
        b"Kelvin\nkelvin\n",
    ),
    ok(
        "gi.unterminated_match",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/err/i p",
        b"ok\nlast ERR",
        b"last ERR\n",
    ),
    ok(
        "gi.terminated_line_extract",
        Family::Grep,
        "rg -i",
        r"x/.*\n/ g/foo/i p",
        b"FOO\nbar\nfoObar\n",
        b"FOO\nfoObar\n",
    ),
    ok(
        "gi.regex_dot_perline",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/er.or/i p",
        b"ERROR\nerbor\nno\n",
        b"ERROR\nerbor\n",
    ),
    ok(
        "gi.regex_class_perline",
        Family::Grep,
        "rg -i",
        r"x/.*\n?/ g/a[0-9]b/i p",
        b"A5B\nxx\na9b\n",
        b"A5B\na9b\n",
    ),
    ok(
        "vi.invert_ci",
        Family::Grep,
        "rg -iv",
        r"x/.*\n?/ v/skip/i p",
        b"keep\nSKIP this\nKeep2\n",
        b"keep\nKeep2\n",
    ),
    ok(
        "gp.literal",
        Family::Grep,
        "grep",
        r"x/.*\n?/ g/needle/ p",
        b"hay\nneedle\nstraw\n",
        b"needle\n",
    ),
    ok(
        "gp.class_digit",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/[0-9]+/ p",
        b"abc\na12b\nxyz\n",
        b"a12b\n",
    ),
    ok(
        "gp.anchor_start",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/^err/ p",
        b"err a\nx err\nerrb\n",
        b"err a\nerrb\n",
    ),
    ok(
        "gp.anchor_end",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/end$/ p",
        b"the end\nend x\nno\n",
        b"the end\n",
    ),
    ok(
        "gp.alternation",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/cat|dog/ p",
        b"cat\nfox\ndog\n",
        b"cat\ndog\n",
    ),
    ok(
        "gp.word",
        Family::Grep,
        "grep -w",
        r"x/.*\n?/ g/\bcat\b/ p",
        b"cat\nscatter\ncat!\n",
        b"cat\ncat!\n",
    ),
    ok(
        "gp.invert",
        Family::Grep,
        "grep -v",
        r"x/.*\n?/ v/drop/ p",
        b"keep\ndrop\nkeep2\n",
        b"keep\nkeep2\n",
    ),
    ok(
        "gp.only_match",
        Family::Grep,
        "grep -Eo",
        r"x/[0-9]+/ p",
        b"a12 b345 c6\n",
        b"12\n345\n6\n",
    ),
    ok(
        "rp.digit_shorthand",
        Family::Ripgrep,
        "rg",
        r"x/.*\n?/ g/\d+/ p",
        b"abc\na12b\n",
        b"a12b\n",
    ),
    ok(
        "rp.word_shorthand",
        Family::Ripgrep,
        "rg -o",
        r"x/\w+/ p",
        b"foo, bar.baz\n",
        b"foo\nbar\nbaz\n",
    ),
    ok(
        "rp.only_match_multi",
        Family::Ripgrep,
        "rg -o",
        r"x/a[0-9]/ p",
        b"a1 a2 a3\n",
        b"a1\na2\na3\n",
    ),
    ok(
        "rp.anchored",
        Family::Ripgrep,
        "rg",
        r"x/.*\n?/ g/^foo/ p",
        b"foo\nafoo\nfoobar\n",
        b"foo\nfoobar\n",
    ),
    ok(
        "rp.alternation",
        Family::Ripgrep,
        "rg",
        r"x/.*\n?/ g/foo|bar/ p",
        b"foo\nqux\nbar\n",
        b"foo\nbar\n",
    ),
    ok(
        "gp.interval",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/ab{2,3}c/ p",
        b"abc\nabbc\nabbbc\nabbbbc\n",
        b"abbc\nabbbc\n",
    ),
    ok(
        "gp.posix_digit",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/[[:digit:]]+/ p",
        b"abc\na12b\nxyz\n",
        b"a12b\n",
    ),
    ok(
        "gp.posix_alpha",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/[[:alpha:]]+/ p",
        b"123\nab12\n456\n",
        b"ab12\n",
    ),
    ok(
        "gp.negated_class",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/a[^0-9]c/ p",
        b"abc\na5c\nazc\n",
        b"abc\nazc\n",
    ),
    ok(
        "gp.escaped_dot",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/a\.c/ p",
        b"abc\na.c\naxc\n",
        b"a.c\n",
    ),
    ok(
        "gp.star",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/ab*c/ p",
        b"ac\nabc\nabbc\nadc\n",
        b"ac\nabc\nabbc\n",
    ),
    ok(
        "gp.plus",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/ab+c/ p",
        b"ac\nabc\nabbc\n",
        b"abc\nabbc\n",
    ),
    ok(
        "gp.question",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/ab?c/ p",
        b"ac\nabc\nabbc\n",
        b"ac\nabc\n",
    ),
    ok(
        "gp.range",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/a[1-3]c/ p",
        b"a0c\na2c\na4c\n",
        b"a2c\n",
    ),
    ok(
        "gp.dot_any",
        Family::Grep,
        "grep -E",
        r"x/.*\n?/ g/a.c/ p",
        b"ac\nabc\na-c\n",
        b"abc\na-c\n",
    ),
    ok(
        "gp.multiple_hits_line",
        Family::Grep,
        "grep",
        r"x/.*\n?/ g/the/ p",
        b"the cat and the dog\nnope\n",
        b"the cat and the dog\n",
    ),
    ok(
        "gp.tabs",
        Family::Grep,
        "grep -P?",
        r"x/.*\n?/ g/x\ty/ p",
        b"x\ty\nxy\n",
        b"x\ty\n",
    ),
    ok(
        "rp.interval",
        Family::Ripgrep,
        "rg",
        r"x/.*\n?/ g/ab{2}c/ p",
        b"abc\nabbc\nabbbc\n",
        b"abbc\n",
    ),
    ok(
        "rp.unicode_prop",
        Family::Ripgrep,
        "rg -o",
        r"x/\p{Greek}+/ p",
        b"latin\n\xce\xb1\xce\xb2\xce\xb3\n",
        b"\xce\xb1\xce\xb2\xce\xb3\n",
    ),
    ok(
        "rp.negated_class",
        Family::Ripgrep,
        "rg -o",
        r"x/[^ ]+/ p",
        b"one two three\n",
        b"one\ntwo\nthree\n",
    ),
    ok(
        "rp.crlf_line",
        Family::Ripgrep,
        "rg",
        r"x/.*\n?/ g/foo/ p",
        b"foo\r\nbar\r\n",
        b"foo\r\n",
    ),
    ok(
        "rp.greedy",
        Family::Ripgrep,
        "rg -o",
        r"x/a.*c/ p",
        b"a1c2c\naXc\n",
        b"a1c2c\naXc\n",
    ),
    ok(
        "sp.global",
        Family::Ripgrep,
        "rg -r",
        r"s/foo/bar/g",
        b"foo foo\nfoo\n",
        b"bar bar\nbar\n",
    ),
    ok(
        "sp.ci_global",
        Family::Ripgrep,
        "rg -ir",
        r"s/foo/X/gi",
        b"FOO foo Foo\n",
        b"X X X\n",
    ),
    ok(
        "sp.capture",
        Family::Ripgrep,
        "rg -r",
        r"s/(\w+)=(\w+)/$2=$1/g",
        b"a=1 b=2\n",
        b"1=a 2=b\n",
    ),
];

fn bytes(value: &[u8]) -> String {
    match std::str::from_utf8(value) {
        Ok(text) => format!("{text:?}"),
        Err(_) => value
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn run(case: &Case) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_se"))
        .arg(case.program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("{}: spawn se: {e}", case.id));
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(case.input)
        .unwrap_or_else(|e| panic!("{}: write stdin: {e}", case.id));
    child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("{}: wait se: {e}", case.id))
}

#[test]
fn grep_ripgrep_parity() {
    let mut failures = Vec::new();
    for case in CASES {
        let out = run(case);
        if !out.status.success() {
            failures.push(format!(
                "{}: se exited non-zero (code {:?})\n  program: {:?}\n  stderr: {}",
                case.id,
                out.status.code(),
                case.program,
                String::from_utf8_lossy(&out.stderr),
            ));
            continue;
        }
        if out.stdout != case.expected_stdout {
            failures.push(format!(
                "{} [{:?}; ref {}]\n  program: {:?}\n  input:    {}\n  expected: {}\n  actual:   {}",
                case.id,
                case.family,
                case.origin,
                case.program,
                bytes(case.input),
                bytes(case.expected_stdout),
                bytes(&out.stdout),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}/{} grep/ripgrep parity cases regressed:\n\n{}",
        failures.len(),
        CASES.len(),
        failures.join("\n\n"),
    );
}

fn stdout_of(program: &str, input: &[u8]) -> Vec<u8> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_se"))
        .arg(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn se");
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "se failed on {program:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// The line-filter optimizer (`parser::optimize`) is a behaviour-preserving
/// rewrite: the fused fast path (top-level `x/.*\n/ g|v/lit/`) must be
/// byte-identical to the un-fused per-line pipeline it replaces. Wrapping the
/// same program in `{ … }` suppresses fusion (the optimizer only rewrites the
/// top level), so it is a self-contained differential reference — no external
/// tool needed. Exercises all fused paths: NEON literal `g`/`v`
/// (`LineFilterCommand`) and case-insensitive literal `g`
/// (`RegexLineFilterCommand`), plus the un-fused regex/invert paths.
#[test]
fn fused_line_filter_equals_unfused() {
    let filters = [
        "g/ERROR/",  // case-sensitive literal -> fused NEON g
        "v/ERROR/",  // case-sensitive literal -> fused NEON v (invert)
        "g/error/i", // case-insensitive literal -> fused RegexLineFilterCommand
        "v/error/i", // case-insensitive literal invert -> stays per-line
        "g/err.r/i", // regex (has `.`) -> stays per-line
        "g/a/i",     // dense single-char literal -> fused, many matches/line
    ];
    let inputs: &[&[u8]] = &[
        b"Error\nplain\nERROR\nerror\n",
        b"a\n\nERROR\n\nb\n",             // blank lines
        b"no match at all here\nstill\n", // nothing matches
        b"ALL a A aA Aa\naAaA\nAAAA\n",   // every line matches, many per line
        b"one ERROR two ERROR end\nsolo\n",
        b"trailing no newline ERROR",          // unterminated final line
        b"",                                   // empty input
        b"unicode \xc3\x89rror line\nplain\n", // "Érror" (U+00C9) matches g/error/i
    ];
    let mut fails = Vec::new();
    for f in filters {
        for splitter in ["x/.*\\n/", "x/.*\\n?/"] {
            let fused = format!("{splitter} {f} p");
            let unfused = format!("{{ {splitter} {f} p }}");
            for inp in inputs {
                let a = stdout_of(&fused, inp);
                let b = stdout_of(&unfused, inp);
                if a != b {
                    fails.push(format!(
                        "filter {f:?} split {splitter:?} input {}:\n  fused:   {}\n  unfused: {}",
                        bytes(inp),
                        bytes(&a),
                        bytes(&b)
                    ));
                }
            }
        }
    }
    assert!(
        fails.is_empty(),
        "{} fused/un-fused mismatches (optimizer changed behaviour):\n\n{}",
        fails.len(),
        fails.join("\n\n")
    );
}

#[test]
fn every_case_has_a_unique_id() {
    let mut ids: Vec<&str> = CASES.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(
        n,
        ids.len(),
        "duplicate case id in grep/ripgrep parity corpus"
    );
}
