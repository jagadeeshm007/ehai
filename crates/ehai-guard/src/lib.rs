//! Compliance guard (spec §9): find PHI/PII in text ehai is about to write to a
//! tracker, a PRD, a POC fixture, a log or a report, and decide what happens:
//!
//! - `off`: nothing is checked.
//! - `warn`: findings go back to the user, who chooses redact / keep / cancel.
//! - `strict`: high-confidence findings are redacted automatically; low-confidence
//!   ones are surfaced, never silently rewritten.
//!
//! Findings record *what kind* of data was found and *where*, never the value.

use ehai_core::config::{ComplianceMode, CustomPattern};
use regex::{Regex, RegexBuilder};
use serde::Serialize;

pub mod audit;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid compliance pattern `{name}`: {source}")]
    Pattern {
        name: String,
        #[source]
        source: regex::Error,
    },
    #[error("audit log: {0}")]
    Audit(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Structured identifiers (SSN, MRN, member id, DOB, email, phone...).
    High,
    /// Heuristics (a name next to a medical term). Never auto-redacted.
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub kind: String,
    pub start: usize,
    pub end: usize,
    pub confidence: Confidence,
}

/// What to do with a piece of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Clean,
    /// `warn` mode: ask the user. `redacted` is offered as the safe option.
    Ask {
        findings: Vec<Finding>,
        redacted: String,
    },
    /// `strict` mode: use `text`. `remaining` are low-confidence findings to mention.
    Redacted {
        text: String,
        findings: Vec<Finding>,
        remaining: Vec<Finding>,
    },
}

struct Rule {
    kind: String,
    re: Regex,
    confidence: Confidence,
    /// Extra check on the match (e.g. SSN area numbers), `true` = keep.
    valid: fn(&str) -> bool,
}

pub struct Guard {
    rules: Vec<Rule>,
}

const MEDICAL_TERMS: &str = "patient|diagnos\\w*|prescri\\w*|diabetes|cancer|tumou?r|hiv|aids|hepatitis|depression|anxiety|bipolar|schizophreni\\w*|pregnan\\w*|asthma|dementia|alzheimer\\w*|chemotherapy|dialysis|surgery|admitted|discharged|medication|allerg\\w*|mg\\b";

/// Capitalised words that commonly start UI copy or stories, not names.
const NOT_NAMES: &[&str] = &[
    "As",
    "Given",
    "When",
    "Then",
    "And",
    "But",
    "The",
    "A",
    "An",
    "If",
    "User",
    "Users",
    "Patient",
    "Patients",
    "Doctor",
    "Book",
    "View",
    "Add",
    "Edit",
    "Show",
    "New",
    "My",
    "Our",
    "Dr",
    "Mr",
    "Mrs",
    "Ms",
    "Care",
    "Team",
    "Admin",
    "Home",
    "Settings",
    "Screen",
    "Page",
    "Button",
    "Story",
    "Epic",
    "Task",
    "Bug",
    "Appointment",
    "Appointments",
    "List",
    "Profile",
    "Record",
    "Records",
    "Visit",
    "Visits",
    "Login",
    "Sign",
    "Dashboard",
    "Search",
    "Open",
    "Save",
    "Cancel",
    "Submit",
];

fn any(_: &str) -> bool {
    true
}

/// US SSN: not area 000/666/9xx, group 00 or serial 0000.
fn valid_ssn(m: &str) -> bool {
    let digits: String = m.chars().filter(char::is_ascii_digit).collect();
    let (area, group, serial) = (&digits[0..3], &digits[3..5], &digits[5..9]);
    area != "000" && area != "666" && !area.starts_with('9') && group != "00" && serial != "0000"
}

fn ci(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .expect("built-in patterns compile")
}

impl Guard {
    pub fn new(custom: &[CustomPattern]) -> Result<Self, Error> {
        let date = r"(?:\d{1,2}[/.-]\d{1,2}[/.-]\d{2,4}|\d{4}-\d{2}-\d{2}|(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.? \d{1,2},? \d{4})";
        let high = |kind: &str, re: Regex, valid: fn(&str) -> bool| Rule {
            kind: kind.into(),
            re,
            confidence: Confidence::High,
            valid,
        };
        let mut rules = vec![
            high(
                "ssn",
                Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn"),
                valid_ssn,
            ),
            high(
                "mrn",
                ci(r"\b(?:mrn|medical record (?:number|no\.?|#))\s*[:#]?\s*[a-z0-9-]{4,}\b"),
                any,
            ),
            high(
                "dob",
                ci(&format!(
                    r"\b(?:dob|d\.o\.b\.|date of birth|born(?: on)?)\s*[:\-]?\s*{date}"
                )),
                any,
            ),
            high(
                "insurance-id",
                ci(
                    r"\b(?:member|policy|insurance|subscriber|medicaid|medicare)\s*(?:id|#|number|no\.?)\s*[:#]?\s*[a-z0-9-]{5,}\b",
                ),
                any,
            ),
            high(
                "email",
                ci(r"\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b"),
                any,
            ),
            high(
                "phone",
                Regex::new(r"(?:\+?1[\s.-]?)?(?:\(\d{3}\)\s?|\b\d{3}[\s.-])\d{3}[\s.-]\d{4}\b")
                    .expect("phone"),
                any,
            ),
            high(
                "address",
                ci(
                    r"\b\d{1,5}\s+(?:[a-z0-9]+\s+){1,4}(?:street|st|avenue|ave|road|rd|boulevard|blvd|lane|ln|drive|dr|court|ct|way|place|pl)\b\.?",
                ),
                any,
            ),
        ];
        // Name + medical term in either order, within one sentence.
        rules.push(Rule {
            kind: "name-with-medical-term".into(),
            re: Regex::new(&format!(
                r"(?i:\b(?:{MEDICAL_TERMS}))[^.\n]{{0,60}}?\b[A-Z][a-z]+ [A-Z][a-z]+\b|\b[A-Z][a-z]+ [A-Z][a-z]+\b[^.\n]{{0,60}}?(?i:\b(?:{MEDICAL_TERMS}))"
            ))
            .expect("name heuristic"),
            confidence: Confidence::Low,
            valid: |m| {
                // some adjacent capitalised pair must look like a name: neither word an
                // ordinary UI/story word ("Book Appointment", "Patient John" → "John Smith")
                let words: Vec<&str> = m.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
                let is_name = |w: &str| {
                    let mut c = w.chars();
                    c.next().is_some_and(char::is_uppercase) && c.all(char::is_lowercase) && !NOT_NAMES.contains(&w)
                };
                words.windows(2).any(|p| is_name(p[0]) && is_name(p[1]))
            },
        });
        for p in custom {
            rules.push(Rule {
                kind: p.name.clone(),
                re: Regex::new(&p.regex).map_err(|source| Error::Pattern {
                    name: p.name.clone(),
                    source,
                })?,
                confidence: Confidence::High,
                valid: any,
            });
        }
        Ok(Self { rules })
    }

    /// All findings, sorted, overlapping matches merged into the earlier one.
    pub fn scan(&self, text: &str) -> Vec<Finding> {
        let mut found: Vec<Finding> = self
            .rules
            .iter()
            .flat_map(|r| {
                r.re.find_iter(text)
                    .filter(|m| (r.valid)(m.as_str()))
                    .map(|m| Finding {
                        kind: r.kind.clone(),
                        start: m.start(),
                        end: m.end(),
                        confidence: r.confidence,
                    })
            })
            .collect();
        // high confidence wins over low when they overlap
        found.sort_by_key(|f| {
            (
                f.start,
                f.confidence == Confidence::Low,
                std::cmp::Reverse(f.end),
            )
        });
        let mut out: Vec<Finding> = Vec::new();
        for f in found {
            match out.last_mut() {
                Some(prev) if f.start < prev.end => prev.end = prev.end.max(f.end),
                _ => out.push(f),
            }
        }
        out
    }

    /// Replace the given findings with `[REDACTED:<kind>]`.
    pub fn redact(text: &str, findings: &[Finding]) -> String {
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for f in findings {
            out.push_str(&text[at..f.start]);
            out.push_str(&format!("[REDACTED:{}]", f.kind));
            at = f.end;
        }
        out.push_str(&text[at..]);
        out
    }

    pub fn check(&self, mode: ComplianceMode, text: &str) -> Outcome {
        if mode == ComplianceMode::Off {
            return Outcome::Clean;
        }
        let findings = self.scan(text);
        if findings.is_empty() {
            return Outcome::Clean;
        }
        let (high, low): (Vec<_>, Vec<_>) = findings
            .into_iter()
            .partition(|f| f.confidence == Confidence::High);
        match mode {
            ComplianceMode::Strict => Outcome::Redacted {
                text: Self::redact(text, &high),
                findings: high,
                remaining: low,
            },
            _ => {
                let redacted = Self::redact(text, &high);
                let mut all = high;
                all.extend(low);
                all.sort_by_key(|f| f.start);
                Outcome::Ask {
                    findings: all,
                    redacted,
                }
            }
        }
    }
}

/// One-line, value-free description: `ssn ×1, email ×2`.
pub fn summarize(findings: &[Finding]) -> String {
    summarize_kinds(findings.iter().map(|f| f.kind.as_str()))
}

/// Same as [`summarize`] from kind names.
pub fn summarize_kinds<'a>(kinds: impl IntoIterator<Item = &'a str>) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for k in kinds {
        match counts.iter_mut().find(|(c, _)| c == k) {
            Some((_, n)) => *n += 1,
            None => counts.push((k.to_string(), 1)),
        }
    }
    counts
        .iter()
        .map(|(k, n)| format!("{k} ×{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn g() -> Guard {
        Guard::new(&[]).unwrap()
    }

    fn kinds(text: &str) -> Vec<String> {
        g().scan(text).into_iter().map(|f| f.kind).collect()
    }

    #[test]
    fn detects_structured_identifiers() {
        assert_eq!(kinds("SSN 123-45-6789 on file"), ["ssn"]);
        assert_eq!(kinds("MRN: 00482913"), ["mrn"]);
        assert_eq!(kinds("Medical record number #A1234-55"), ["mrn"]);
        assert_eq!(kinds("DOB 04/12/1987"), ["dob"]);
        assert_eq!(kinds("date of birth: March 3, 1990"), ["dob"]);
        assert_eq!(kinds("Member ID: XJH4433219"), ["insurance-id"]);
        assert_eq!(kinds("mail jane.doe@example.org"), ["email"]);
        assert_eq!(kinds("call (555) 123-4567"), ["phone"]);
        assert_eq!(kinds("call +1 555.123.4567"), ["phone"]);
        assert_eq!(kinds("lives at 42 Elm Street"), ["address"]);
    }

    #[test]
    fn avoids_common_false_positives() {
        for clean in [
            "As a patient I can book an appointment",
            "Given the patient is signed in, When they open Book Appointment, Then they see slots",
            "Version 1.2.3 released on 2026-11-17",
            "SSN-like but invalid 000-12-3456 and 666-12-3456 and 900-12-3456",
            "Story points: 3, priority P1, size M",
            "Show patient list with 20 rows per page",
        ] {
            assert!(
                g().scan(clean)
                    .iter()
                    .all(|f| f.confidence == Confidence::Low)
                    && kinds(clean).iter().all(|k| k != "ssn"),
                "{clean}: {:?}",
                kinds(clean)
            );
            assert!(g().scan(clean).is_empty(), "{clean}: {:?}", kinds(clean));
        }
    }

    #[test]
    fn name_with_medical_term_is_low_confidence() {
        let f = g().scan("Patient John Smith was diagnosed last week");
        assert_eq!(f.len(), 1);
        assert_eq!(
            (f[0].kind.as_str(), f[0].confidence),
            ("name-with-medical-term", Confidence::Low)
        );
    }

    #[test]
    fn modes() {
        let text = "Follow up with jane@example.org about John Smith's diabetes";
        assert_eq!(g().check(ComplianceMode::Off, text), Outcome::Clean);
        match g().check(ComplianceMode::Warn, text) {
            Outcome::Ask { findings, redacted } => {
                assert_eq!(findings.len(), 2);
                assert!(redacted.contains("[REDACTED:email]") && redacted.contains("John Smith"));
            }
            o => panic!("{o:?}"),
        }
        match g().check(ComplianceMode::Strict, text) {
            Outcome::Redacted {
                text,
                findings,
                remaining,
            } => {
                assert!(!text.contains("jane@example.org"));
                assert_eq!(findings.len(), 1);
                assert_eq!(
                    remaining[0].kind, "name-with-medical-term",
                    "low confidence surfaced, not rewritten"
                );
            }
            o => panic!("{o:?}"),
        }
        assert_eq!(
            g().check(ComplianceMode::Strict, "Add a booking screen"),
            Outcome::Clean
        );
    }

    #[test]
    fn custom_patterns() {
        let p = CustomPattern {
            name: "clinic-id".into(),
            regex: r"\bCLN-\d{6}\b".into(),
        };
        let g = Guard::new(&[p]).unwrap();
        assert_eq!(g.scan("ref CLN-123456")[0].kind, "clinic-id");
        let bad = CustomPattern {
            name: "x".into(),
            regex: "(".into(),
        };
        assert!(Guard::new(&[bad]).is_err());
    }

    #[test]
    fn summary_never_contains_values() {
        let text = "SSN 123-45-6789, mail a@b.co and c@d.co";
        let s = summarize(&g().scan(text));
        assert_eq!(s, "ssn ×1, email ×2");
    }

    fn ssn() -> impl Strategy<Value = String> {
        (1u32..900, 1u32..100, 1u32..10000)
            .prop_filter("valid area", |(a, _, _)| *a != 666)
            .prop_map(|(a, g, s)| format!("{a:03}-{g:02}-{s:04}"))
    }

    proptest! {
        /// No false negatives on generated PHI embedded in ordinary prose.
        #[test]
        fn generated_phi_is_always_found(
            prefix in "[a-z ]{0,40}",
            suffix in "[a-z ]{0,40}",
            ssn in ssn(),
            mrn in "[0-9]{6,10}",
            user in "[a-z]{1,10}",
            host in "[a-z]{2,10}",
            (m, d, y) in (1u32..=12, 1u32..=28, 1930u32..2020),
            area in 200u32..999,
        ) {
            let g = g();
            for (sample, kind) in [
                (format!("SSN {ssn}"), "ssn"),
                (format!("MRN {mrn}"), "mrn"),
                (format!("{user}@{host}.com"), "email"),
                (format!("DOB {m}/{d}/{y}"), "dob"),
                (format!("({area}) 555-0142"), "phone"),
                (format!("member id {mrn}"), "insurance-id"),
            ] {
                let text = format!("{prefix} {sample} {suffix}");
                let found = g.scan(&text);
                prop_assert!(found.iter().any(|f| f.kind == kind), "{} not found in {:?}: {:?}", kind, text, found);
                let red = Guard::redact(&text, &found);
                prop_assert!(!red.contains(&sample), "redaction removes {}", kind);
            }
        }
    }
}
