# The se command language

## Name

`se` is a structural editor. A program transforms a stream of byte-range views
over one input. A command may select new views, reject views, print information,
or record replacements.

## Synopsis

```text
se [OPTIONS] PROGRAM [FILE...]
```

Commands are separated by whitespace or semicolons. Whitespace inside a
delimited pattern or an awk block belongs to that construct.

```text
x/[0-9]+/ p
x/[0-9]+/;p
```

The spaced form is easier to read. A closing delimiter may be followed
immediately by flags, so `g/error/ip` means a case-insensitive `g` followed by
`p`, but `g/error/i p` leaves no doubt.

## The view stream

The first view is normally the complete input. If the input is 100 bytes long,
that view is `0..100`. `x/re/` replaces it with every non-overlapping match of
`re`. `g/re/` keeps a view if it contains a match. A block runs its own pipeline
for each incoming view and splices the results back in order.

A view is a borrowed range, not a copied string. The editor records mutations
against absolute byte offsets and applies them after the pipeline has finished.
Two practical rules follow:

1. Selectors are silent. Add `p` or `=` when the job is to report selected
   views.
2. Mutations emit the transformed document. Do not add `p` unless a separate
   diagnostic print is really wanted.

Programs beginning with a slash get one extra operation from the driver. `/re/`
is treated as a line predicate over `x/.*\n?/` records, so it is the short form
for grep-like work. No other leading command gets an implicit line split.

## Patterns and delimiters

Commands that take a pattern use the character immediately after the command
letter as the delimiter:

```text
x/pattern/
s/pattern/replacement/
s#path/with/slashes#replacement#
```

The delimiter is not fixed to `/`. Escape a delimiter inside a field with a
backslash. The parser consumes through the next unescaped delimiter.

Regular expressions use the `regex-automata` syntax. Useful parts include:

- literals, `.`, `*`, `+`, `?`, alternation, and groups;
- counted repetition such as `{2}` and `{1,4}`;
- ASCII/POSIX classes such as `[[:digit:]]`;
- Unicode classes, properties, `\w`, and word boundaries;
- named captures `(?P<name>...)` and `(?<name>...)`;
- inline flags accepted by the underlying regex syntax.

`^` and `$` use multiline behavior by default. A dot does not match `\n` unless
the `s` flag or `(?s)` is present. Look-around and regex backreferences are not
supported. Backreferences in a replacement are supported because they refer to
captures from a completed match.

Plain case-sensitive literals skip the general regex engine and use the local
SIMD/SWAR scanner. This is an implementation choice, not different syntax.

## Flags

Flags follow the closing delimiter.

| Flag | Commands | Meaning |
|---|---|---|
| `i` or `I` | pattern commands, `s` | Unicode-aware case-insensitive matching |
| `s` | pattern commands, `s` | dot matches newline |
| `g` | `s` | replace every eligible match in each view |
| `m` or `M` | accepted | multiline anchors are already enabled |
| positive integer | `s` | start with that occurrence in each view |

`s/foo/bar/2` replaces only the second match. `s/foo/bar/g2` starts at the
second match and then replaces the rest. An occurrence of zero is an error.

## Selectors

### Extract: `x/re/`

Replace every incoming view with its non-overlapping matches. Matches stay in
input order.

Input:

```text
request=13AF09C2 status=ok trace=not-a-token
```

<!-- tested: language.extract -->
```console
$ se 'x/[A-F0-9]{8}/ p' input.txt
13AF09C2
```

`x` is the structural equivalent of ripgrep's only-matching mode, but it can
feed another selector or mutation instead of printing.

### Split on matches: `y/re/` and `z/re/`

Replace a view with the gaps between matches. `y` and `z` are aliases. Empty
gaps are not emitted.

<!-- tested: language.split -->
```console
$ printf 'alpha,beta,,gamma' | se 'y/,+/ p'
alpha
beta
gamma
```

This is a regex split, not CSV parsing. Quoted delimiters and escaped fields
need a format-aware parser.

### Keep matching views: `g/re/`

Keep each view containing at least one match. `g` does not extract the match.

<!-- tested: language.keep -->
```console
$ printf 'ok\ntimeout after 5s\nretry\n' |
> se 'x/.*\n?/ g/timeout/ p'
timeout after 5s
```

### Reject matching views: `v/re/`

Drop each view containing a match.

<!-- tested: language.reject -->
```console
$ printf '# comment\nport=80\n# disabled\n' |
> se 'x/.*\n?/ v/^#/ p'
port=80
```

## Mutations

### Change a view: `c/text/`

Replace the complete bytes of every incoming view with `text`. The text is a
literal replacement, not a regular expression. It understands `\n`, `\t`,
`\r`, `\0`, `\\`, and an escaped delimiter.

<!-- tested: language.change -->
```console
$ printf 'token="secret"\n' | se 'x/"[^"]*"/ c/"REDACTED"/'
token="REDACTED"
```

Use `c//` to delete selected views.

### Substitute: `s/re/replacement/flags`

Replace the first match in each view unless `g` or a numeric occurrence says
otherwise.

<!-- tested: language.substitute -->
```console
$ printf 'doe, jane; roe, richard\n' |
> se 's/(?P<family>[a-z]+), (?P<given>[a-z]+)/$given $family/g'
jane doe; richard roe
```

Replacement references are:

| Form | Meaning |
|---|---|
| `$0`, `${0}`, `&`, `\0` | complete match |
| `$1` through `$99`, `${12}` | numbered capture |
| `$name`, `${name}` | named capture |
| `\1` through `\9` | sed-style numbered capture |
| `$$` | one literal dollar sign |
| `\&` | one literal ampersand |
| `\n`, `\t`, `\r`, `\\` | control character or backslash |
| `\xNN` | byte written by two hexadecimal digits |

An unknown or optional nonparticipating capture expands to empty bytes. A NUL
replacement is `\x00`; `\0` means the complete match.

Mutations should not overlap. When they do, the stitcher sorts by start offset
and drops a later mutation whose start lies inside bytes already replaced.
Write pipelines so selected edit ranges are disjoint.

## Actions

### Print a view: `p`

`p` writes each view. It adds one newline only when the view lacks one. This
makes match output readable and avoids double spacing for line views that
already include `\n`.

Printing is a side effect. The view continues to the next command.

### Print byte coordinates: `=`

Write `start,end,length` for each view. Offsets are zero-based, half-open byte
coordinates into the original input.

<!-- tested: language.offsets -->
```console
$ printf 'one TODO, two TODO\n' | se 'x/TODO/ ='
4,8,4
14,18,4
```

Coordinates count bytes, not Unicode scalar values, grapheme clusters, display
columns, or line numbers.

## Blocks and control

### Group: `{ PROGRAM }`

Run `PROGRAM` on every incoming view. The block's output views replace that
view in the outer stream. A group is how a line-scoped substitution is written.

<!-- tested: language.block -->
```console
$ printf 'ok password=open\nERROR password=hunter2 user=7\n' |
> se 'x/.*\n?/ { g/error/i s/password=[^ ]+/password=REDACTED/g }'
ok password=open
ERROR password=REDACTED user=7
```

The first line never reaches `s` because `g/error/i` rejects it. The second line
is rewritten. Unchanged bytes are copied from the original during stitching.

### Pattern binder: `/re/ { PROGRAM }`

Run the block on matching views and pass nonmatching views through unchanged.
Without a block, the action defaults to `p`. At the beginning of the whole
program, the driver first splits input into lines.

The pass-through rule matters for mutations: an address-like program can edit
matching lines without deleting the others. It also means several binders may
print the same line more than once if it matches several predicates.

### Map: `m/re/ { PROGRAM }`

Extract every match and run the block on each match in order. It is close to
`x/re/ { PROGRAM }`.

<!-- tested: language.map -->
```console
$ printf 'Ada met Grace.\n' | se 'm/[A-Z][a-z]+/ { p }'
Ada
Grace
```

Map execution is sequential. Output order is deterministic.

### Test a substitution: `t { PROGRAM }`

Run the block if a prior substitution made a change. The test consumes the
flag, following sed's `t` idea without labels or jumps.

<!-- tested: language.branch -->
```console
$ printf 'error severity=high\n' |
> se 's/error/warning/ t { s/severity=high/severity=review/ }'
warning severity=review
```

If the first substitution does not match, the block is skipped and its input
view passes through.

## Stream operators

### Join adjacent pairs: `N`

Merge view one with view two, view three with view four, and so on. An odd final
view passes through unchanged. The merged range includes bytes between the two
views.

<!-- tested: language.next -->
```console
$ printf '1,2,3,4' | se 'x/[0-9]/ N p'
1,2
3,4
```

`N` is not sed's "append the next input line" command. It operates on adjacent
views already present in the structural stream.

### Reduce: `r/separator/`

Join all current views into one value separated by literal `separator`. Follow
it with `p` to emit the result.

<!-- tested: language.reduce -->
```console
$ printf 'red green blue\n' | se 'x/[a-z]+/ r/, / p'
red, green, blue
```

The reduced value is output data, not an edit against source offsets. Large
reductions allocate a buffer proportional to the result.

## Structural tree navigation

`+` widens each view to the smallest bracketed range enclosing it. `-` replaces
a view with the contents inside its first balanced bracket pair. The scanner
recognizes `()`, `[]`, and `{}`.

<!-- tested: language.tree -->
```console
$ printf 'server { retry { timeout = 5 } mode = "safe" }\n' |
> se 'x/timeout/ + p'
{ timeout = 5 }
```

This is delimiter navigation, not syntax parsing. Brackets inside strings or
comments still count. Mismatched bracket kinds share one nesting depth, so use
these operators on formats where balanced punctuation is meaningful.

A program beginning with `-` must follow `--`:

```sh
se -- '- p' input.txt
```

## Fuzzy selection

`~k/literal/` keeps views containing a substring within Levenshtein distance
`k` of the literal. Insertions, deletions, and substitutions cost one.
Transposition is two edits.

<!-- tested: language.fuzzy -->
```console
$ printf 'receive\nreceve\nreceiver\nsend\n' |
> se 'x/.*\n?/ ~1/receive/ p'
receive
receve
receiver
```

The match is byte-based. A distance greater than or equal to the pattern length
matches every view because the empty substring is close enough.

## Concept selection

`:sem:/concept/` expands a known concept through a small built-in,
case-insensitive lexicon, then keeps matching views.

<!-- tested: language.semantic -->
```console
$ printf 'request succeeded\npanic in worker\ncache failure\n' |
> se 'x/.*\n?/ :sem:/error/ p'
panic in worker
cache failure
```

This is not embedding search. The lexicon is a static table in
`src/engine/semantic.rs`. An unknown concept falls back to a
case-insensitive substring match of itself. Review the table before using this
selector for classification or policy.

## Awk action

`@{ AWK_PROGRAM }` runs the bundled awk language with each incoming view as one
record. The interpreter supplies fields, variables, arrays, arithmetic, control
flow, printing, and common string/math functions.

```text
x/.*\n?/ @{ sum += $2 END { print sum } }
```

The braces inside the awk source are balanced by the outer parser. Braces in
double-quoted awk strings and awk comments do not affect that balance. See the
[awk action guide](awk.md) for the full language and its deliberate omissions.

## Line records and final newlines

Use `x/.*\n?/` for ordinary Unix line records. The optional newline keeps the
last record when a file does not end with `\n`. Use `x/.*\n/` only when the
final unterminated bytes should be ignored.

The newline remains part of a matched line view. Anchors still behave as line
anchors because multiline mode is enabled. `p` sees the existing newline and
does not add another.

For CRLF input, the line view ends in `\r\n`. Patterns that mean "the whole
content of a line" commonly need `\r?` before `$`. The cookbook has a tested
example.

## Errors

The parser rejects an unterminated delimiter, unknown command, unknown named
modifier, missing block, invalid regex, fuzzy selector without a distance, and
substitution occurrence zero. Runtime awk faults also make the process fail.

No-match selectors are not errors. They produce an empty view stream and a zero
process status.
