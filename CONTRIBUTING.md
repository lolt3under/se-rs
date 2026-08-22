# Contributing to se

`se` is small enough that a patch should be possible to understand without a
tour of a framework. The hard part is behavioral compatibility: a text editor
can be wrong by one byte and still look right in a terminal. Tests therefore
matter as much as the implementation.

## Before writing code

Search the issue tracker and the compatibility notes in
[`docs/compatibility.md`](docs/compatibility.md). If a change adds syntax or
alters byte-level behavior, open an issue first. A short example with exact
input and output is more useful than a broad feature proposal.

Use Rust 1.85 or newer. The usual development loop is:

```sh
env CARGO_TARGET_DIR=/tmp/se-target cargo test --all-targets --all-features --locked
env CARGO_TARGET_DIR=/tmp/se-target cargo fmt --all -- --check
env CARGO_TARGET_DIR=/tmp/se-target cargo clippy --all-targets --all-features --locked -- -D warnings
```

The temporary target directory is optional. It keeps build output out of the
checkout and makes accidental commits of compiled files impossible.

## Tests

Put integration tests under `tests/`. Small tests for an internal matcher or
parser belong beside that module under `#[cfg(test)]`. Fixtures belong under
`tests/fixtures/`; do not add scratch input files to the repository root.

A bug test should pin all three observable results:

- stdout bytes;
- success or failure status;
- stderr when the diagnostic itself is under test.

Include an empty input or no-match case when it is relevant. For line-oriented
behavior, include a final line without `\n`. For matching behavior, consider
NUL bytes and non-ASCII text. Those cases have caused real differences between
Unix text tools.

Examples added to `README.md` or `docs/` need a `<!-- tested: ID -->` marker and
a matching case in `tests/documentation.rs`. The marker check prevents the
cookbook from drifting away from the binary.

## Compatibility changes

The regular suite must stay green:

```sh
env CARGO_TARGET_DIR=/tmp/se-target cargo test --test compatibility --locked
```

The ignored audit reports behavior that is intentionally not implemented yet:

```sh
env CARGO_TARGET_DIR=/tmp/se-target \
  cargo test --test compatibility full_compatibility_audit -- --ignored --nocapture
```

When fixing an item, change its expected status in the corpus and remove the
corresponding entry from `docs/compatibility.md` in the same patch. Do not make
the expected output less precise just to turn the suite green.

## Documentation style

Write commands a reader can paste. Show the input before the command and the
exact output after it. Say when an example assumes GNU syntax, BSD behavior, or
a macOS-only facility. Avoid speed claims without a reproducible command and a
named test machine.

The command language is terse. Documentation around it should not be. Explain
which views enter a command, which views leave it, and whether it prints or
rewrites bytes.

## Pull requests

Keep a pull request to one behavior change when possible. Describe the failure,
the byte-level rule after the patch, and the tests that cover it. CI runs the
format check, Clippy with warnings denied, the MSRV check, and tests on Linux
and macOS.

By contributing, you agree that your contribution is licensed under the BSD
3-Clause license in this repository.
