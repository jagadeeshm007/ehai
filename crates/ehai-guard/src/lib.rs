//! Compliance guard (spec §9): find sensitive data/PII in text ehai is about to
//! write to a tracker, a PRD, a POC fixture, a log or a report, and decide what
//! happens:
//!
//! - `off`: nothing is checked.
//! - `warn`: findings go back to the user, who chooses redact / keep / cancel.
//! - `strict`: high-confidence findings are redacted automatically; low-confidence
//!   ones are surfaced, never silently rewritten.
//!
//! Findings record *what kind* of data was found and *where*, never the value.
//! Which rules are active depends on the configured [`ComplianceProfile`].

use ehai_core::config::{ComplianceMode, ComplianceProfile, CustomPattern};
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
    /// Structured identifiers (SSN, credit card, email, phone…).
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
    /// Extra check on the match, `true` = keep.
    valid: fn(&str) -> bool,
}

pub struct Guard {
    rules: Vec<Rule>,
}

// ── Health profile constants ───────────────────────────────────────────────

const HEALTH_TERMS: &str = "patient|diagnos\\w*|prescri\\w*|diabetes|cancer|tumou?r|hiv|aids|hepatitis|depression|anxiety|bipolar|schizophreni\\w*|pregnan\\w*|asthma|dementia|alzheimer\\w*|chemotherapy|dialysis|surgery|admitted|discharged|medication|allerg\\w*|mg\\b";

/// Capitalised words that commonly start UI copy or stories, not personal names.
const NOT_NAMES: &[&str] = &[
    "As", "Given", "When", "Then", "And", "But", "The", "A", "An", "If",
    "User", "Users", "Patient", "Patients", "Doctor", "Book", "View", "Add",
    "Edit", "Show", "New", "My", "Our", "Dr", "Mr", "Mrs", "Ms", "Care",
    "Team", "Admin", "Home", "Settings", "Screen", "Page", "Button", "Story",
    "Epic", "Task", "Bug", "Appointment", "Appointments", "List", "Profile",
    "Record", "Records", "Visit", "Visits", "Login", "Sign", "Dashboard",
    "Search", "Open", "Save", "Cancel", "Submit",
];

// ── Helpers ────────────────────────────────────────────────────────────────

fn any(_: &str) -> bool {
    true
}

fn ci(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .expect("built-in patterns compile")
}

fn high(kind: &str, re: Regex, valid: fn(&str) -> bool) -> Rule {
    Rule { kind: kind.into(), re, confidence: Confidence::High, valid }
}

fn low(kind: &str, re: Regex, valid: fn(&str) -> bool) -> Rule {
    Rule { kind: kind.into(), re, confidence: Confidence::Low, valid }
}

/// US SSN: not area 000/666/9xx, group 00 or serial 0000.
fn valid_ssn(m: &str) -> bool {
    let digits: String = m.chars().filter(char::is_ascii_digit).collect();
    let (area, group, serial) = (&digits[0..3], &digits[3..5], &digits[5..9]);
    area != "000" && area != "666" && !area.starts_with('9') && group != "00" && serial != "0000"
}

/// Luhn check for credit card numbers (strips non-digits first).
fn valid_luhn(m: &str) -> bool {
    let digits: Vec<u32> = m.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let v = d * 2;
                if v > 9 { v - 9 } else { v }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

// ── Rule sets by profile ───────────────────────────────────────────────────

fn tech_rules() -> Vec<Rule> {
    let date = r"(?:\d{1,2}[/.-]\d{1,2}[/.-]\d{2,4}|\d{4}-\d{2}-\d{2}|(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.? \d{1,2},? \d{4})";
    vec![
        high("ssn", Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn"), valid_ssn),
        high("dob", ci(&format!(r"\b(?:dob|d\.o\.b\.|date of birth|born(?: on)?)\s*[:\-]?\s*{date}")), any),
        high("email", ci(r"\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b"), any),
        high("phone", Regex::new(r"(?:\+?1[\s.-]?)?(?:\(\d{3}\)\s?|\b\d{3}[\s.-])\d{3}[\s.-]\d{4}\b").expect("phone"), any),
        high(
            "address",
            ci(r"\b\d{1,5}\s+(?:[a-z0-9]+\s+){1,4}(?:street|st|avenue|ave|road|rd|boulevard|blvd|lane|ln|drive|dr|court|ct|way|place|pl)\b\.?"),
            any,
        ),
    ]
}

fn health_rules() -> Vec<Rule> {
    vec![
        high(
            "mrn",
            ci(r"\b(?:mrn|medical record (?:number|no\.?|#))\s*[:#]?\s*[a-z0-9-]{4,}\b"),
            any,
        ),
        high(
            "insurance-id",
            ci(r"\b(?:member|policy|insurance|subscriber|medicaid|medicare)\s*(?:id|#|number|no\.?)\s*[:#]?\s*[a-z0-9-]{5,}\b"),
            any,
        ),
        // Name + medical term in either order, within one sentence.
        low(
            "name-with-medical-term",
            Regex::new(&format!(
                r"(?i:\b(?:{HEALTH_TERMS}))[^.\n]{{0,60}}?\b[A-Z][a-z]+ [A-Z][a-z]+\b|\b[A-Z][a-z]+ [A-Z][a-z]+\b[^.\n]{{0,60}}?(?i:\b(?:{HEALTH_TERMS}))"
            )).expect("name heuristic"),
            |m| {
                let words: Vec<&str> = m.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
                let is_name = |w: &str| {
                    let mut c = w.chars();
                    c.next().is_some_and(char::is_uppercase) && c.all(char::is_lowercase) && !NOT_NAMES.contains(&w)
                };
                words.windows(2).any(|p| is_name(p[0]) && is_name(p[1]))
            },
        ),
    ]
}

fn finance_rules() -> Vec<Rule> {
    vec![
        // Credit card: 13-19 digits (may include spaces/dashes), Luhn-validated.
        high(
            "credit-card",
            Regex::new(r"\b(?:\d[ -]?){13,19}\b").expect("credit-card"),
            valid_luhn,
        ),
        // IBAN: country code + check digits + BBAN (structural only, no checksum).
        high(
            "iban",
            Regex::new(r"\b[A-Z]{2}\d{2}[A-Z0-9]{4,32}\b").expect("iban"),
            any,
        ),
        // US ABA routing number, keyword-anchored to cut false positives.
        high(
            "routing-number",
            ci(r"\b(?:routing|ABA)[\s:/-]*(?:number|no\.?)?\s*[:#]?\s*\d{9}\b"),
            any,
        ),
    ]
}

fn gdpr_rules() -> Vec<Rule> {
    vec![
        // Simplified EU passport: 1-2 uppercase letters + 7-9 digits.
        high(
            "eu-passport",
            Regex::new(r"\b[A-Z]{1,2}\d{7,9}\b").expect("eu-passport"),
            any,
        ),
        // Data-subject language near a name-like pair (Low confidence).
        low(
            "gdpr-data-subject",
            ci(r"\b(?:data subject|right to erasure|right to be forgotten|gdpr|consent withdrawn?)[^.\n]{0,80}?\b[A-Z][a-z]+ [A-Z][a-z]+\b|\b[A-Z][a-z]+ [A-Z][a-z]+\b[^.\n]{0,80}?\b(?:data subject|right to erasure|right to be forgotten|gdpr|consent withdrawn?)"),
            any,
        ),
    ]
}

// ── Guard ──────────────────────────────────────────────────────────────────

impl Guard {
    pub fn new(profile: ComplianceProfile, custom: &[CustomPattern]) -> Result<Self, Error> {
        let mut rules = tech_rules();
        match profile {
            ComplianceProfile::Tech => {}
            ComplianceProfile::Health => rules.extend(health_rules()),
            ComplianceProfile::Finance => rules.extend(finance_rules()),
            ComplianceProfile::Gdpr => rules.extend(gdpr_rules()),
        }
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
        Guard::new(ComplianceProfile::Tech, &[]).unwrap()
    }

    fn g_health() -> Guard {
        Guard::new(ComplianceProfile::Health, &[]).unwrap()
    }

    fn g_finance() -> Guard {
        Guard::new(ComplianceProfile::Finance, &[]).unwrap()
    }

    fn g_gdpr() -> Guard {
        Guard::new(ComplianceProfile::Gdpr, &[]).unwrap()
    }

    fn kinds_of(guard: &Guard, text: &str) -> Vec<String> {
        guard.scan(text).into_iter().map(|f| f.kind).collect()
    }

    fn kinds(text: &str) -> Vec<String> {
        kinds_of(&g(), text)
    }

    #[test]
    fn detects_tech_identifiers() {
        assert_eq!(kinds("SSN 123-45-6789 on file"), ["ssn"]);
        assert_eq!(kinds("DOB 04/12/1987"), ["dob"]);
        assert_eq!(kinds("date of birth: March 3, 1990"), ["dob"]);
        assert_eq!(kinds("mail jane.doe@example.org"), ["email"]);
        assert_eq!(kinds("call (555) 123-4567"), ["phone"]);
        assert_eq!(kinds("call +1 555.123.4567"), ["phone"]);
        assert_eq!(kinds("lives at 42 Elm Street"), ["address"]);
    }

    #[test]
    fn detects_health_identifiers() {
        let h = g_health();
        assert_eq!(kinds_of(&h, "MRN: 00482913"), ["mrn"]);
        assert_eq!(kinds_of(&h, "Medical record number #A1234-55"), ["mrn"]);
        assert_eq!(kinds_of(&h, "Member ID: XJH4433219"), ["insurance-id"]);
        // Tech rules still active in health profile
        assert_eq!(kinds_of(&h, "SSN 123-45-6789"), ["ssn"]);
    }

    #[test]
    fn detects_finance_identifiers() {
        let f = g_finance();
        // Visa test card — passes Luhn
        assert_eq!(kinds_of(&f, "card 4111111111111111"), ["credit-card"]);
        assert_eq!(kinds_of(&f, "IBAN DE89370400440532013000"), ["iban"]);
        assert_eq!(kinds_of(&f, "routing number 021000021"), ["routing-number"]);
        // Tech rules still active
        assert_eq!(kinds_of(&f, "user@example.com"), ["email"]);
    }

    #[test]
    fn detects_gdpr_identifiers() {
        let g_g = g_gdpr();
        assert_eq!(kinds_of(&g_g, "passport PA1234567"), ["eu-passport"]);
        // Tech rules still active
        assert_eq!(kinds_of(&g_g, "user@example.com"), ["email"]);
    }

    #[test]
    fn mrn_not_detected_in_tech_profile() {
        // MRN should NOT trigger in the default tech profile
        assert!(g().scan("MRN: 00482913").is_empty());
    }

    #[test]
    fn credit_card_not_detected_in_tech_profile() {
        assert!(g().scan("4111111111111111").is_empty());
    }

    #[test]
    fn luhn_rejects_invalid_cards() {
        // 4111111111111112 fails Luhn
        assert!(g_finance().scan("card 4111111111111112").is_empty());
    }

    #[test]
    fn avoids_common_false_positives() {
        for clean in [
            "As a user I can book an appointment",
            "Given the user is signed in, When they open Book Appointment, Then they see slots",
            "Version 1.2.3 released on 2026-11-18",
            "SSN-like but invalid 000-12-3456 and 666-12-3456 and 900-12-3456",
            "Story points: 3, priority P1, size M",
            "Show user list with 20 rows per page",
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
        let f = g_health().scan("Patient John Smith was diagnosed last week");
        assert_eq!(f.len(), 1);
        assert_eq!(
            (f[0].kind.as_str(), f[0].confidence),
            ("name-with-medical-term", Confidence::Low)
        );
    }

    #[test]
    fn modes() {
        let guard = g_health();
        let text = "Follow up with jane@example.org about John Smith's diabetes";
        assert_eq!(guard.check(ComplianceMode::Off, text), Outcome::Clean);
        match guard.check(ComplianceMode::Warn, text) {
            Outcome::Ask { findings, redacted } => {
                assert_eq!(findings.len(), 2);
                assert!(redacted.contains("[REDACTED:email]") && redacted.contains("John Smith"));
            }
            o => panic!("{o:?}"),
        }
        match guard.check(ComplianceMode::Strict, text) {
            Outcome::Redacted { text, findings, remaining } => {
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
            guard.check(ComplianceMode::Strict, "Add a booking screen"),
            Outcome::Clean
        );
    }

    #[test]
    fn custom_patterns() {
        let p = CustomPattern {
            name: "clinic-id".into(),
            regex: r"\bCLN-\d{6}\b".into(),
        };
        let g = Guard::new(ComplianceProfile::Tech, &[p]).unwrap();
        assert_eq!(g.scan("ref CLN-123456")[0].kind, "clinic-id");
        let bad = CustomPattern {
            name: "x".into(),
            regex: "(".into(),
        };
        assert!(Guard::new(ComplianceProfile::Tech, &[bad]).is_err());
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
        /// No false negatives on generated tech PII embedded in ordinary prose.
        #[test]
        fn generated_tech_pii_always_found(
            prefix in "[a-z ]{0,40}",
            suffix in "[a-z ]{0,40}",
            ssn in ssn(),
            user in "[a-z]{1,10}",
            host in "[a-z]{2,10}",
            (m, d, y) in (1u32..=12, 1u32..=28, 1930u32..2020),
            area in 200u32..999,
        ) {
            let g = g();
            for (sample, kind) in [
                (format!("SSN {ssn}"), "ssn"),
                (format!("{user}@{host}.com"), "email"),
                (format!("DOB {m}/{d}/{y}"), "dob"),
                (format!("({area}) 555-0142"), "phone"),
            ] {
                let text = format!("{prefix} {sample} {suffix}");
                let found = g.scan(&text);
                prop_assert!(found.iter().any(|f| f.kind == kind), "{} not found in {:?}: {:?}", kind, text, found);
                let red = Guard::redact(&text, &found);
                prop_assert!(!red.contains(&sample), "redaction removes {}", kind);
            }
        }

        /// No false negatives on generated health PII.
        #[test]
        fn generated_health_pii_always_found(
            prefix in "[a-z ]{0,40}",
            suffix in "[a-z ]{0,40}",
            mrn in "[0-9]{6,10}",
            member_id in "[a-z0-9]{5,10}",
        ) {
            let g = g_health();
            for (sample, kind) in [
                (format!("MRN {mrn}"), "mrn"),
                (format!("member id {member_id}"), "insurance-id"),
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
