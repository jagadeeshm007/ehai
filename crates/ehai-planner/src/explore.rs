//! Lite PRD gap-finder: `find_gaps` scans a PRD for the most common missing
//! pieces and returns at most 5 `PrdGap` items. Called by the `plan_explore`
//! MCP tool after every `save_prd` so the AI can refine the PRD without burning
//! extra tokens on a full re-write.

use crate::prd::Prd;

/// One gap in a PRD, with enough context for the AI to ask the user a focused
/// single question.
#[derive(Debug, Clone)]
pub struct PrdGap {
    /// Which PRD section is weak: `"flows"`, `"metrics"`, `"risks"`,
    /// `"scope"`, or `"users"`.
    pub section: &'static str,
    /// Human-readable question for the AI to surface to the user.
    pub question: String,
    /// A brief example or hint, e.g. `"e.g. 'If the API is down, use cached data'"`.
    pub hint: String,
}

/// Scan `prd` for the most important gaps. Returns at most 5, in priority order
/// (flows > metrics > risks > scope > users).
pub fn find_gaps(prd: &Prd) -> Vec<PrdGap> {
    let mut gaps: Vec<PrdGap> = Vec::new();

    // flows — highest priority
    if prd.flows.is_empty() {
        gaps.push(PrdGap {
            section: "flows",
            question: "No user flows described. What is the main step-by-step journey a user takes through this feature?".into(),
            hint: "e.g. User opens app → taps Book → picks slot → confirms → sees confirmation".into(),
        });
    } else if prd.flows.len() == 1 {
        let mermaid = prd.flows[0].mermaid.to_lowercase();
        let has_error =
            mermaid.contains("error") || mermaid.contains("fail") || mermaid.contains("fallback");
        if !has_error {
            gaps.push(PrdGap {
                section: "flows",
                question: "Only a happy-path flow exists. What should happen when something goes wrong (network error, validation failure, timeout)?".into(),
                hint: "e.g. 'If the API is down, show a retry button and use cached data'".into(),
            });
        }
    }

    // metrics
    for m in &prd.metrics {
        if !m.target.chars().any(|c| c.is_ascii_digit()) {
            gaps.push(PrdGap {
                section: "metrics",
                question: format!(
                    "Metric '{}' has no measurable number. What is the specific target?",
                    m.name
                ),
                hint: "e.g. '≥ 60% of bookings completed online within 3 months of launch'".into(),
            });
            break; // report at most one metric gap per pass
        }
    }

    // risks
    for r in &prd.risks {
        if r.mitigation.trim().len() < 15 {
            gaps.push(PrdGap {
                section: "risks",
                question: format!(
                    "Risk '{}' has no mitigation. How will you reduce or handle this risk?",
                    r.risk
                ),
                hint: "e.g. 'Feature-flag the rollout; monitor error rate; rollback if > 1%'"
                    .into(),
            });
            break;
        }
    }

    // scope
    if prd.out_of_scope.is_empty() {
        gaps.push(PrdGap {
            section: "scope",
            question: "Nothing is listed as out of scope. What related things are explicitly NOT part of this feature?".into(),
            hint: "e.g. 'Payments, notifications, admin dashboard, desktop web'".into(),
        });
    }

    // users
    if prd.users.len() < 2 {
        gaps.push(PrdGap {
            section: "users",
            question: "Only one user persona is described. Who else interacts with this feature (staff, admins, third-party systems)?".into(),
            hint: "e.g. 'Clinic receptionist who needs to override a booking'".into(),
        });
    }

    gaps.truncate(5);
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prd::{Flow, Metric, Risk};

    fn empty_prd() -> Prd {
        Prd {
            title: "Test".into(),
            problem: "p".into(),
            users: vec!["Patient".into()],
            goals: vec!["g".into()],
            scope: vec!["s".into()],
            ..Default::default()
        }
    }

    #[test]
    fn no_flows_reported() {
        let gaps = find_gaps(&empty_prd());
        assert!(gaps.iter().any(|g| g.section == "flows"), "{gaps:?}");
    }

    #[test]
    fn metric_without_number_reported() {
        let mut prd = empty_prd();
        prd.flows = vec![Flow {
            name: "book".into(),
            mermaid: "flowchart TD\n  A-->B\n  B-->Fail".into(),
        }];
        prd.metrics = vec![Metric {
            name: "adoption".into(),
            target: "increase bookings".into(), // no digit
        }];
        let gaps = find_gaps(&prd);
        assert!(gaps.iter().any(|g| g.section == "metrics"), "{gaps:?}");
    }

    #[test]
    fn metric_with_number_not_reported() {
        let mut prd = empty_prd();
        prd.flows = vec![
            Flow {
                name: "happy".into(),
                mermaid: "flowchart TD\n  A-->B".into(),
            },
            Flow {
                name: "err".into(),
                mermaid: "flowchart TD\n  A-->Error".into(),
            },
        ];
        prd.metrics = vec![Metric {
            name: "adoption".into(),
            target: "60% within 3 months".into(),
        }];
        prd.out_of_scope = vec!["Payments".into()];
        prd.users = vec!["Patient".into(), "Receptionist".into()];
        let gaps = find_gaps(&prd);
        // all gaps resolved
        assert!(gaps.is_empty(), "{gaps:?}");
    }

    #[test]
    fn risk_without_mitigation_reported() {
        let mut prd = empty_prd();
        prd.risks = vec![Risk {
            risk: "API down".into(),
            mitigation: "TBD".into(), // < 15 chars
        }];
        let gaps = find_gaps(&prd);
        assert!(gaps.iter().any(|g| g.section == "risks"), "{gaps:?}");
    }

    #[test]
    fn out_of_scope_empty_reported() {
        let prd = empty_prd();
        let gaps = find_gaps(&prd);
        assert!(gaps.iter().any(|g| g.section == "scope"), "{gaps:?}");
    }

    #[test]
    fn single_flow_with_error_handling_is_not_reported() {
        let mut prd = empty_prd();
        prd.flows = vec![Flow {
            name: "book".into(),
            mermaid: "flowchart TD\n  A-->B\n  B-->Fail".into(),
        }];
        let gaps = find_gaps(&prd);
        assert!(!gaps.iter().any(|g| g.section == "flows"), "{gaps:?}");
    }

    #[test]
    fn single_happy_path_flow_is_reported() {
        let mut prd = empty_prd();
        prd.flows = vec![Flow {
            name: "book".into(),
            mermaid: "flowchart TD\n  A-->B".into(),
        }];
        let gaps = find_gaps(&prd);
        assert!(gaps.iter().any(|g| g.section == "flows"), "{gaps:?}");
    }

    #[test]
    fn one_user_reported() {
        let prd = empty_prd();
        let gaps = find_gaps(&prd);
        assert!(gaps.iter().any(|g| g.section == "users"), "{gaps:?}");
    }

    #[test]
    fn capped_at_five() {
        // A PRD that triggers every gap rule
        let prd = Prd {
            title: "T".into(),
            problem: "p".into(),
            users: vec!["User".into()],
            goals: vec!["g".into()],
            scope: vec!["s".into()],
            metrics: vec![Metric {
                name: "m".into(),
                target: "good".into(),
            }],
            risks: vec![Risk {
                risk: "r".into(),
                mitigation: "ok".into(),
            }],
            ..Default::default()
        };
        assert!(find_gaps(&prd).len() <= 5);
    }
}
