## Problem

Describe the incorrect behavior or missing capability. Include exact input and
output for text-processing changes.

## Change

Explain the byte-level rule after this patch and any compatibility tradeoff.

## Checks

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --all-targets --all-features --locked`
- [ ] New or changed documentation examples have cases in `tests/documentation.rs`
