# Compatibility notes

- Last audited: **2026-08-22**
- Corpus: [`tests/compatibility.rs`](../tests/compatibility.rs)
- Status: **251 static cases; 230 compatible; 21 known failures**

This file records every mismatch found by the static compatibility audit. It is
an inventory, not a claim that `se` should become a drop-in CLI replacement for
all four tools. The corpus translates behavior into `se` programs where the
operation is expressible. It also includes a focused set of important missing
semantics so that omissions remain visible and executable.

## How to run the corpus

The normal compatibility target is green. It checks all compatible cases
against the desired behavior and checks every known failure against `se`'s
currently observed behavior:

~~~sh
cargo test --test compatibility
~~~

The full audit compares all 251 cases with the desired grep/ripgrep/sed/awk
behavior. It is intentionally red until the ledger is empty:

~~~sh
cargo test --test compatibility full_compatibility_audit -- --ignored --nocapture
~~~

When fixing a failure:

1. Change the implementation.
2. Run the full audit and confirm that the named case now passes.
3. Remove its `KnownFailure` entry from `tests/compatibility.rs`.
4. Move or remove its ledger entry here.
5. Run `cargo test --all-targets`.

The known-failure test pins the current incorrect stdout, success/error status,
and a stable stderr fragment. An accidental behavior change therefore cannot
silently turn one known failure into a different failure.

## Upstream source snapshots

The test vectors were translated from or inspired by these exact upstream
snapshots:

| Family | Upstream snapshot | Main material consulted |
|---|---|---|
| GNU grep | [`79da8e07613966b9e53c7ef31b4765d39f98044d`](https://git.savannah.gnu.org/cgit/grep.git/commit/?id=79da8e07613966b9e53c7ef31b4765d39f98044d) | `tests/ere.tests`, empty-line, anchors, backrefs, UTF-8, NUL, long-pattern tests |
| ripgrep | [`3fce3b5bb0236da2df6d99672afb8a719642eca7`](https://github.com/BurntSushi/ripgrep/commit/3fce3b5bb0236da2df6d99672afb8a719642eca7) | `tests/misc.rs`, `multiline.rs`, `binary.rs`, `feature.rs`, `regression.rs` |
| GNU sed | [`1cfb077a6087b5e62efd727678fd6f11d3771c04`](https://git.savannah.gnu.org/cgit/sed.git/commit/?id=1cfb077a6087b5e62efd727678fd6f11d3771c04) | substitution replacement/options, addresses, regex errors, stdin and NUL tests |
| GNU awk | [`edcf238aaec147483d10bf2adfb030b17504a74f`](https://git.savannah.gnu.org/cgit/gawk.git/commit/?id=edcf238aaec147483d10bf2adfb030b17504a74f) | fields, scalar coercion, arrays, formatting, builtins, control flow |

Expected values were also spot-checked against locally installed BSD grep,
ripgrep 15.2.0, BSD sed, and One True Awk where their semantics overlap.

## Results

| Family | Cases | Passing | Failing |
|---|---:|---:|---:|
| grep | 63 | 61 | 2 |
| ripgrep | 28 | 28 | 0 |
| sed | 56 | 49 | 7 |
| awk | 104 | 92 | 12 |
| **Total** | **251** | **230** | **21** |

The four highest-leverage items from the repair order are done: the synthetic
end-of-input record, awk runtime-error propagation, sed `&`/numeric-selector/`I`
syntax, and the `\0`/`$$` dialect decision. The remaining failures are dominated
by sed case-conversion escapes and missing awk language features.

## Shared root-cause map

| Cluster | Affected | Likely implementation area | Suggested first move |
|---|---:|---|---|
| ~~Synthetic EOF record / zero-width progress~~ | 0 (fixed) | `src/engine/regex.rs` | Fixed: iterators drop a zero-width match at `slice.len()` |
| `p` forces newline on unterminated data | 1 | `PrintCommand` | Preserve the selected view's terminator state |
| Unsupported regex backreferences | 1 | `StructuralRegex` backend choice | Add a fallback engine or explicitly retain non-support |
| sed case-conversion escapes (`\U\L\u\l\E`) | 5 | `src/commands/replacement.rs` | Add a case-mode segment to the replacement AST |
| Replacement-dialect boundary (`$$` literal) | 1 (permanent) | `src/commands/replacement.rs` | Decided: `$$`=literal `$` (ripgrep); sed's plain-`$` not adopted |
| Chained out-of-band mutations | 1 | substitution/mutation model | Make later substitutions observe prior replacements |
| awk fields/coercion/formatting | 9 | `src/awk/interp.rs`, `value.rs`, `printf.rs`, lexer | Add special variables and retain scalar provenance |
| Missing awk language constructs | 3 | awk lexer/parser/AST/interpreter | Add regex operators, substitutions, and functions separately |
| ~~Awk runtime errors exit successfully~~ | 0 (fixed) | `AwkRun` in `src/commands/mod.rs` | Fixed: errors recorded on `ExecutionContext` → non-zero exit |
| ~~sed `&` / numeric selector / uppercase `I`~~ | 0 (fixed) | `src/parser/mod.rs`, `src/commands/replacement.rs` | Fixed: `&`/`\0`=whole match, `s///N` selector, `I`/`M` flags |

## grep failures

### `grep.unterminated_matching_line`

- Origin: GNU grep final-line behavior.
- Program: `x/.*\n?/ g/hit/ p`
- Input: `"no\nhit"` with no final newline.
- Desired stdout: `"hit"`, still unterminated.
- Current stdout: `"hit\n"` with exit 0.
- Detail: `PrintCommand` appends a newline whenever a view lacks one. This is
  convenient for tokens but not byte-compatible with grep record output.
- Acceptance: grep-shaped output preserves the final record's terminator state.

### `grep.ere_backreference`

- Origin: GNU grep `backref`.
- Program: `x/.*\n?/ g/^(a)\1$/ p`
- Input: `"aa\nab\n"`
- Desired stdout: `"aa\n"`.
- Current: exit 1, stderr contains `Invalid regex`, no stdout.
- Detail: `regex-automata` does not implement backreferences; GNU grep ERE does.
- Acceptance: the first line matches and the second does not, or the feature is
  explicitly retained as a permanent compatibility boundary.

## ripgrep failures

None. All translated ripgrep cases pass (the two former failures,
`rg.only_matching_empty_suppressed` and `rg.empty_match_at_line`, were fixed by
suppressing the zero-width match at `slice.len()` in `src/engine/regex.rs`).

## sed failures

GNU sed executes substitution once per pattern space. Most translations below
use `x/.*\n?/ { ... }` to express that record cycle.

### `sed.literal_dollar`: permanent dialect boundary

- Origin: GNU sed replacement behavior.
- Program: `x/.*\n?/ { s/x/$$/ }`
- Input: `"x\n"`
- Desired (GNU sed) stdout: `"$$\n"`
- `se` stdout: `"$\n"`
- Decision (2026-08-22): `se` uses the ripgrep replacement dialect, where `$`
  introduces group references and `$$` is the escape for a literal `$`. Adopting
  sed's plain-`$` semantics (bare `$$` → `$$`) is impossible without breaking
  `$1`/`${name}`/`$0` and the passing `rg.replacement_dollar_literal` case
  (`s/x/$$/g` → `$ $`). This case is therefore an accepted, permanent divergence,
  not a bug to fix. (The `\0`/`&` half of the dialect conflict *was* resolved:
  `&`, `\0`, `$0` are the whole match; a NUL byte is written `\x00`.)

### `sed.uppercase_all_replacement`

- Origin: GNU sed `subst-replacement`.
- Program: `x/.*\n?/ { s/(.)(.)(.)/\U\1\2\3/ }`
- Input: `"abCde\n"`
- Desired stdout: `"ABCde\n"`
- Current stdout: `"UabCde\n"`
- Detail: unknown escape `\U` discards the slash and emits literal `U`.
- Acceptance: uppercase replacement content until `\E`.

### `sed.uppercase_next_replacement`

- Origin: GNU sed `subst-replacement`.
- Program: `x/.*\n?/ { s/(.)(.)(.)/\u\1\2\3/ }`
- Input: `"abCde\n"`
- Desired stdout: `"AbCde\n"`
- Current stdout: `"uabCde\n"`
- Acceptance: uppercase only the next replacement character.

### `sed.lowercase_all_replacement`

- Origin: GNU sed `subst-replacement`.
- Program: `x/.*\n?/ { s/(.)(.)(.)/\L\1\2\3/ }`
- Input: `"AbCde\n"`
- Desired stdout: `"abcde\n"`
- Current stdout: `"LAbCde\n"`
- Acceptance: lowercase replacement content until `\E`.

### `sed.lowercase_next_replacement`

- Origin: GNU sed `subst-replacement`.
- Program: `x/.*\n?/ { s/(.)(.)(.)/\l\1\2\3/ }`
- Input: `"AbCde\n"`
- Desired stdout: `"abCde\n"`
- Current stdout: `"lAbCde\n"`
- Acceptance: lowercase only the next replacement character.

### `sed.case_conversion_end`

- Origin: GNU sed `subst-replacement`.
- Program: `x/.*\n?/ { s/(.)(.)(.)/\U\1\E\2\3/ }`
- Input: `"abCde\n"`
- Desired stdout: `"AbCde\n"`
- Current stdout: `"UaEbCde\n"`
- Detail: neither case mode nor `\E` has an AST representation.
- Acceptance: uppercase group 1 and return to neutral before groups 2 and 3.

### `sed.chained_substitution`

- Origin: GNU sed execution semantics.
- Program: `x/.*\n?/ { s/a/b/ s/b/c/ }`
- Input: `"a\n"`
- Desired stdout: `"c\n"`
- Current stdout: `"b\n"`
- Detail: edits are out-of-band while the original view continues downstream.
  The second substitution cannot see the first replacement.
- Acceptance: later commands observe earlier substitutions without losing
  non-overlap and offset correctness.

## awk failures

### `awk.fnr_single_input`

- Origin: awk special variables.
- Program: `x/.*\n/ @{ print FNR }`
- Input: `"a\nb\n"`
- Desired stdout: `"1\n2\n"`
- Current stdout: `"\n\n"`
- Detail: `FNR` is an ordinary uninitialized variable. Even for one input it
  should track `NR`.
- Acceptance: implement `FNR` and later add reset semantics at file boundaries.

### `awk.fs_multichar_regex`

- Origin: GNU awk field tests.
- Program: `x/.*\n/ @{ BEGIN { FS="[,:]+" } print NF, $1, $2, $3 }`
- Input: `"a,,b:c\n"`
- Desired stdout: `"3 a b c\n"`
- Current stdout: `"1 a,,b:c  \n"`
- Detail: multi-character `FS` is a literal separator, but awk specifies it as
  a regular expression.
- Acceptance: compile non-special `FS` values and split on regex matches.

### `awk.numeric_string_comparison`

- Origin: GNU awk scalar coercion behavior.
- Program: `x/.*\n/ @{ print ("02" == 2), ("10" < "2") }`
- Input: one dummy record.
- Desired stdout: `"0 1\n"`
- Current stdout: `"1 0\n"`
- Detail: every fully parseable string is considered numeric. Awk distinguishes
  pure strings from strnum values derived from input/conversion, and comparison
  mode depends on that provenance.
- Suspected area: `src/awk/value.rs`.
- Acceptance: pure string literals compare as strings in these expressions.

### `awk.length_record`

- Origin: awk string-function syntax.
- Program: `x/.*\n/ @{ print length }`
- Input: `"abc de\n"`
- Desired stdout: `"6\n"`
- Current stdout: `"\n"`
- Detail: awk permits bare `length` as `length($0)`. The parser resolves it as
  an uninitialized variable.
- Acceptance: recognize bare `length` in expression position.

### `awk.index_unicode`

- Origin: GNU awk multibyte string tests.
- Program: `x/.*\n/ @{ print index($1,"é") }`
- Input: `"aλéc\n"`
- Desired stdout: `"3\n"`
- Current stdout: `"0\n"`
- Detail: `index()` counts character positions, but the awk string lexer pushes
  UTF-8 bytes individually as chars and corrupts the non-ASCII needle.
- Suspected area: `lex_string` in `src/awk/lexer.rs`.
- Acceptance: preserve UTF-8 literals and return character index 3.

### `awk.printf_dynamic_width`

- Origin: GNU awk `printf`.
- Program: `x/.*\n/ @{ printf "%*s\n", 4, "x" }`
- Input: one dummy record.
- Desired stdout: `"   x\n"`
- Current: non-zero exit, empty stdout; stderr contains
  `unsupported printf conversion '%*'`.
- Detail: the runtime error now propagates to a non-zero status (fixed), but
  dynamic width is still unimplemented.
- Acceptance: support `*` width so the program succeeds with the desired stdout.

### `awk.convfmt`

- Origin: GNU awk `CONVFMT`.
- Program: `x/.*\n/ @{ BEGIN { CONVFMT="%.2f"; x=1/3; print x "" } }`
- Desired stdout: `"0.33\n"`
- Current stdout: `"0.333333\n"`
- Detail: `convfmt_raw()` is hard-coded to `%.6g`.
- Acceptance: string conversion of numbers uses the current `CONVFMT`.

### `awk.ofmt`

- Origin: GNU awk `OFMT`.
- Program: `x/.*\n/ @{ BEGIN { OFMT="%.2f"; print 1/3 } }`
- Desired stdout: `"0.33\n"`
- Current stdout: `"0.333333\n"`
- Detail: `print` ignores `OFMT`.
- Acceptance: numeric print formatting uses the current `OFMT` where required.

### `awk.large_number_default_format`

- Origin: GNU awk numeric formatting.
- Program: `x/.*\n/ @{ print 1e20 }`
- Desired stdout: `"1e+20\n"`
- Current stdout: `"100000000000000000000\n"`
- Detail: `fmt_num` uses fixed formatting for large integral values instead of
  `%g`-style exponent selection.
- Acceptance: match GNU awk's default significant-digit/exponent behavior.

### `awk.regex_match_operator`

- Origin: core awk regex expressions.
- Program: `x/.*\n/ @{ if ($0 ~ "foo") print $0 }`
- Input: `"foo\nbar\n"`
- Desired stdout: `"foo\n"`
- Current: exit 1; lexer reports unexpected `~`.
- Detail: this is intentionally omitted today, but it is a major compatibility
  boundary and now has an executable test.
- Acceptance: implement `~`/`!~` or retain this as a permanent non-goal.

### `awk.gsub_builtin`

- Origin: GNU awk `gsub`.
- Program: `x/.*\n/ @{ gsub("a","x"); print }`
- Input: `"banana\n"`
- Desired stdout: `"bxnxnx\n"`
- Current: non-zero exit, empty stdout; stderr contains `unknown function 'gsub'`.
- Detail: the missing-function error now propagates to a non-zero status (fixed),
  but `gsub` itself is still absent.
- Acceptance: mutate `$0`, resplit fields, and print the result.

### `awk.user_function`

- Origin: GNU awk function tests.
- Program: `x/.*\n/ @{ function twice(x) { return x*2 } print twice($1) }`
- Input: `"21\n"`
- Desired stdout: `"42\n"`
- Current: non-zero exit, empty stdout; stderr contains `unknown function 'twice'`.
- Detail: the runtime call failure now propagates to a non-zero status (fixed),
  but functions and `return` still lack AST/interpreter support.
- Acceptance: implement function scope/call/return.

## Suggested repair order

1. The shared EOF/zero-width record bug is fixed. The match iterators
   in `src/engine/regex.rs` drop a zero-width match at `slice.len()`, retiring
   14 failures across all four families.
2. Awk runtime errors now propagate. `AwkRun` records the first
   interpreter fault on `ExecutionContext.error`; `drive` turns it into a
   non-zero exit. Division/modulo by zero is now diagnosed. This retired 2
   failures and made 3 missing-feature cases return a failure status.
3. Sed `&`, numeric occurrence selectors, and uppercase `I` are implemented.
   `&`/`\0` compile to the whole match (`\&` literal), `s///N` selects the Nth
   occurrence, and `I`/`M` are accepted uppercase flag aliases. Retired 4.
4. The `\0` and `$$` replacement-dialect conflicts have an explicit decision.
   `\0`/`&`/`$0` = whole match, NUL = `\x00` (retired
   `sed.whole_match_backslash_zero`); `$$` stays the ripgrep literal-`$` escape,
   so `sed.literal_dollar` is an accepted permanent boundary.
5. Make chained substitutions observe prior edits.
6. Fix awk scalar provenance, `FNR`, bare `length`, UTF-8 string literals, and
   formatting variables.
7. Treat larger regex-engine and awk-language additions as separate projects.
