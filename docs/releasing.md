# Maintainer release procedure

Releases start from the current `main` branch head. The `Publish release`
workflow runs the regular checks, packages the crate, publishes crates.io,
creates the matching GitHub Release, and updates the Homebrew formula. It is a
manual workflow because crates.io versions cannot be replaced or deleted.

The Cargo package is named `se-rs`. Its installed command remains `se`.

## One-time repository setup

Create a crates.io API token authorized to publish `se-rs`. Store it in the
`release` environment as `CARGO_REGISTRY_TOKEN`. Do not put the token in a
shell profile, workflow file, issue, or command history.

The Homebrew tap uses a dedicated Ed25519 deploy key. Its public half is the
write-enabled `se-rs release workflow` deploy key on
`lolt3under/homebrew-tap`; its private half is stored as
`HOMEBREW_TAP_SSH_KEY` in the `release` environment. It has no access to other
repositories or account settings. To rotate it, add and verify the replacement
key before deleting the old one.

The workflow checks both secrets and verifies write access to the tap before
it runs `cargo publish`. A missing or under-scoped credential therefore stops
the job before crates.io is changed.

## Prepare a version

Work on a branch and open a pull request. Make the release commit contain all
of the following:

1. Set the intended SemVer version in `Cargo.toml`.
2. Regenerate `Cargo.lock` with `cargo check --locked` or, when the version has
   changed, `cargo check` followed by a review of the lockfile diff.
3. Move the relevant `CHANGELOG.md` entries from `Unreleased` to a heading for
   the version and release date.
4. Run the test, lint, audit, and package commands in
   [CONTRIBUTING.md](../CONTRIBUTING.md).
5. Merge only after every required check succeeds on the current pull-request
   head.

Do not reuse a version. crates.io publication is permanent, even if its GitHub
Release or Homebrew formula is later removed.

## Run the workflow

Open **Actions**, choose **Publish release**, select **Run workflow**, leave the
branch set to `main`, and enter the version without a `v` prefix. For version
0.2.0, enter `0.2.0`, not `v0.2.0`.

The workflow refuses to continue unless:

- the selected commit is the current `main` head;
- the supplied version exactly matches `Cargo.toml`;
- both publishing secrets are present;
- the tap credential can authenticate a push;
- formatting, Clippy, and the full test suite pass; and
- `cargo package --locked` can build and verify the release archive.

The crate archive's SHA-256 digest is used in both `SHA256SUMS` and the Homebrew
formula. The GitHub Release tag is `vVERSION`; the crates.io and Cargo version
is `VERSION`.

## Verify the result

Replace `0.1.0` below with the released version:

```sh
cargo install se-rs --version 0.1.0 --locked
se --version
se --install-man
printf 'skip\nmatch\n' | se '/match/'

brew update
brew install lolt3under/tap/se
brew test lolt3under/tap/se
brew audit --strict lolt3under/tap/se
man se
```

Check that the output of `se --version` is `se 0.1.0` and that the search prints
`match`. Then verify that the GitHub Release contains the `.crate` archive and
`SHA256SUMS`, and that `Formula/se.rb` in the tap contains the same version and
digest. The Homebrew installation must also contain `share/man/man1/se.1`.

## Re-running a failed release

A run that fails before `cargo publish` can be corrected and restarted. If the
crate was already published, do not change the source or checksum. Re-run the
workflow on the same `main` commit with the same version.

The workflow queries crates.io first. When it finds the version, it compares
the registry checksum with the locally built archive. A match permits the run
to repair a missing GitHub Release asset or Homebrew formula. A mismatch stops
the run because two different archives must never claim the same version.

If the source must change after publication, prepare a new patch version.
