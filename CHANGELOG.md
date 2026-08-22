# Changelog

This file records user-visible changes. The project follows Semantic Versioning
once a stable release exists.

## Unreleased

### Added

- Structural selectors, substitutions, blocks, tree navigation, fuzzy matching,
  concept matching, and the awk action language.
- Memory-mapped file input, buffered stdin, atomic in-place writes, and macOS
  kqueue watch mode.
- Compatibility suites derived from common grep, ripgrep, sed, and awk behavior.

### Changed

- Prepared the source tree, package metadata, tests, and documentation for the
  first public release.
- Updated `anyhow` and `memmap2` to releases that address their current RustSec
  unsoundness advisories.

### Known limitations

- Watch mode requires macOS.
- `se` is not a command-line-compatible replacement for grep, ripgrep, sed, or
  awk. It covers their common text-processing jobs with its own language.
- The package is not yet published to crates.io.
