# Public release checklist

This checklist is for the first import into `lolt3under/se-rs` and for later
tagged releases. Check the command output; do not treat the list as ceremony.

## Before the clean import

- [ ] Confirm that BSD 3-Clause remains the intended license and that
  `lolt3under` remains the correct copyright holder in `LICENSE`.
- [ ] Confirm the final GitHub owner and update `repository` in `Cargo.toml`,
  badge URLs, issue-template links, and clone commands if it is not
  `lolt3under/se-rs`.
- [x] Publish the crate as `se-rs` while retaining `se` as the binary name.
  `publish = ["crates-io"]` prevents publication to an unintended registry.
- [ ] Review `git diff --check` and every deletion. In particular, the old
  checkout tracked compiled files under `target/`.
- [ ] Search the working tree for credentials, private host names, personal
  paths, editor state, build output, and scratch input.

The old Git object database contains the files and messages from its history.
Do not copy `.git` into the new repository. A clean import also keeps the
compiled objects from inflating the new clone.

## Create se-rs from a committed tree

Commit the reviewed cleanup in the old checkout first. Then export exactly that
commit into an empty directory:

```sh
old_repo=/absolute/path/to/se
new_repo=/absolute/path/to/se-rs

test ! -e "$new_repo"
mkdir -p "$new_repo"
git -C "$old_repo" archive --format=tar HEAD | tar -xf - -C "$new_repo"
git -C "$new_repo" init -b main
git -C "$new_repo" add .
git -C "$new_repo" commit -S -m 'Initial public release'
git -C "$new_repo" remote add origin https://github.com/lolt3under/se-rs.git
git -C "$new_repo" push -u origin main
```

The `test ! -e` guard refuses to write into an existing path. Keep it. If the
new GitHub repository was created with an initial README or license, clone it
and reconcile that commit explicitly instead of overwriting it.

After the import:

```sh
test ! -e "$new_repo/.git/objects/info/alternates"
test ! -e "$new_repo/target"
git -C "$new_repo" status --short
git -C "$new_repo" ls-files | sort
```

`git status --short` should print nothing.

## Repository settings

- [ ] Set the description to a factual one-line summary from `Cargo.toml`.
- [ ] Add topics such as `rust`, `text-processing`, `regex`, `grep`, `sed`, and
  `awk`.
- [ ] Enable Issues, Discussions only if someone will moderate them, and private
  vulnerability reporting.
- [ ] Protect `main`. Require the format/lint, Linux test,
  macOS test, Rust 1.85, package, and RustSec checks. Also require resolved
  review conversations and linear history. A mandatory approval is not useful
  while the project has one maintainer, because it would make a maintainer's
  own pull request unmergeable.
- [ ] Prevent branch deletion and force pushes on `main`.
- [ ] Enable dependency graph, Dependabot alerts, and secret scanning where the
  repository plan provides them.
- [ ] Confirm the scheduled RustSec dependency audit has completed successfully.
- [ ] Check that the issue forms, pull-request template, security policy,
  contributing guide, code of conduct, and license appear in GitHub's community
  profile.
- [ ] Decide whether `docs/` is the canonical wiki or whether it will be synced
  to GitHub Wiki. Do not maintain two divergent copies by hand.

## Clean build and test

Run the checks from a fresh clone, with no pre-existing `target/`:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features --document-private-items --locked
cargo +1.85.0 check --all-targets --all-features --locked
cargo audit --deny warnings
cargo package --locked
```

The audit command requires `cargo-audit`; CI installs the version pinned in
`.github/workflows/security.yml`.

The package command must verify the unpacked archive, not merely create a
`.crate` file. Inspect its contents:

```sh
cargo package --locked --list
```

There should be no `target/` objects, local settings, repository administration
files, or scratch data in the crate archive.

Run a Linux build and a macOS build. CI covers both after the repository exists,
but the first release should not be the first time an artifact is executed.
On macOS, also exercise `-w` against a temporary file and stop it with Ctrl-C.

## Version and release notes

- [ ] Move useful entries from "Unreleased" in `CHANGELOG.md` into a version
  heading with the release date.
- [ ] Choose a SemVer version. Pre-1.0 minor changes may still break syntax, but
  the changelog must say so.
- [ ] Update `Cargo.toml` and regenerate `Cargo.lock` with the same version.
- [ ] Run the complete compatibility audit and attach the known-failure count to
  the release notes.
- [ ] Re-run benchmarks only if the release makes a performance claim. Record
  all environment details from `docs/performance.md`.
- [ ] Commit the release change and run the `Publish release` workflow from
  `main`. See [the maintainer release procedure](releasing.md).

## Binary artifacts

At minimum, name every archive with version, operating system, architecture, and
libc where it matters. Do not label an artifact simply "linux" if it requires a
particular dynamic libc.

For each archive:

- [ ] build from the signed tag in a clean environment;
- [ ] include `LICENSE`, `README.md`, and the `se` binary;
- [ ] run `se --version` from the unpacked archive;
- [ ] run a search, substitution, awk action, empty file, and unterminated final
  line case against that binary;
- [ ] check the linked libraries with `otool -L` on macOS or `ldd` on Linux;
- [ ] generate SHA-256 checksums;
- [ ] sign the checksum file or use a release-signing system with documented
  verification.

```sh
sha256sum se-v0.1.0-*.tar.gz > SHA256SUMS
sha256sum -c SHA256SUMS
```

macOS uses `shasum -a 256` if GNU `sha256sum` is not installed.

## After publishing

- [ ] Install one release artifact on a clean account and follow the README,
  not a maintainer shortcut.
- [ ] Verify the source link, license, checksums, and documentation links from
  the public release page.
- [ ] Confirm CI runs for pull requests from forks with read-only permissions.
- [ ] Open one test issue and pull request to confirm templates and required
  checks, then close them.
- [ ] Keep "Unreleased" at the top of `CHANGELOG.md` for the next change.

If any artifact differs from the signed tag, remove or replace the release
before announcing it. A checksum only proves which wrong file someone received;
it does not make the file correct.
