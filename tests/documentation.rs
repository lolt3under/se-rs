//! Executable examples used by README.md and the guide under docs/.
//!
//! Every documented command carries a tested marker. The second test in this
//! file checks that the marker set and this table stay identical.

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone, Copy)]
struct Case {
    id: &'static str,
    program: &'static str,
    input: &'static [u8],
    expected: &'static [u8],
}

const CASES: &[Case] = &[
    Case {
        id: "readme.search",
        program: r"/error/i",
        input: b"ready\nERROR disk full\nretry\n",
        expected: b"ERROR disk full\n",
    },
    Case {
        id: "readme.replace",
        program: r"s/colour/color/g",
        input: b"colour=blue; foreground-colour=white\n",
        expected: b"color=blue; foreground-color=white\n",
    },
    Case {
        id: "readme.sum",
        program: r"x/.*\n?/ @{ total += $2 END { print total } }",
        input: b"tea 12\ncoffee 18\nwater 3\n",
        expected: b"33\n",
    },
    Case {
        id: "language.extract",
        program: r"x/[A-F0-9]{8}/ p",
        input: b"request=13AF09C2 status=ok trace=not-a-token\n",
        expected: b"13AF09C2\n",
    },
    Case {
        id: "language.split",
        program: r"y/,+/ p",
        input: b"alpha,beta,,gamma",
        expected: b"alpha\nbeta\ngamma\n",
    },
    Case {
        id: "language.keep",
        program: r"x/.*\n?/ g/timeout/ p",
        input: b"ok\ntimeout after 5s\nretry\n",
        expected: b"timeout after 5s\n",
    },
    Case {
        id: "language.reject",
        program: r"x/.*\n?/ v/^#/ p",
        input: b"# comment\nport=80\n# disabled\n",
        expected: b"port=80\n",
    },
    Case {
        id: "language.change",
        program: r#"x/"[^"]*"/ c/"REDACTED"/"#,
        input: b"token=\"secret\"\n",
        expected: b"token=\"REDACTED\"\n",
    },
    Case {
        id: "language.substitute",
        program: r"s/(?P<family>[a-z]+), (?P<given>[a-z]+)/$given $family/g",
        input: b"doe, jane; roe, richard\n",
        expected: b"jane doe; richard roe\n",
    },
    Case {
        id: "language.offsets",
        program: r"x/TODO/ =",
        input: b"one TODO, two TODO\n",
        expected: b"4,8,4\n14,18,4\n",
    },
    Case {
        id: "language.block",
        program: r"x/.*\n?/ { g/error/i s/password=[^ ]+/password=REDACTED/g }",
        input: b"ok password=open\nERROR password=hunter2 user=7\n",
        expected: b"ok password=open\nERROR password=REDACTED user=7\n",
    },
    Case {
        id: "language.map",
        program: r"m/[A-Z][a-z]+/ { p }",
        input: b"Ada met Grace.\n",
        expected: b"Ada\nGrace\n",
    },
    Case {
        id: "language.next",
        program: r"x/[0-9]/ N p",
        input: b"1,2,3,4",
        expected: b"1,2\n3,4\n",
    },
    Case {
        id: "language.reduce",
        program: r"x/[a-z]+/ r/, / p",
        input: b"red green blue\n",
        expected: b"red, green, blue\n",
    },
    Case {
        id: "language.branch",
        program: r"s/error/warning/ t { s/severity=high/severity=review/ }",
        input: b"error severity=high\n",
        expected: b"warning severity=review\n",
    },
    Case {
        id: "language.tree",
        program: r"x/timeout/ + p",
        input: b"server { retry { timeout = 5 } mode = \"safe\" }\n",
        expected: b"{ timeout = 5 }\n",
    },
    Case {
        id: "language.fuzzy",
        program: r"x/.*\n?/ ~1/receive/ p",
        input: b"receive\nreceve\nreceiver\nsend\n",
        expected: b"receive\nreceve\nreceiver\n",
    },
    Case {
        id: "language.semantic",
        program: r"x/.*\n?/ :sem:/error/ p",
        input: b"request succeeded\npanic in worker\ncache failure\n",
        expected: b"panic in worker\ncache failure\n",
    },
    Case {
        id: "grep.literal",
        program: r"/failed/",
        input: b"started\njob failed\nfinished\n",
        expected: b"job failed\n",
    },
    Case {
        id: "grep.insensitive",
        program: r"/warning/i",
        input: b"WARNING hot\nwarning cold\ninfo\n",
        expected: b"WARNING hot\nwarning cold\n",
    },
    Case {
        id: "grep.invert",
        program: r"x/.*\n?/ v/^#/ p",
        input: b"# heading\nvalue=1\n\nvalue=2\n",
        expected: b"value=1\n\nvalue=2\n",
    },
    Case {
        id: "grep.regex",
        program: r"x/.*\n?/ g/status=(4|5)[0-9]{2}/ p",
        input: b"status=200\nstatus=404\nstatus=503\n",
        expected: b"status=404\nstatus=503\n",
    },
    Case {
        id: "grep.exact",
        program: r"x/.*\n?/ g/^ready$/ p",
        input: b"ready\nnot ready\nready now\n",
        expected: b"ready\n",
    },
    Case {
        id: "grep.word",
        program: r"x/.*\n?/ g/\bcat\b/ p",
        input: b"cat\nconcatenate\na cat!\ncat_2\n",
        expected: b"cat\na cat!\n",
    },
    Case {
        id: "grep.alternatives",
        program: r"x/.*\n?/ g/fatal|panic|segfault/i p",
        input: b"normal\nPANIC: worker\nsegfault at 0x0\n",
        expected: b"PANIC: worker\nsegfault at 0x0\n",
    },
    Case {
        id: "grep.matches",
        program: r"x/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/ p",
        input: b"from a@example.org to bob.smith@example.net\n",
        expected: b"a@example.org\nbob.smith@example.net\n",
    },
    Case {
        id: "grep.count",
        program: r"x/.*\n?/ g/error/i @{ END { print NR } }",
        input: b"ok\nerror one\nERROR two\n",
        expected: b"2\n",
    },
    Case {
        id: "grep.blank",
        program: r"x/.*\n?/ g/^$/ p",
        input: b"one\n\ntwo\n\n",
        expected: b"\n\n",
    },
    Case {
        id: "grep.offset",
        program: r"x/needle/ =",
        input: b"a needle and needle\n",
        expected: b"2,8,6\n13,19,6\n",
    },
    Case {
        id: "grep.crlf",
        program: r"x/.*\n?/ g/^error\r?$/i p",
        input: b"ok\r\nERROR\r\nerror detail\r\n",
        expected: b"ERROR\r\n",
    },
    Case {
        id: "grep.nul",
        program: r"x/\x00/ =",
        input: b"left\0right",
        expected: b"4,5,1\n",
    },
    Case {
        id: "sed.first_each_line",
        program: r"x/.*\n?/ { s/foo/bar/ }",
        input: b"foo foo\nfoo foo\n",
        expected: b"bar foo\nbar foo\n",
    },
    Case {
        id: "sed.global",
        program: r"s/foo/bar/g",
        input: b"foo foo\nfoo\n",
        expected: b"bar bar\nbar\n",
    },
    Case {
        id: "sed.path",
        program: r"s#/usr/local#/opt/se#g",
        input: b"PATH=/usr/local/bin:/usr/local/sbin\n",
        expected: b"PATH=/opt/se/bin:/opt/se/sbin\n",
    },
    Case {
        id: "sed.capture",
        program: r"s/([0-9]{4})-([0-9]{2})-([0-9]{2})/$3\/$2\/$1/g",
        input: b"from 2026-08-22 to 2026-09-01\n",
        expected: b"from 22/08/2026 to 01/09/2026\n",
    },
    Case {
        id: "sed.redact",
        program: r"s/(token|password)=[^ &\n]+/$1=REDACTED/g",
        input: b"token=abc&user=7 password=hunter2\n",
        expected: b"token=REDACTED&user=7 password=REDACTED\n",
    },
    Case {
        id: "sed.delete",
        program: r"x/.*\n?/ { g/^\s*(#|$)/ c// }",
        input: b"# comment\n\nport=8080\n  # note\nhost=localhost\n",
        expected: b"port=8080\nhost=localhost\n",
    },
    Case {
        id: "sed.prefix",
        program: r"x/.*\n?/ { s/^/> / }",
        input: b"alpha\nbeta",
        expected: b"> alpha\n> beta",
    },
    Case {
        id: "sed.trim",
        program: r"x/.*\n?/ { s/^[ \t]+// s/[ \t]+$// }",
        input: b"  alpha  \n\tbeta\t\n",
        expected: b"alpha\nbeta\n",
    },
    Case {
        id: "sed.collapse_blank",
        program: r"s/\n{3,}/\n\n/g",
        input: b"one\n\n\n\n\ntwo\n",
        expected: b"one\n\ntwo\n",
    },
    Case {
        id: "sed.address",
        program: r"/deprecated/ { s/enabled=true/enabled=false/g }",
        input: b"stable enabled=true\ndeprecated enabled=true\n",
        expected: b"stable enabled=true\ndeprecated enabled=false\n",
    },
    Case {
        id: "sed.second",
        program: r"x/.*\n?/ { s/:/-/2 }",
        input: b"a:b:c:d\nx:y\n",
        expected: b"a:b-c:d\nx:y\n",
    },
    Case {
        id: "sed.multiline",
        program: r"s/BEGIN.*END/[section removed]/s",
        input: b"keep\nBEGIN\nsecret\nEND\nkeep\n",
        expected: b"keep\n[section removed]\nkeep\n",
    },
    Case {
        id: "awk.fields",
        program: r"x/.*\n?/ @{ print NR, NF, $1, $NF }",
        input: b"alpha beta gamma\none two\n",
        expected: b"1 3 alpha gamma\n2 2 one two\n",
    },
    Case {
        id: "awk.sum",
        program: r"x/.*\n?/ @{ sum += $2 END { print sum } }",
        input: b"apples 12\npears 8\nplums 5\n",
        expected: b"25\n",
    },
    Case {
        id: "awk.average",
        program: r#"x/.*\n?/ @{ sum += $2 END { printf "%.2f\n", sum/NR } }"#,
        input: b"a 10\nb 20\nc 25\n",
        expected: b"18.33\n",
    },
    Case {
        id: "awk.filter",
        program: r"x/.*\n?/ @{ if ($3 >= 80) print $1, $3 }",
        input: b"ada math 91\nlin math 72\ngrace math 88\n",
        expected: b"ada 91\ngrace 88\n",
    },
    Case {
        id: "awk.separator",
        program: r#"x/.*\n?/ @{ BEGIN { FS=":"; OFS="\t" } print $1, $3 }"#,
        input: b"root:x:0:0\nnobody:x:65534:65534\n",
        expected: b"root\t0\nnobody\t65534\n",
    },
    Case {
        id: "awk.count",
        program: r#"x/.*\n?/ @{ count[$1]++ END { print count["GET"], count["POST"] } }"#,
        input: b"GET /\nPOST /login\nGET /health\nGET /\n",
        expected: b"3 1\n",
    },
    Case {
        id: "awk.unique",
        program: r"x/.*\n?/ @{ seen[$1]=1 END { n=0; for (k in seen) n++; print n } }",
        input: b"red\nblue\nred\ngreen\n",
        expected: b"3\n",
    },
    Case {
        id: "awk.minmax",
        program: r"x/.*\n?/ @{ if (NR==1 || $1<min) min=$1; if (NR==1 || $1>max) max=$1 END { print min, max } }",
        input: b"-4\n12\n3\n-9\n",
        expected: b"-9 12\n",
    },
    Case {
        id: "awk.percent",
        program: r#"x/.*\n?/ @{ total += $2; if ($1=="cache") cache += $2 END { printf "%.1f%%\n", 100*cache/total } }"#,
        input: b"cache 30\ndb 20\ncache 50\n",
        expected: b"80.0%\n",
    },
    Case {
        id: "awk.split",
        program: r#"x/.*\n?/ @{ n=split($2, part, "."); print $1, n, part[n] }"#,
        input: b"archive backup.2026.08.tar\n",
        expected: b"archive 4 tar\n",
    },
    Case {
        id: "awk.strings",
        program: r"x/.*\n?/ @{ print toupper($1), substr($2, 1, 4), length($2) }",
        input: b"alice engineering\n",
        expected: b"ALICE engi 11\n",
    },
    Case {
        id: "awk.format",
        program: r#"x/.*\n?/ @{ printf "%-10s %8.2f\n", $1, $2 }"#,
        input: b"cpu 7.5\nmemory 128\n",
        expected: b"cpu            7.50\nmemory       128.00\n",
    },
    Case {
        id: "awk.control",
        program: r#"x/.*\n?/ @{ if ($2<50) grade="fail"; else if ($2<75) grade="pass"; else grade="distinction"; print $1, grade }"#,
        input: b"ann 42\nbob 68\ncy 91\n",
        expected: b"ann fail\nbob pass\ncy distinction\n",
    },
    Case {
        id: "structural.json",
        program: r"x/timeout/ + p",
        input: br#"{"server":{"timeout":30,"retry":2},"client":{"retry":1}}
"#,
        expected: br#"{"timeout":30,"retry":2}
"#,
    },
    Case {
        id: "structural.fuzzy",
        program: r"x/.*\n?/ ~2/kubernetes/ p",
        input: b"kubernetes\nkubernets\nkuberntes\ndocker\n",
        expected: b"kubernetes\nkubernets\nkuberntes\n",
    },
    Case {
        id: "structural.semantic",
        program: r"x/.*\n?/ :sem:/network/ p",
        input: b"CPU is hot\nDNS lookup failed\nsocket closed\n",
        expected: b"DNS lookup failed\nsocket closed\n",
    },
];

fn run(program: &str, input: &[u8]) -> (Vec<u8>, Vec<u8>, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_se"))
        .arg(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn se");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input)
        .expect("write test input");
    let output = child.wait_with_output().expect("wait for se");
    (
        output.stdout,
        output.stderr,
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn documented_commands_produce_the_shown_output() {
    let mut failures = Vec::new();
    for case in CASES {
        let (stdout, stderr, status) = run(case.program, case.input);
        if status != 0 || stdout != case.expected {
            failures.push(format!(
                "{}\n  program: {:?}\n  status: {}\n  stdout: {:?}\n  expected: {:?}\n  stderr: {}",
                case.id,
                case.program,
                status,
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(case.expected),
                String::from_utf8_lossy(&stderr),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "documentation examples failed:\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn documentation_markers_match_the_executable_cases() {
    let expected: BTreeSet<&str> = CASES.iter().map(|case| case.id).collect();
    assert_eq!(
        expected.len(),
        CASES.len(),
        "duplicate documentation case ID"
    );

    let mut found = BTreeSet::new();
    let mut duplicates = Vec::new();
    let root = env!("CARGO_MANIFEST_DIR");
    for path in [
        "README.md",
        "docs/getting-started.md",
        "docs/language.md",
        "docs/cookbook.md",
        "docs/awk.md",
    ] {
        let text = std::fs::read_to_string(format!("{root}/{path}"))
            .unwrap_or_else(|error| panic!("read {path}: {error}"));
        for line in text.lines() {
            let Some(rest) = line.split("<!-- tested: ").nth(1) else {
                continue;
            };
            let id = rest
                .split(" -->")
                .next()
                .unwrap_or_else(|| panic!("malformed test marker in {path}: {line}"));
            if !found.insert(id.to_owned()) {
                duplicates.push(id.to_owned());
            }
        }
    }

    assert!(
        duplicates.is_empty(),
        "duplicate test markers: {duplicates:?}"
    );
    let expected: BTreeSet<String> = expected.into_iter().map(str::to_owned).collect();
    assert_eq!(found, expected, "documentation marker set is out of sync");
}
