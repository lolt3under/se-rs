# Security policy

## Supported versions

Until the first stable release, security fixes are made on the default branch.
Once stable release branches exist, this table will name the supported series.

## Reporting a vulnerability

Do not open a public issue for a vulnerability that could expose files, corrupt
data during an in-place edit, or execute unintended input as a program. Use
GitHub's private vulnerability reporting feature in the `se-rs` repository.
Include:

- the operating system and `se --version` output;
- the smallest input and program that reproduce the problem;
- the expected and actual result;
- whether `-i` or `-w` was involved;
- any crash log or sanitizer output you already have.

You should receive an acknowledgement within seven days. A fix, test, and
release timeline will follow once the report is reproduced. Please allow time
for a release before publishing details.

Text supplied to `se` is data, but the `PROGRAM` argument is code in se's own
language. Do not pass an untrusted string as the program. In-place editing also
deserves the same care as `sed -i`: test the transformation without `-i` first,
then use `-i=.bak` when the original matters.
