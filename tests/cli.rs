//! End-to-end tests driving the compiled `se` binary over stdin / files.

use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};

/// Run `se ARGS` feeding `stdin`, returning `(stdout, stderr, exit_code)`.
fn run(args: &[&str], stdin: &str) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_se"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn se");
    if let Err(error) = child.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        assert_eq!(
            error.kind(),
            ErrorKind::BrokenPipe,
            "failed to write test input"
        );
    }
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn stdout(program: &str, input: &str) -> String {
    let (o, _e, code) = run(&[program], input);
    assert_eq!(code, 0, "expected success for `{program}`");
    o
}

#[test]
fn version_uses_the_command_name() {
    let (out, err, code) = run(&["--version"], "");
    assert_eq!(code, 0);
    assert_eq!(out, concat!("se ", env!("CARGO_PKG_VERSION"), "\n"));
    assert!(err.is_empty());
}

// ---------------------------------------------------------------- selectors

#[test]
fn extract_and_print() {
    assert_eq!(
        stdout("x/error/ p", "a error b\nc error d\n"),
        "error\nerror\n"
    );
}

#[test]
fn grep_emulation_is_single_spaced() {
    // x/.*\n/ extracts whole lines (with their newline); p must not double it.
    assert_eq!(
        stdout("x/.*\\n/ g/error/ p", "alpha\nbeta error\ngamma\n"),
        "beta error\n"
    );
}

#[test]
fn yank_splits_between_matches() {
    assert_eq!(stdout("y/,/ p", "a,b,c\n"), "a\nb\nc\n");
}

#[test]
fn z_is_split_like_y() {
    assert_eq!(stdout("z/,/ p", "a,b,c\n"), "a\nb\nc\n");
}

#[test]
fn global_filter() {
    assert_eq!(
        stdout("x/.*\\n/ g/keep/ p", "drop\nkeep me\ndrop\n"),
        "keep me\n"
    );
}

#[test]
fn reject_filter() {
    assert_eq!(stdout("x/.*\\n/ v/drop/ p", "drop\nkeep\n"), "keep\n");
}

// ------------------------------------------------- fused line filter (grep)
// `x/.*\n/ g/lit/` is rewritten to a single NEON search-then-extend pass. These
// pin the behaviour to be byte-identical to the unfused pipeline / grep.

#[test]
fn fused_grep_multiple_matches_per_line_emitted_once() {
    assert_eq!(
        stdout("x/.*\\n/ g/ERROR/ p", "a\nx ERROR ERROR y\nb\n"),
        "x ERROR ERROR y\n"
    );
}

#[test]
fn fused_grep_match_at_start_and_blank_lines() {
    assert_eq!(
        stdout("x/.*\\n/ g/keep/ p", "keep one\n\n\nkeep two\n"),
        "keep one\nkeep two\n"
    );
}

#[test]
fn fused_grep_drops_newlineless_tail_under_required_newline() {
    // `.*\n` requires a terminating newline, so an unterminated final line is
    // not a "line" and a match there is dropped, matching the unfused pipeline.
    assert_eq!(stdout("x/.*\\n/ g/x/ p", "ax\nbx"), "ax\n");
}

#[test]
fn fused_grep_keeps_newlineless_tail_under_optional_newline() {
    assert_eq!(stdout("x/.*\\n?/ g/x/ p", "ax\nbx"), "ax\nbx\n");
}

#[test]
fn fused_grep_invert_keeps_nonmatching_lines() {
    assert_eq!(
        stdout("x/.*\\n/ v/ERROR/ p", "alpha\nbeta ERROR\ngamma\n"),
        "alpha\ngamma\n"
    );
}

#[test]
fn fused_bare_awk_form_prints_matching_lines() {
    // `/lit/` and `/lit/ { p }` desugar to the implicit line split + awk print,
    // which fuses to the same fast line filter.
    assert_eq!(stdout("/error/", "a\nb error\nc\n"), "b error\n");
    assert_eq!(stdout("/error/ { p }", "a\nb error\nc\n"), "b error\n");
}

#[test]
fn awk_multi_binder_passthrough_is_not_broken_by_fusion() {
    // Two binders: the first must NOT be fused to a selector, because it passes
    // every line through to the second. Each line prints once per matching binder.
    assert_eq!(
        stdout("/a/ { p } /b/ { p }", "a\nb\nab\n"),
        "a\nb\nab\nab\n"
    );
}

// ----------------------------------------------- s/// capture-group backrefs

#[test]
fn substitute_numbered_groups() {
    assert_eq!(
        stdout("s/(\\w+) (\\w+)/$2 $1/", "hello world"),
        "world hello"
    );
}

#[test]
fn substitute_sed_style_backrefs() {
    assert_eq!(
        stdout("s/(\\w+) (\\w+)/\\2 \\1/", "hello world"),
        "world hello"
    );
}

#[test]
fn substitute_named_group() {
    assert_eq!(
        stdout("s/year=(?P<y>[0-9]+)/[${y}]/", "year=2026"),
        "[2026]"
    );
    // Bare `$name` form too.
    assert_eq!(stdout("s/year=(?P<y>[0-9]+)/$y!/", "year=2026"), "2026!");
}

#[test]
fn substitute_whole_match_dollar_zero() {
    assert_eq!(stdout("s/hi/<$0>/", "hi there"), "<hi> there");
}

#[test]
fn substitute_literal_double_dollar() {
    assert_eq!(stdout("s/([0-9]+)/$$$1/", "5"), "$5");
}

#[test]
fn substitute_missing_group_renders_empty() {
    assert_eq!(stdout("s/x/[$7]/", "x"), "[]");
}

#[test]
fn substitute_global_with_captures() {
    assert_eq!(stdout("s/(\\w)([0-9])/$2$1/g", "a1 b2"), "1a 2b");
}

// -------------------------------------------------------------------- flags

#[test]
fn substitute_global() {
    assert_eq!(stdout("s/foo/bar/g", "foo foo foo\n"), "bar bar bar\n");
}

#[test]
fn substitute_first_only() {
    assert_eq!(stdout("s/foo/bar/", "foo foo\n"), "bar foo\n");
}

#[test]
fn substitute_case_insensitive() {
    assert_eq!(stdout("s/foo/x/gi", "FOO Foo foo\n"), "x x x\n");
}

#[test]
fn filter_case_insensitive() {
    assert_eq!(stdout("x/.*\\n/ g/foo/i p", "BAR\nFoo\n"), "Foo\n");
}

#[test]
fn replacement_escapes() {
    // \n in the replacement becomes a real newline.
    assert_eq!(stdout("s/,/\\n/g", "a,b,c\n"), "a\nb\nc\n");
}

// ------------------------------------------------------------------ actions

#[test]
fn change_inside_block() {
    assert_eq!(
        stdout("x/\"[^\"]*\"/ c/\"***\"/", "key: \"secret\"\n"),
        "key: \"***\"\n"
    );
}

#[test]
fn equals_prints_offsets_and_length() {
    assert_eq!(stdout("x/cd/ =", "abcdef\n"), "2,4,2\n");
}

// --------------------------------------------------------------- operators

#[test]
fn next_joins_adjacent_pairs() {
    // Digits at 0,2,4,6 join into spans 0..3 and 4..7.
    assert_eq!(stdout("x/[0-9]/ N =", "1\n2\n3\n4\n"), "0,3,3\n4,7,3\n");
}

#[test]
fn reduce_folds_with_separator() {
    // Reduce is a value-producing fold: emit it with `p`.
    assert_eq!(stdout("x/[a-z]/ r/, / p", "a\nb\nc\n"), "a, b, c\n");
}

#[test]
fn awk_binder_runs_action_on_match() {
    assert_eq!(stdout("/error/ { p }", "x\nerror y\nz\n"), "error y\n");
}

#[test]
fn map_runs_block_per_match_in_order() {
    // Regression: a buffered print inside m// must not deadlock, and the map
    // runs sequentially so output order is deterministic.
    assert_eq!(stdout("m/[a-c]/ { p }", "aXbXc\n"), "a\nb\nc\n");
}

#[test]
fn nested_block() {
    // Extract quoted strings, then within each redact the inner word.
    assert_eq!(
        stdout("x/\".*\"/ { s/secret/REDACTED/ }", "a = \"secret\"\n"),
        "a = \"REDACTED\"\n"
    );
}

// ----------------------------------------------------------------- edge cases

#[test]
fn empty_input_is_a_noop() {
    let (o, _e, code) = run(&["x/foo/ p"], "");
    assert_eq!(code, 0);
    assert_eq!(o, "");
}

#[test]
fn empty_file_is_a_noop() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let (o, e, code) = run(&["/anything/", path], "");
    assert_eq!(code, 0, "stderr: {e}");
    assert_eq!(o, "");
}

#[test]
fn multiple_files_are_processed_in_argument_order() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.log");
    let second = dir.path().join("second.log");
    std::fs::write(&first, "needle one\nskip\n").unwrap();
    std::fs::write(&second, "skip\nneedle two\n").unwrap();

    let (o, e, code) = run(
        &[
            "/needle/",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {e}");
    assert_eq!(o, "needle one\nneedle two\n");
}

#[cfg(unix)]
#[test]
fn file_name_containing_a_newline_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("line\nbreak.log");
    std::fs::write(&path, "needle\n").unwrap();

    let (o, e, code) = run(&["/needle/", path.to_str().unwrap()], "");
    assert_eq!(code, 0, "stderr: {e}");
    assert_eq!(o, "needle\n");
}

#[test]
fn missing_input_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.log");

    let (o, e, code) = run(&["/needle/", path.to_str().unwrap()], "");
    assert_ne!(code, 0);
    assert_eq!(o, "");
    assert!(e.contains("failed to open"), "stderr: {e}");
}

#[test]
fn no_match_substitute_passes_through_unchanged() {
    // A substitute is a stream filter: with no match it emits the input verbatim.
    assert_eq!(stdout("s/zzz/q/g", "hello world\n"), "hello world\n");
}

#[test]
fn invalid_regex_is_an_error() {
    let (_o, _e, code) = run(&["x/[/ p"], "abc\n");
    assert_ne!(code, 0);
}

#[test]
fn unbalanced_brace_is_an_error() {
    let (_o, _e, code) = run(&["x/a/ p }"], "abc\n");
    assert_ne!(code, 0);
}

#[test]
fn unknown_command_is_an_error() {
    let (_o, _e, code) = run(&["Q/foo/"], "abc\n");
    assert_ne!(code, 0);
}

// ------------------------------------------------------------- in-place edit

#[test]
fn in_place_edit_rewrites_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("se_ip_{}.txt", std::process::id()));
    std::fs::write(&path, "foo bar foo\n").unwrap();

    let (_o, _e, code) = run(&["-i", "s/foo/QUX/g", path.to_str().unwrap()], "");
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "QUX bar QUX\n");

    std::fs::remove_file(&path).ok();
}

#[cfg(unix)]
#[test]
fn in_place_edit_preserves_file_mode() {
    use std::os::unix::fs::PermissionsExt;

    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(f, "foo").unwrap();
    std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o640)).unwrap();
    let path = f.path().to_str().unwrap();

    let (_o, e, code) = run(&["-i", "s/foo/bar/g", path], "");
    assert_eq!(code, 0, "stderr: {e}");
    let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}

#[test]
fn in_place_edit_with_backup() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("se_ipb_{}.txt", std::process::id()));
    let backup = dir.join(format!("se_ipb_{}.txt.bak", std::process::id()));
    std::fs::write(&path, "hello world\n").unwrap();

    let (_o, _e, code) = run(&["-i=.bak", "s/world/EARTH/", path.to_str().unwrap()], "");
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello EARTH\n");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "hello world\n");

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&backup).ok();
}

// ------------------------------------------------- structural tree navigation

#[test]
fn tree_expand_selects_enclosing_block() {
    assert_eq!(stdout("x/b/ + p", "x = {a, b, c}\n"), "{a, b, c}\n");
}

#[test]
fn tree_collapse_descends_into_first_bracket() {
    assert_eq!(stdout("x/.*\\n/ - p", "outer(inner)\n"), "inner\n");
}

#[test]
fn tree_expand_then_collapse_roundtrip() {
    // `b` -> enclosing `{a, b}` -> back into it -> `a, b`.
    assert_eq!(stdout("x/b/ + - p", "f({a, b}, c)\n"), "a, b\n");
}

#[test]
fn tree_expand_at_top_level_is_passthrough() {
    assert_eq!(stdout("x/word/ + p", "no brackets word here\n"), "word\n");
}

// ------------------------------------------------------------ fuzzy matching

#[test]
fn fuzzy_keeps_near_matches() {
    assert_eq!(
        stdout("x/.*\\n/ ~1/error/ p", "no\nerrror\nerror\nxyz\n"),
        "errror\nerror\n"
    );
}

#[test]
fn fuzzy_distance_two() {
    // "sittin" (inside "sitting") is within edit distance 2 of "kitten".
    assert_eq!(
        stdout("x/.*\\n/ ~2/kitten/ p", "kitten\nsitting\nbanana\n"),
        "kitten\nsitting\n"
    );
}

// --------------------------------------------------------- semantic matching

#[test]
fn semantic_matches_concept_not_just_word() {
    assert_eq!(
        stdout(
            "x/.*\\n/ :sem:/error/ p",
            "all ok\npanic stations\nclean run\n"
        ),
        "panic stations\n"
    );
}

#[test]
fn semantic_unknown_concept_matches_itself() {
    assert_eq!(
        stdout("x/.*\\n/ :sem:/kumquat/ p", "an apple\na kumquat\n"),
        "a kumquat\n"
    );
}

// ------------------------------------------------------ awk variables / math

#[test]
fn awk_fields_nr_nf() {
    assert_eq!(
        stdout("x/.*\\n/ @{ print NR, NF, $2 }", "a b c\nd e\n"),
        "1 3 b\n2 2 e\n"
    );
}

#[test]
fn awk_sum_with_begin_end() {
    assert_eq!(
        stdout(
            "x/.*\\n/ @{ BEGIN{s=0} s += $1 END { print s } }",
            "10\n20\n12\n"
        ),
        "42\n"
    );
}

#[test]
fn awk_associative_array_and_for_in() {
    assert_eq!(
        stdout(
            "x/.*\\n/ @{ c[$1]++ END { print c[\"a\"], c[\"b\"] } }",
            "a\nb\na\na\n"
        ),
        "3 1\n"
    );
}

#[test]
fn awk_math_builtins_and_printf() {
    assert_eq!(
        stdout("x/.*\\n/ @{ printf \"%.0f\\n\", sqrt($1) }", "144\n9\n"),
        "12\n3\n"
    );
}

#[test]
fn awk_unknown_modifier_is_an_error() {
    let (_o, _e, code) = run(&["x/.*\\n/ :bogus:/x/ p"], "x\n");
    assert_ne!(code, 0);
}

#[test]
fn fuzzy_without_distance_is_an_error() {
    let (_o, _e, code) = run(&["~/x/ p"], "x\n");
    assert_ne!(code, 0);
}
