# Implementation status

This page is the short inventory. Syntax and examples live in
[the command language](language.md); compatibility details live in
[compatibility notes](compatibility.md).

## Working and covered by tests

The editor currently implements:

- `x` extraction, `y`/`z` splitting, `g` keep, and `v` reject selectors;
- `p` output and `=` byte-coordinate output;
- `s` substitution with global, case-insensitive, dot-all, and numeric
  occurrence flags;
- numbered, named, sed-style, and complete-match replacement references;
- `c` change, groups, pattern binders, `m` map, `t` conditional block, `N` pair
  join, and `r` reduction;
- bracket-tree expansion and collapse with `+` and `-`;
- bounded Levenshtein selection with `~k/literal/`;
- static lexicon expansion with `:sem:/concept/`;
- the awk action documented in [awk.md](awk.md);
- mapped file input, buffered/spilled standard input, sequential multi-file
  processing, and atomic in-place replacement;
- kqueue watch mode on macOS;
- AArch64 NEON literal scanning and a portable fallback.

The regular suite includes unit tests, CLI integration tests, 251 compatibility
cases, grep/ripgrep parity data, and 59 documentation commands.

## Deliberate boundaries

`se` has its own command language. It does not dispatch on `argv[0]` or accept
grep/sed/awk option sets.

The regular-expression backend does not support look-around or pattern
backreferences. Replacement references work because they are expanded after a
match.

The awk action leaves out regex operators, substitutions, `getline`, user
functions, redirection, several special variables, regex-valued `FS`, and some
GNU formatting behavior. Use full awk when a job needs those semantics.

Concept matching is a small static lexicon, not a model or embedding index.
Tree navigation counts brackets; it does not parse strings, comments, JSON, or
a programming language.

Map execution is sequential. This keeps printed output in input order and
avoids shared-output coordination inside nested blocks.

## Platform support

Normal search, selection, and rewriting compile on Linux and macOS. The
AArch64 build uses NEON; other architectures use the fallback scanner.

`-w` is macOS-only. Linux builds return a clear error for that option until an
inotify backend exists.

macOS file mappings receive `F_NOCACHE` and `madvise` hints. Other systems use a
normal read-only mapping.

## Known correctness gaps

The compatibility ledger currently has 21 known mismatches:

- grep output gains a newline when a selected final record lacked one;
- GNU grep ERE backreferences are rejected;
- GNU sed replacement case-conversion escapes are absent;
- chained substitutions do not see prior out-of-band edits;
- the `$$` replacement spelling follows ripgrep rather than sed;
- awk differs in `FNR`, multi-character `FS`, scalar provenance, a Unicode
  string-literal case, dynamic printf width, numeric formatting, and missing
  language constructs.

Each item has an executable case and exact current/desired output in
[compatibility notes](compatibility.md).

## Performance limits

Mapped input and view selection avoid copying source text, but not every
pipeline is allocation-free. Global edits store one mutation per match,
capture replacements own rendered bytes, awk owns state, and reduce owns its
result.

The fused literal line filter avoids constructing a view for every line.
General regex line filters still use the ordinary view pipeline. See
[performance notes](performance.md) for measured numbers and caveats.

## Work before 1.0

The public release still needs decisions that should not be hidden in code:

- add native Linux watch support or keep `-w` explicitly platform-limited;
- decide whether grep's unterminated-line behavior belongs in `p` or a distinct
  record-print action;
- decide whether chained mutations should materialize intermediate views;
- publish signed/tagged binaries for the supported platform matrix;
- keep the compatibility ledger and manual synchronized with syntax changes.

The crates.io package is named `se-rs`; it installs a binary named `se`.
Publication to crates.io, GitHub Releases, and `lolt3under/homebrew-tap` is
performed by the guarded release workflow documented in [releasing.md](releasing.md).

None of these blocks source releases from GitHub. They do block calling the CLI
or language stable.
