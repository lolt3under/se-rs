# se wiki

This directory is the maintained manual for `se`. It is written as repository
documentation so changes can be reviewed beside the code and tested in the same
CI run. If these pages are mirrored to GitHub Wiki, keep this directory as the
canonical copy and adjust repository-relative links during publication.

## Read in this order

1. [Getting started](getting-started.md) explains how programs receive input,
   when output appears, how shell quoting works, and how to make safe file
   edits.
2. [The command language](language.md) is the reference. It defines the view
   stream and every token the parser accepts.
3. [The Linux text-processing cookbook](cookbook.md) starts with the question
   people usually have ("How do I find these lines?", "How do I remove this
   field?") and gives a tested `se` program.
4. [The awk action guide](awk.md) covers numeric reports, fields, arrays,
   formatting, loops, and the differences from a full POSIX awk.

The remaining pages are for checking claims and maintaining the project:

- [Compatibility notes](compatibility.md) list the size of the parity corpus
  and its known failures.
- [Performance notes](performance.md) include the machine, data set, commands,
  and caveats behind each number.
- [Implementation status](status.md) says what is finished, partial, or absent.
- [Public release checklist](release-checklist.md) covers the clean `se-rs`
  import, repository settings, artifacts, checksums, and tag verification.
- [Maintainer release procedure](releasing.md) covers the guarded crates.io,
  GitHub Release, and Homebrew workflow.
- [Contributing](../CONTRIBUTING.md) gives the required checks and test layout.
- [Security policy](../SECURITY.md) explains private reporting and the risks of
  in-place editing.

## Notation used by this manual

Shell transcripts use `$` for the first prompt and `>` for a continuation
prompt. Do not paste the prompt characters.

```console
$ printf 'one\ntwo\n' | se '/two/'
two
```

Text blocks headed "Input" and "Output" show exact data. A missing final newline
is called out explicitly. Byte offsets are zero-based and half-open:
`start,end,length` means bytes `start..end` and `length = end - start`.

The words must, should, and may have the usual manual-page force:

- must states a requirement of the language or CLI;
- should describes the safe or conventional choice;
- may marks optional behavior.

## Tested examples

Every `se` transcript that states exact output in the README and the four main
guide pages has a marker of the form `<!-- tested: family.name -->`.
`tests/documentation.rs` contains the input bytes and expected output for the
same identifier. CI fails if either side has an identifier the other side
lacks. Installation, traversal, and release commands are checked separately
because their results depend on the local filesystem and installed tools.

That check is intentionally plain. Documentation for a text editor should not
depend on a Markdown parser deciding whether a fence is executable. The marker
says which command is a contract; the Rust test compares bytes.

The examples include empty input, no matches, an unterminated final line, CRLF,
Unicode, and NUL bytes across the wider test suite. See
[Compatibility notes](compatibility.md) for the larger corpus.

## Manual conventions

The command reference follows the shape of a traditional Unix manual:
synopsis, semantics, errors, and examples. The cookbook follows the Arch and
Gentoo wiki habit of showing the concrete job first and placing warnings next
to the command that needs them. Compatibility claims stay on their own page so
an attractive README cannot quietly outrun the implementation.
