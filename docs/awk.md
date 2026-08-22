# The awk action

## Name

`@{ ... }` runs a compact awk-like interpreter over the current `se` view
stream. It is meant for arithmetic, columns, counters, associative arrays, and
formatted reports after structural selection.

It is not a complete POSIX awk. In particular, it has no regex match operators,
`getline`, user-defined functions, or redirection. Use `se` selectors for
matching and the shell for files and pipes.

## Records come from the structural pipeline

Classic awk decides that one input line is one record. `se` decides records
before the awk action. Each incoming view becomes `$0` and increments `NR`.

For ordinary lines:

```text
x/.*\n?/ @{ AWK_PROGRAM }
```

For quoted strings:

```text
x/"[^"]*"/ @{ AWK_PROGRAM }
```

For matched blocks:

```text
x/marker/ + @{ AWK_PROGRAM }
```

This is the main reason the action exists inside `se`. Field processing no
longer decides the structure of the source.

## Program form

The text between `@{` and its matching `}` is parsed as awk source. It may
contain:

- `BEGIN { ... }` blocks, run before the first record;
- ordinary statements, run once per record;
- `END { ... }` blocks, run after the last record.

Braces in a double-quoted string or after an awk `#` comment do not close the
outer action. Quote the complete `se` program with single quotes in the shell.

## Records and fields

`$0` is the current record without its trailing LF or CRLF. `$1` through `$NF`
are one-based fields. `$(expression)` selects a computed field. A missing field
is the uninitialized value, which prints as an empty string and converts to
zero.

<!-- tested: awk.fields -->
```console
$ printf 'alpha beta gamma\none two\n' |
> se 'x/.*\n?/ @{ print NR, NF, $1, $NF }'
1 3 alpha gamma
2 2 one two
```

The predefined read-only values are:

| Variable | Value |
|---|---|
| `NR` | number of records received by this awk action |
| `NF` | number of fields in the current record |

`NF` may be assigned. Shrinking it drops fields; growing it adds empty fields
and rebuilds `$0` with `OFS`. Assigning `$0` resplits the new record. Assigning
`$n` updates that field and rebuilds `$0`.

Unlike a full awk, `FNR`, `FILENAME`, `ARGC`, and `ARGV` are not provided. The
outer CLI owns file iteration.

## Field splitting

`FS` controls how `$0` is split:

| `FS` value | Rule |
|---|---|
| one space, the default | split on runs of Unicode whitespace; trim ends |
| empty string | split into Unicode scalar values |
| one character | split at that character and keep empty interior/end fields |
| several characters | split on that literal string |

A multi-character `FS` is not a regular expression. `FS="[,:]+"` means that
exact five-character separator, not "one or more comma/colon characters."

`OFS` is inserted between arguments to `print` and between fields when `$0` is
rebuilt. It defaults to one space. `ORS` follows every `print` and defaults to
one newline.

<!-- tested: awk.separator -->
```console
$ printf 'root:x:0:0\nnobody:x:65534:65534\n' |
> se 'x/.*\n?/ @{ BEGIN { FS=":"; OFS="\t" } print $1, $3 }'
root	0
nobody	65534
```

That example reads the user name and numeric UID from passwd-shaped text. It
does not consult NSS and should not replace `getent passwd` for account lookup.

`SUBSEP` joins the components of a multi-index array key. Its default is the
ASCII file-separator character.

## Values and conversion

Values are numbers, strings, or uninitialized. Arithmetic converts an operand
to a number. Concatenation converts it to a string. Comparisons are numeric when
both operands carry numeric values; otherwise they compare strings.

Uninitialized values are useful for counters:

```text
count[$1]++
total += $2
```

They are dangerous when initializing a minimum from data that may be positive
or negative. Use `NR == 1` for the first record rather than assuming zero is a
valid starting minimum.

Numeric output uses a fixed `%.6g`-style conversion. `CONVFMT` and `OFMT` are
not implemented as special variables; use `printf` when the format is part of
the contract.

## Operators

From high to low precedence, the useful families are:

- field selection `$expr` and calls `name(args)`;
- exponentiation `^`;
- unary `+`, `-`, `!`, and pre/post `++` or `--`;
- multiplication, division, and remainder: `* / %`;
- addition and subtraction: `+ -`;
- string concatenation by adjacency;
- comparisons `< <= > >= == !=` and membership `key in array`;
- logical `&&` and `||`;
- ternary `condition ? yes : no`;
- assignment `= += -= *= /= %= ^=`.

Logical operators short-circuit. Division or remainder by zero is a runtime
error and makes `se` exit nonzero.

## Printing

`print` with no arguments writes `$0 ORS`. With arguments, it converts each
value, joins them with `OFS`, and writes `ORS`.

`printf` accepts a format followed by values and does not append `ORS`. The
implementation covers the common integer, radix, floating, string, character,
width, precision, sign, zero-fill, and left-alignment forms.

<!-- tested: awk.format -->
```console
$ printf 'cpu 7.5\nmemory 128\n' |
> se 'x/.*\n?/ @{ printf "%-10s %8.2f\n", $1, $2 }'
cpu            7.50
memory       128.00
```

Use `sprintf(format, values...)` to build the same formatted string without
printing it immediately.

## Arithmetic reports

### Sum a column

<!-- tested: awk.sum -->
```console
$ printf 'apples 12\npears 8\nplums 5\n' |
> se 'x/.*\n?/ @{ sum += $2 END { print sum } }'
25
```

### Compute an average

<!-- tested: awk.average -->
```console
$ printf 'a 10\nb 20\nc 25\n' |
> se 'x/.*\n?/ @{ sum += $2 END { printf "%.2f\n", sum/NR } }'
18.33
```

An empty input leaves `NR` at zero, so this exact formula would divide by zero.
For data that may be empty:

```text
END { if (NR) printf "%.2f\n", sum/NR; else print "no records" }
```

### Print rows passing a numeric condition

<!-- tested: awk.filter -->
```console
$ printf 'ada math 91\nlin math 72\ngrace math 88\n' |
> se 'x/.*\n?/ @{ if ($3 >= 80) print $1, $3 }'
ada 91
grace 88
```

For regex filtering, select views before `@{ ... }`. The awk action does not
implement `~` or `!~`.

### Find minimum and maximum

<!-- tested: awk.minmax -->
```console
$ printf '%s\n' -4 12 3 -9 |
> se 'x/.*\n?/ @{ if (NR==1 || $1<min) min=$1; if (NR==1 || $1>max) max=$1 END { print min, max } }'
-9 12
```

The `NR==1` branch handles all-positive and all-negative data correctly.

### Compute a percentage

<!-- tested: awk.percent -->
```console
$ printf 'cache 30\ndb 20\ncache 50\n' |
> se 'x/.*\n?/ @{ total += $2; if ($1=="cache") cache += $2 END { printf "%.1f%%\n", 100*cache/total } }'
80.0%
```

Guard `total` when zero values or empty data are possible.

## Associative arrays

Arrays are created on first assignment and use string keys.

```text
count[$1]++
pair[$1, $2] = $3
```

Use `key in array` to test membership without creating the element. Use
`delete array[key]` for one element and `delete array` for the whole array.

### Count named categories

<!-- tested: awk.count -->
```console
$ printf 'GET /\nPOST /login\nGET /health\nGET /\n' |
> se 'x/.*\n?/ @{ count[$1]++ END { print count["GET"], count["POST"] } }'
3 1
```

### Count distinct values

<!-- tested: awk.unique -->
```console
$ printf 'red\nblue\nred\ngreen\n' |
> se 'x/.*\n?/ @{ seen[$1]=1 END { n=0; for (k in seen) n++; print n } }'
3
```

`for (key in array)` visits every current key, but order comes from a hash map
and is not stable. Pipe printed keys to `sort` when people or scripts need a
defined order.

## Control flow

The interpreter supports:

```text
if (condition) statement
if (condition) statement else statement
while (condition) statement
for (init; condition; post) statement
for (key in array) statement
break
continue
next
```

A statement may be a brace block. `next` stops the current record and proceeds
to the next incoming view. `break` and `continue` outside a loop are errors.

<!-- tested: awk.control -->
```console
$ printf 'ann 42\nbob 68\ncy 91\n' |
> se 'x/.*\n?/ @{ if ($2<50) grade="fail"; else if ($2<75) grade="pass"; else grade="distinction"; print $1, grade }'
ann fail
bob pass
cy distinction
```

## String functions

| Function | Result |
|---|---|
| `length()` or `length(value)` | number of Unicode scalar values |
| `substr(text, start)` | text from one-based character position |
| `substr(text, start, count)` | at most `count` characters |
| `index(text, needle)` | one-based character position, or zero |
| `tolower(text)` | Unicode lowercase conversion |
| `toupper(text)` | Unicode uppercase conversion |
| `split(text, array)` | split with `FS`, replace array, return field count |
| `split(text, array, separator)` | split with the stated literal separator |
| `sprintf(format, values...)` | formatted string |

<!-- tested: awk.strings -->
```console
$ printf 'alice engineering\n' |
> se 'x/.*\n?/ @{ print toupper($1), substr($2, 1, 4), length($2) }'
ALICE engi 11
```

### Split a field again

<!-- tested: awk.split -->
```console
$ printf 'archive backup.2026.08.tar\n' |
> se 'x/.*\n?/ @{ n=split($2, part, "."); print $1, n, part[n] }'
archive 4 tar
```

The separator is literal. A dot here means a dot, not regex "any character."

## Math functions

The numeric builtins are:

| Function | Meaning |
|---|---|
| `sin(x)`, `cos(x)` | radians |
| `atan2(y, x)` | angle in radians |
| `exp(x)`, `log(x)` | exponential and natural logarithm |
| `sqrt(x)` | square root |
| `int(x)` | truncate toward zero |
| `rand()` | pseudo-random number in `[0, 1)` |
| `srand()`, `srand(seed)` | reseed and return the previous seed |

The generator is deterministic until `srand` is called. Do not use it for
passwords, tokens, sampling that must resist manipulation, or cryptography.

## What full awk users will miss

The following syntax is intentionally absent or incomplete:

- regex literals, `~`, `!~`, `match`, `sub`, and `gsub`;
- user-defined `function` and `return`;
- `getline`, pipes, and file redirection inside awk;
- `FNR`, `FILENAME`, `ARGC`, `ARGV`, `RSTART`, and `RLENGTH`;
- special behavior for `CONVFMT` and `OFMT`;
- regex-valued multi-character `FS`;
- locale-specific numeric and collation rules.

These limits are recorded as executable cases in
[compatibility notes](compatibility.md). Use full `awk` or `gawk` when a job
depends on them. The point of `@{ ... }` is to keep common field arithmetic next
to structural selection, not to hide a partial clone behind a familiar name.
