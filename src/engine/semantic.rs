//! Static term expansion for the `:sem:/concept/` selector.
//!
//! A known concept expands to one row in [`LEXICON`]. The terms are escaped,
//! joined into a case-insensitive alternation, and matched by
//! [`StructuralRegex`]. An unknown concept matches itself as a
//! case-insensitive substring.
//!
//! This module does not perform embedding or model-based search. The table is
//! intentionally small enough to inspect in source.

use crate::engine::{Flags, StructuralRegex};
use anyhow::Result;

/// Built-in concept-to-terms table. The first entry in each row is its key.
/// Matching is case-insensitive and unanchored.
const LEXICON: &[&[&str]] = &[
    &[
        "error",
        "err",
        "fail",
        "failure",
        "failed",
        "exception",
        "fatal",
        "panic",
        "crash",
        "abort",
        "fault",
        "bug",
    ],
    &["warning", "warn", "caution", "deprecated", "alert"],
    &[
        "success",
        "succeeded",
        "succeed",
        "ok",
        "okay",
        "passed",
        "pass",
        "done",
        "complete",
        "completed",
    ],
    &[
        "time",
        "date",
        "datetime",
        "timestamp",
        "clock",
        "hour",
        "minute",
        "second",
        "day",
        "month",
        "year",
    ],
    &[
        "money", "price", "cost", "dollar", "payment", "pay", "invoice", "salary", "fee", "budget",
        "currency", "amount",
    ],
    &[
        "person", "user", "username", "customer", "employee", "author", "member", "account",
        "owner",
    ],
    &[
        "location",
        "place",
        "city",
        "country",
        "address",
        "region",
        "geo",
        "coordinate",
        "coordinates",
        "latitude",
        "longitude",
    ],
    &[
        "network",
        "connection",
        "socket",
        "tcp",
        "udp",
        "ip",
        "host",
        "hostname",
        "port",
        "dns",
        "http",
        "https",
    ],
    &[
        "delete", "remove", "removed", "drop", "dropped", "erase", "destroy", "purge", "discard",
    ],
    &[
        "create", "created", "add", "added", "insert", "inserted", "new", "make", "build",
        "generate",
    ],
    &[
        "speed",
        "fast",
        "quick",
        "rapid",
        "performance",
        "latency",
        "throughput",
        "slow",
    ],
    &[
        "security",
        "auth",
        "authentication",
        "authorization",
        "password",
        "credential",
        "token",
        "encrypt",
        "decrypt",
        "vulnerability",
    ],
];

/// A compiled semantic matcher: the concept's expansion, wrapped as a normal
/// case-insensitive structural regex.
pub struct SemanticMatcher {
    re: StructuralRegex,
}

impl SemanticMatcher {
    pub fn new(concept: &str) -> Result<Self> {
        let terms = expand(concept);
        // Escape each term and join into an alternation. Case-insensitivity is
        // handled by the flag rather than inline so the meta engine can lower it.
        let alternation = terms
            .iter()
            .map(|t| regex_escape(t))
            .collect::<Vec<_>>()
            .join("|");
        let re = StructuralRegex::compile_with(
            &alternation,
            Flags {
                case_insensitive: true,
                ..Flags::default()
            },
        )?;
        Ok(Self { re })
    }

    /// True if `text` mentions any expansion of the concept.
    #[inline]
    pub fn is_match(&self, text: &[u8]) -> bool {
        self.re.find_iter(text).next().is_some()
    }
}

/// Expand `concept` to its related terms, or `[concept]` if it isn't a known
/// concept (so an arbitrary word still matches itself).
fn expand(concept: &str) -> Vec<String> {
    let lc = concept.to_ascii_lowercase();
    for row in LEXICON {
        if row[0] == lc {
            return row.iter().map(|s| s.to_string()).collect();
        }
    }
    vec![concept.to_string()]
}

/// Escape regex metacharacters so a lexicon/fallback term is matched literally.
fn regex_escape(s: &str) -> String {
    const META: &[char] = &[
        '.', '*', '+', '?', '^', '$', '(', ')', '[', ']', '{', '}', '|', '\\', '/',
    ];
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if META.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::SemanticMatcher;

    fn m(concept: &str, text: &str) -> bool {
        SemanticMatcher::new(concept)
            .unwrap()
            .is_match(text.as_bytes())
    }

    #[test]
    fn concept_expands_to_related_terms() {
        assert!(m("error", "the process did panic"));
        assert!(m("error", "connection failure detected"));
        assert!(m("error", "ERROR: boom")); // case-insensitive
        assert!(!m("error", "everything nominal"));
    }

    #[test]
    fn substring_catches_inflections() {
        assert!(m("error", "several errors occurred"));
        assert!(m("delete", "deleted 4 rows"));
    }

    #[test]
    fn unknown_concept_falls_back_to_self() {
        assert!(m("kumquat", "I ate a kumquat"));
        assert!(!m("kumquat", "I ate a banana"));
    }
}
