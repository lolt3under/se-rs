# Linux text-processing cookbook

This page starts with the job, not the language grammar. Each tested transcript
uses exact input bytes from `tests/documentation.rs`. Read
[the command language](language.md) when a selector or block is unfamiliar.

The closest rough translations are:

| Familiar command | se shape |
|---|---|
| `grep PATTERN` | `se '/PATTERN/'` |
| `grep -i PATTERN` | `se '/PATTERN/i'` |
| `grep -v PATTERN` | `se 'x/.*\n?/ v/PATTERN/ p'` |
| `grep -o PATTERN` | `se 'x/PATTERN/ p'` |
| `sed 's/A/B/g'` | `se 'x/.*\n?/ { s/A/B/g }'` for sed's per-line behavior |
| `rg --multiline 'A.*B'` | `se 'x/A.*B/s p'` |
| `awk '{...}'` | `se 'x/.*\n?/ @{ ... }'` |

These are translations, not CLI aliases. `se` does not interpret grep, ripgrep,
sed, or awk options.

## Finding lines

### How do I print lines containing a literal?

Start the program with a slash. That form is line-oriented and prints matching
lines.

<!-- tested: grep.literal -->
```console
$ printf 'started\njob failed\nfinished\n' | se '/failed/'
job failed
```

The line includes its input newline. An unterminated matching final line is
still printed with a newline because `p` normalizes match output.

### How do I ignore case?

Append `i` to the closing delimiter.

<!-- tested: grep.insensitive -->
```console
$ printf 'WARNING hot\nwarning cold\ninfo\n' | se '/warning/i'
WARNING hot
warning cold
```

Case-insensitive regex matching uses Unicode rules. A plain case-sensitive ASCII
literal takes the faster literal scanner.

### How do I exclude matching lines?

Make the line records explicit and use `v`.

<!-- tested: grep.invert -->
```console
$ printf '# heading\nvalue=1\n\nvalue=2\n' |
> se 'x/.*\n?/ v/^#/ p'
value=1

value=2
```

This removes comment lines but keeps blank lines. To remove both, use
`v/^\s*(#|$)/` or the deletion recipe later on this page.

### How do I match one of several patterns?

Use regular-expression alternation.

<!-- tested: grep.alternatives -->
```console
$ printf 'normal\nPANIC: worker\nsegfault at 0x0\n' |
> se 'x/.*\n?/ g/fatal|panic|segfault/i p'
PANIC: worker
segfault at 0x0
```

Unlike several `grep -e` arguments, the alternatives share one set of flags.
Group them when an anchor or repetition should apply to the whole choice.

### How do I find HTTP error status lines?

Keep line views containing a 400 or 500 series status:

<!-- tested: grep.regex -->
```console
$ printf 'status=200\nstatus=404\nstatus=503\n' |
> se 'x/.*\n?/ g/status=(4|5)[0-9]{2}/ p'
status=404
status=503
```

This checks only three digits after `status=`. Tighten the right boundary if the
input can contain a value such as `status=5031`.

### How do I match a complete line?

Anchor both ends.

<!-- tested: grep.exact -->
```console
$ printf 'ready\nnot ready\nready now\n' |
> se 'x/.*\n?/ g/^ready$/ p'
ready
```

Anchors are multiline by default. For CRLF, put `\r?` before `$` as shown
below.

### How do I match a whole word?

Use `\b` around the word.

<!-- tested: grep.word -->
```console
$ printf 'cat\nconcatenate\na cat!\ncat_2\n' |
> se 'x/.*\n?/ g/\bcat\b/ p'
cat
a cat!
```

`\b` follows the regex engine's Unicode word definition. Underscore is a word
character, which is why `cat_2` does not match.

### How do I print only the matching parts?

Extract matches with `x` and print them.

<!-- tested: grep.matches -->
```console
$ printf 'from a@example.org to bob.smith@example.net\n' |
> se 'x/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/ p'
a@example.org
bob.smith@example.net
```

This expression is useful for logs and ad hoc text. It is not a complete parser
for every address allowed by the mail RFCs.

### How do I count matching lines?

Filter first, then let the awk action print its record count at `END`.

<!-- tested: grep.count -->
```console
$ printf 'ok\nerror one\nERROR two\n' |
> se 'x/.*\n?/ g/error/i @{ END { print NR } }'
2
```

`NR` counts views that reach the awk action. Here those views are matching
lines, so it is the selected-line count.

### How do I find blank lines?

An empty anchored pattern matches an empty line view.

<!-- tested: grep.blank -->
```console
$ printf 'one\n\ntwo\n\n' | se 'x/.*\n?/ g/^$/ p'


```

The output contains two newline bytes. Blank output is hard to inspect by eye;
pipe to `wc -l` or `od -An -tx1` when verifying it in a script.

### How do I get byte offsets?

Extract the match and use `=`.

<!-- tested: grep.offset -->
```console
$ printf 'a needle and needle\n' | se 'x/needle/ ='
2,8,6
13,19,6
```

Offsets are bytes into the whole input. They are not the `file:line:column`
coordinates printed by grep or ripgrep.

### How do I handle CRLF files?

Line views retain `\r\n`. Permit the carriage return when anchoring the end.

<!-- tested: grep.crlf -->
```console
$ printf 'ok\r\nERROR\r\nerror detail\r\n' |
> se 'x/.*\n?/ g/^error\r?$/i p'
ERROR
```

The displayed line is still CRLF. Many terminals render it like an LF line.
Use `od` when the distinction matters.

### Can I search for a NUL byte?

Yes. `\x00` in the regex is a NUL byte escape. This example reports its
coordinate instead of sending the byte back to a terminal.

<!-- tested: grep.nul -->
```console
$ printf 'left\0right' | se 'x/\x00/ ='
4,5,1
```

`se` treats input as bytes. A literal pattern can also find text on either side
of invalid UTF-8. Unicode regex constructs still require the regex engine's
Unicode semantics.

## Choosing files

### How do I search several named files?

Pass them after the program:

```sh
se '/timeout/' api.log worker.log scheduler.log
```

Output is concatenated in argument order. `se` does not add file-name headers,
so use a shell loop if provenance must be visible:

```sh
for file in api.log worker.log scheduler.log; do
    printf '==> %s <==\n' "$file"
    se '/timeout/' "$file"
done
```

### How do I search recursively?

`se` deliberately leaves traversal to a path enumerator.

```sh
find . -type f -name '*.rs' -exec se '/unsafe/' {} +
```

To honor ignore files and hidden-file rules, ask ripgrep only for paths:

```sh
rg --files -0 -g '*.rs' |
while IFS= read -r -d '' file; do
    se '/unsafe/' "$file"
done
```

The NUL separator preserves spaces, tabs, quotes, and newlines in file names.
Recursive output still lacks path labels unless the loop prints them.

### How do I search compressed logs?

Decompress to standard input:

```sh
gzip -cd application.log.gz | se '/panic|fatal/i'
xz -cd archive.log.xz | se '/panic|fatal/i'
```

In-place editing is impossible on a decompression stream. Write a new file,
inspect it, then replace the archive separately.

## Rewriting text

### How do I replace every occurrence in the input?

At top level, one view is the whole input. The `g` flag replaces every match in
that view.

<!-- tested: sed.global -->
```console
$ printf 'foo foo\nfoo\n' | se 's/foo/bar/g'
bar bar
bar
```

Without `g` this particular program replaces only the first `foo` in the
complete input, not the first on each line.

### How do I replace the first occurrence on every line?

Select line records and apply the substitution inside a block.

<!-- tested: sed.first_each_line -->
```console
$ printf 'foo foo\nfoo foo\n' |
> se 'x/.*\n?/ { s/foo/bar/ }'
bar foo
bar foo
```

This line block is the usual bridge from sed's pattern-space model to `se`.

### How do I avoid escaping slash-heavy paths?

Pick another delimiter.

<!-- tested: sed.path -->
```console
$ printf 'PATH=/usr/local/bin:/usr/local/sbin\n' |
> se 's#/usr/local#/opt/se#g'
PATH=/opt/se/bin:/opt/se/sbin
```

Any character can be the delimiter. Choose one absent from both fields, or
escape it.

### How do I reorder capture groups?

Capture the fields and reference them in the replacement.

<!-- tested: sed.capture -->
```console
$ printf 'from 2026-08-22 to 2026-09-01\n' |
> se 's/([0-9]{4})-([0-9]{2})-([0-9]{2})/$3\/$2\/$1/g'
from 22/08/2026 to 01/09/2026
```

The slashes in the replacement are escaped because this example keeps `/` as
the delimiter. `s#...#$3/$2/$1#g` is easier to read in a real script.

### How do I redact secrets in key-value text?

Keep the key in group 1 and replace the value.

<!-- tested: sed.redact -->
```console
$ printf 'token=abc&user=7 password=hunter2\n' |
> se 's/(token|password)=[^ &\n]+/$1=REDACTED/g'
token=REDACTED&user=7 password=REDACTED
```

This is appropriate for simple log fields. It does not understand shell
quoting, JSON escapes, URL percent encoding, or multiline values. Use a parser
for structured secrets.

### How do I delete comments and blank lines?

Select each line, keep comment or blank lines inside the block, then change
those selected views to empty bytes.

<!-- tested: sed.delete -->
```console
$ printf '# comment\n\nport=8080\n  # note\nhost=localhost\n' |
> se 'x/.*\n?/ { g/^\s*(#|$)/ c// }'
port=8080
host=localhost
```

Because mutations are stitched into the original, lines rejected by `g` stay
unchanged. This is different from a reporting pipeline where rejected views
simply disappear from printed output.

### How do I prefix every line?

Use a line block and substitute at the start anchor.

<!-- tested: sed.prefix -->
```console
$ printf 'alpha\nbeta' | se 'x/.*\n?/ { s/^/> / }'
> alpha
> beta
```

The final input line has no newline and remains unterminated in the transformed
document. The display above ends at `beta`.

### How do I trim spaces and tabs around each line?

Use horizontal characters, not `[[:space:]]`. The latter includes the newline
that belongs to a line view.

<!-- tested: sed.trim -->
```console
$ printf '  alpha  \n\tbeta\t\n' |
> se 'x/.*\n?/ { s/^[ \t]+// s/[ \t]+$// }'
alpha
beta
```

This removes ASCII space and tab. Unicode whitespace needs an explicit Unicode
class and careful treatment of the trailing line ending.

### How do I collapse runs of blank lines?

Operate on the whole input and replace three or more newline bytes with two.

<!-- tested: sed.collapse_blank -->
```console
$ printf 'one\n\n\n\n\ntwo\n' | se 's/\n{3,}/\n\n/g'
one

two
```

This normalizes LF files. It does not normalize CRLF first; use a separate,
reviewed conversion when line-ending preservation matters.

### How do I change only lines matching an address?

A pattern binder passes all lines through, but runs the block only on matches.

<!-- tested: sed.address -->
```console
$ printf 'stable enabled=true\ndeprecated enabled=true\n' |
> se '/deprecated/ { s/enabled=true/enabled=false/g }'
stable enabled=true
deprecated enabled=false
```

This syntax is close to a sed regex address. Numeric addresses and address
ranges are not implemented.

### How do I replace only the second match on each line?

Append a positive occurrence number.

<!-- tested: sed.second -->
```console
$ printf 'a:b:c:d\nx:y\n' |
> se 'x/.*\n?/ { s/:/-/2 }'
a:b-c:d
x:y
```

A view with fewer than two matches is unchanged. Add `g` before or after the
number to replace the second and every later match.

### How do I remove a section spanning several lines?

Use dot-all mode on the complete-input view.

<!-- tested: sed.multiline -->
```console
$ printf 'keep\nBEGIN\nsecret\nEND\nkeep\n' |
> se 's/BEGIN.*END/[section removed]/s'
keep
[section removed]
keep
```

`.*` is greedy. If several sections can appear, a lazy expression such as
`.*?` or a more specific delimiter pattern may be required. Test malformed
input with a missing `END` before using `-i`.

## Structural and approximate work

### How do I select the object containing a key?

Extract the key and widen once to its enclosing bracket pair.

<!-- tested: structural.json -->
```console
$ printf '{"server":{"timeout":30,"retry":2},"client":{"retry":1}}\n' |
> se 'x/timeout/ + p'
{"timeout":30,"retry":2}
```

This happens to work on the compact JSON shown. `+` counts brackets inside JSON
strings too, so it is not a JSON parser and can choose the wrong range when a
string value contains `{` or `}`.

### How do I find likely misspellings?

Use a bounded Levenshtein selector on line views.

<!-- tested: structural.fuzzy -->
```console
$ printf 'kubernetes\nkubernets\nkuberntes\ndocker\n' |
> se 'x/.*\n?/ ~2/kubernetes/ p'
kubernetes
kubernets
kuberntes
```

Start with a small distance. Work rises with pattern length and the number of
candidate views that survive the literal prefilter.

### How do I search a small related-term concept?

Use the built-in lexicon selector.

<!-- tested: structural.semantic -->
```console
$ printf 'CPU is hot\nDNS lookup failed\nsocket closed\n' |
> se 'x/.*\n?/ :sem:/network/ p'
DNS lookup failed
socket closed
```

The `network` row includes terms such as `connection`, `socket`, `tcp`, `host`,
`port`, `dns`, and `http`. It is deterministic and inspectable, but it does not
understand context. A sentence saying "no network problem" still matches.

## Jobs that need another tool or a shell wrapper

`se` currently has no native equivalent for these common switches:

- recursive path traversal and ignore-file handling;
- file-name prefixes or "files with matches" output;
- grep's status 1 for no match;
- source line numbers;
- before/after context lines;
- sorted output or stable ordering of associative-array keys;
- sed numeric addresses, ranges, hold space, labels, and general branching;
- full awk regex operators, `getline`, user functions, or output redirection;
- PCRE look-around and regex backreferences;
- format-aware JSON, CSV, XML, or programming-language parsing.

Composition is preferable to pretending those semantics exist. Use `find` or
`rg --files` for paths, `sort` for ordering, `jq` for JSON, a CSV parser for
quoted CSV, and grep when its exit-status contract is the point of the command.
