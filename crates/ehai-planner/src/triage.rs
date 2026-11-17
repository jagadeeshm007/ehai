//! `/ehai:triage`: before filing a bug or request, look for items that already
//! cover it. The harness classifies and prioritises; this finds likely duplicates.

use std::collections::BTreeSet;

use ehai_tracker::Item;
use serde::Serialize;

/// Words too common to say anything about duplication.
const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "to", "of", "in", "on", "for", "with", "is", "it", "be", "can",
    "when", "i", "we", "user", "users", "app", "not", "does", "doesn", "t", "should", "add", "fix",
    "bug", "issue", "this", "that", "as", "at", "from", "by", "my",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
    /// 0.0–1.0 word overlap with the new report.
    pub score: f32,
}

fn words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .map(|w| w.trim_end_matches('s').to_string()) // crude plural folding
        .filter(|w| w.len() > 1 && !STOP.contains(&w.as_str()))
        .collect()
}

/// Items whose title overlaps the new one by at least `threshold` (Jaccard on
/// content words, title weighted over body), best first, at most 5.
pub fn duplicates(
    title: &str,
    description: &str,
    items: &[Item],
    threshold: f32,
) -> Vec<Candidate> {
    let t = words(title);
    let d = words(description);
    let mut out: Vec<Candidate> = items
        .iter()
        .filter(|i| !i.closed)
        .filter_map(|i| {
            let it = words(&i.title);
            let score = 0.75 * jaccard(&t, &it)
                + 0.25
                    * jaccard(
                        &t.union(&d).cloned().collect(),
                        &words(&format!("{} {}", i.title, i.body)),
                    );
            (score >= threshold).then(|| Candidate {
                id: i.id.clone(),
                title: i.title.clone(),
                url: i.url.clone(),
                score,
            })
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(5);
    out
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f32 / union as f32
}

pub const DEFAULT_THRESHOLD: f32 = 0.35;

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, title: &str, body: &str) -> Item {
        Item {
            id: id.into(),
            number: None,
            url: None,
            title: title.into(),
            body: body.into(),
            ehai_id: None,
            status: None,
            priority: None,
            size: None,
            kind: None,
            closed: false,
            updated_at: None,
            assignees: vec![],
        }
    }

    #[test]
    fn finds_likely_duplicates_only() {
        let items = [
            item(
                "1",
                "Login fails with expired password",
                "Users see a blank screen",
            ),
            item("2", "Add dark mode", ""),
            item("3", "Booking confirmation email not sent", ""),
            Item {
                closed: true,
                ..item("4", "Login fails after password expiry", "")
            },
        ];
        let d = duplicates(
            "Login fails when password expired",
            "blank screen after login",
            &items,
            DEFAULT_THRESHOLD,
        );
        assert_eq!(
            d.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["1"],
            "{d:?}"
        );
        assert!(
            duplicates("Export visit history as PDF", "", &items, DEFAULT_THRESHOLD).is_empty()
        );
    }
}
