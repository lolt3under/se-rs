# se

`se` is a structural text editor. It searches and rewrites byte ranges called
views instead of forcing every job through a line-oriented pattern space.

[![CI](https://github.com/lolt3under/se-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/lolt3under/se-rs/actions/workflows/ci.yml)
[![License: BSD 3-Clause](https://img.shields.io/badge/license-BSD%203--Clause-blue.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](Cargo.toml)

Developer: [xer0x.in](https://xer0x.in)

The command language comes from Rob Pike's structural regular expressions:
select a shape, narrow or widen it, then print or change it. A line is just one
possible shape. The same pipeline can work on words, quoted strings, balanced
blocks, matches spanning several lines, or an entire mapped file.

This is pre-1.0 software. The core editor and its tests are usable, but the
language may still change. `se` is also not a flag-compatible replacement for
grep, ripgrep, sed, or awk. It covers many of the jobs people give those tools
with a smaller set of composable commands.

## Quick look

Search lines without spelling out a line selector:

<!-- tested: readme.search -->
```console
$ printf 'ready\nERROR disk full\nretry\n' | se '/error/i'
ERROR disk full
```

Rewrite every match. A mutating pipeline writes the transformed document:

<!-- tested: readme.replace -->
```console
$ printf 'colour=blue; foreground-colour=white\n' | se 's/colour/color/g'
color=blue; foreground-color=white
```

Select line records and run an awk action over them:

<!-- tested: readme.sum -->
```console
$ printf 'tea 12\ncoffee 18\nwater 3\n' |
> se 'x/.*\n?/ @{ total += $2 END { print total } }'
33
```

## Install

Rust 1.85 or newer is required.

Published releases can be installed from crates.io:

```sh
cargo install se-rs
se --install-man
```

On systems with Homebrew:

```sh
brew install lolt3under/tap/se
```

Until the first tagged release appears, build from a checkout:

```sh
git clone https://github.com/lolt3under/se-rs.git
cd se-rs
cargo build --release
install -Dm755 target/release/se "$HOME/.local/bin/se"
```

Use `sudo install -Dm755 target/release/se /usr/local/bin/se` for a system-wide
installation. Install the manual from a checkout with
`install -Dm644 man/se.1 "$HOME/.local/share/man/man1/se.1"`. Linux and macOS
support normal editing. The `-w` watch option uses kqueue and currently works
only on macOS.

## Synopsis

```text
se [OPTIONS] PROGRAM [FILE...]
```

With no file, `se` reads standard input. Files are processed in argument order.
A program that starts with `/pattern/` receives one line view at a time.
Otherwise the first view is the complete input.

| Option | Meaning |
|---|---|
| `-i` | Rewrite each file atomically. Standard input is not allowed. |
| `-i=.bak` | Copy each original to `FILE.bak`, then rewrite it atomically. |
| `-n` | Accepted for sed compatibility. `se` never prints implicitly. |
| `-E` | Accepted for compatibility. Extended syntax is already the default. |
| `-w` | Re-run when a file changes. macOS only. |
| `--print-man` | Write the bundled `se(1)` source to standard output. |
| `--install-man[=DIR]` | Install `se(1)` for the current user or under `DIR`. |
| `--` | End option parsing. Required when `PROGRAM` starts with `-`. |

The shortest useful command is `/pattern/`, which prints matching lines. The
full language has selectors (`x`, `y`, `g`, `v`), mutations (`s`, `c`), actions
(`p`, `=`), blocks, reducers, tree navigation, fuzzy matching, concept matching,
and an awk action introduced by `@{ ... }`.

## Why views matter

Given this input:

```text
server {
    retry { timeout = 5 }
}
```

`x/timeout/` selects the seven bytes in `timeout`. Appending `+` widens that view
to `{ timeout = 5 }`. Appending `p` prints the widened view. No parser for the
configuration format is involved; the editor walks balanced delimiters around
the selected bytes.

Selectors do not print by themselves. `p` prints a view, `=` prints its byte
range, and a mutation causes the original input to be stitched together with
the requested replacements. Keeping these operations separate makes a program
such as `x/.../ g/.../ s/.../.../` predictable.

## Documentation

The repository keeps its long-form manual in [`docs/`](docs/README.md). Treat
that directory as the canonical copy if the pages are also published to a
GitHub Wiki.

- [Getting started](docs/getting-started.md) covers installation, shell quoting,
  files, standard input, in-place edits, and exit behavior.
- [Command language](docs/language.md) defines views, delimiters, flags, every
  command, replacement reference, and structural operator.
- [Linux text-processing cookbook](docs/cookbook.md) translates common grep,
  ripgrep, and sed tasks and calls out jobs that `se` does not yet cover.
- [Awk action guide](docs/awk.md) documents records, fields, variables,
  expressions, arrays, control flow, builtins, formatting, and worked reports.
- [Compatibility notes](docs/compatibility.md) record the executable parity
  corpus and every known mismatch.
- [Performance notes](docs/performance.md) give the benchmark rig, commands,
  results, and limitations.
- [Implementation status](docs/status.md) separates finished work from plans.
- [Maintainer release procedure](docs/releasing.md) documents crates.io,
  GitHub Release, and Homebrew publication from `main`.
- [`se(1)`](man/se.1) is the installed command reference. It includes the
  command language and a compact grep, ripgrep, sed, and awk cookbook.

## Tests

```sh
cargo test --all-targets --all-features --locked
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
```

The compatibility corpus contains 251 translated grep, ripgrep, sed, and awk
cases. The documentation suite executes every marked example and checks the
marker list against the table in `tests/documentation.rs`.

Build output belongs in `target/` and is ignored. Test fixtures belong in
`tests/fixtures/`. See [CONTRIBUTING.md](CONTRIBUTING.md) before changing syntax
or compatibility behavior.

## License

`se` is distributed under the [BSD 3-Clause license](LICENSE).
