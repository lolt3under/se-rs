# Getting started

## Requirements

The minimum supported compiler is Rust 1.85, the first stable release with the
2024 edition. The normal editor works on Linux and macOS. Watch mode (`-w`)
requires macOS because its event loop uses kqueue.

Check the toolchain:

```console
$ rustc --version
rustc 1.85.0 (or newer)
```

## Install a release

Cargo installs the package as the `se` command:

```sh
cargo install se-rs
se --install-man
```

Cargo installs executable targets but does not copy ancillary files into a
manual directory. The second command writes the manual bundled in `se` to
`$XDG_DATA_HOME/man/man1/se.1`, or to
`$HOME/.local/share/man/man1/se.1` when `XDG_DATA_HOME` is unset. Pass an
explicit directory with `se --install-man=/usr/local/share/man/man1` if the
system manual tree is preferred and writable.

Homebrew users can install the formula from the project tap:

```sh
brew install lolt3under/tap/se
```

The Homebrew formula installs `se(1)` automatically.

These commands become available when the first tagged release has been
published. Use a source checkout before then.

## Build from a checkout

```sh
git clone https://github.com/lolt3under/se-rs.git
cd se-rs
cargo test --all-targets --all-features
cargo build --release
```

The binary is `target/release/se`. Install it for one user:

```sh
install -d "$HOME/.local/bin"
install -m755 target/release/se "$HOME/.local/bin/se"
```

Make sure `$HOME/.local/bin` is on `PATH`. A system installation usually goes
under `/usr/local/bin`:

```sh
sudo install -m755 target/release/se /usr/local/bin/se
```

Install the manual from the checkout with either of these forms:

```sh
se --install-man
install -Dm644 man/se.1 "$HOME/.local/share/man/man1/se.1"
```

The repository does not force `target-cpu=native`. Distribution binaries should
run across the target architecture, not only on the CPU that compiled them. For
a local benchmark build, set the flag for that command:

```sh
RUSTFLAGS='-C target-cpu=native' cargo build --release
```

## The first program

The synopsis is:

```text
se [OPTIONS] PROGRAM [FILE...]
```

`PROGRAM` is one shell argument containing the structural pipeline. With no
`FILE`, input comes from standard input. With one or more files, `se` processes
them in the order given.

A program beginning with `/` is a line predicate. `se '/error/' system.log`
prints lines containing `error`. Programs beginning with other commands start
with one view containing the entire input. In that form, line work begins by
extracting line records with `x/.*\n?/`.

Read [the command language](language.md) before relying on this distinction. It
is the source of most surprising first attempts.

## Shell quoting

Put the whole program in single quotes. That keeps the shell from expanding
`$1`, `$name`, backslashes, braces, `*`, and `?` before `se` sees them.

Good:

```sh
se 's/([a-z]+), ([a-z]+)/$2 $1/g' names.txt
```

Wrong in most shells:

```sh
se "s/([a-z]+), ([a-z]+)/$2 $1/g" names.txt
```

The double-quoted version lets the shell replace `$1` and `$2` with its own
positional parameters. If a replacement truly needs a single quote, either use
the standard shell `'\''` sequence or place the program in a variable with
careful quoting.

An initial collapse command looks like an option. End option parsing first:

```sh
se -- '- p' brackets.txt
```

## Printing and rewriting are separate

Selectors build a stream of views. They are silent unless followed by `p` or
`=`. For example, `x/[0-9]+/` selects numbers but produces no output;
`x/[0-9]+/ p` prints one number per output line.

Mutations behave differently. `s/foo/bar/g` records replacements and emits the
whole transformed document after the pipeline finishes. `c/text/` does the same
for each selected view. You normally do not append `p` to a mutating program.

This separation prevents duplicate output, but it also means that `-n` has
little work to do. It is accepted for sed compatibility; `se` does not
auto-print in the first place.

## Standard input, files, and several files

These forms are equivalent when `input.txt` contains the same bytes:

```sh
se '/needle/' input.txt
se '/needle/' < input.txt
cat input.txt | se '/needle/'
```

Prefer redirection over `cat` for a single file. A pipeline is useful when an
earlier command produces the input.

Several path arguments are processed sequentially:

```sh
se '/needle/' first.log second.log third.log
```

Unlike grep, `se` does not prefix output with file names. It also does not walk
directories. Let the shell or `find` choose files:

```sh
find logs -type f -name '*.log' -exec se '/panic/' {} +
```

File names containing newlines are safe in the `find -exec ... {} +` form.
Avoid `$(find ...)`, which asks the shell to split path names.

## Safe in-place editing

First run the transformation without `-i` and inspect stdout:

```sh
se 's/old.example/new.example/g' service.conf
```

Then keep a backup for the first real edit:

```sh
se -i=.bak 's/old.example/new.example/g' service.conf
cmp -s service.conf.bak service.conf && echo 'no match; file unchanged'
```

The equals sign in `-i=.bak` is required. `-i .bak` is not accepted because it
would make the next argument ambiguous with `PROGRAM`.

The rewrite uses a sibling temporary file followed by rename. A crash cannot
leave a half-written destination, but atomic rename is not a substitute for a
backup, version control, or a filesystem snapshot. Hard links, extended
attributes, ACLs, and platform-specific metadata need special care; test the
behavior required by your environment before bulk edits.

Never combine an unreviewed program with `-i` over a broad path set. Generate
the candidate file list, inspect it, run without `-i`, and only then edit.

## Exit status and diagnostics

`se` returns zero after successful parsing, input, execution, and output. It
returns nonzero for an invalid regular expression, malformed program, missing
file, disallowed `-i` on standard input, or an awk runtime error.

A successful search with no matching view still returns zero. This differs from
grep, where "no selected lines" is status 1. Do not use `se '/x/' file` directly
as a shell condition when you need grep's three-state status contract.

Diagnostics go to standard error. Selected text and transformed documents go
to standard output.

## Watch mode

On macOS, `-w` waits for kqueue notifications and re-runs the program after a
write, extension, or replacement-by-rename:

```sh
se -w '/error/i' application.log
```

Press Ctrl-C to stop. Linux builds reject `-w` with a diagnostic. Use a file
watcher such as `entr` or a systemd path unit around a normal `se` invocation
until a native Linux backend is implemented.

## Where to go next

- Learn the grammar in [the command language](language.md).
- Copy a familiar job from the [Linux cookbook](cookbook.md).
- Use [the awk action guide](awk.md) for columns and reports.
- Check [compatibility notes](compatibility.md) before assuming a POSIX or GNU
  corner case behaves identically.
