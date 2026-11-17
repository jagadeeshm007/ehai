//! Append-only audit log (spec §9): one JSON line per write ehai makes to an
//! outside system or shared file — what kind of thing, where, by which command,
//! which kinds of sensitive data were found and what was done. Never the values.
//! Files: `~/.ehai/audit/YYYY-MM.jsonl`.

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{Error, Finding};

#[derive(Debug, Clone, Serialize)]
pub struct Event<'a> {
    /// Unix seconds.
    pub ts: u64,
    pub command: &'a str,
    /// What was written: `tracker-item`, `prd`, `report`...
    pub target: &'a str,
    /// Where: an issue URL, a file path, a project.
    pub location: &'a str,
    /// Kinds only, e.g. `["ssn", "email"]`.
    pub findings: Vec<String>,
    /// `clean`, `redacted`, `kept`, `cancelled`.
    pub action: &'a str,
}

impl<'a> Event<'a> {
    pub fn new(
        command: &'a str,
        target: &'a str,
        location: &'a str,
        findings: &[Finding],
        action: &'a str,
    ) -> Self {
        let kinds = findings.iter().map(|f| f.kind.clone()).collect();
        Self::with_kinds(command, target, location, kinds, action)
    }

    pub fn with_kinds(
        command: &'a str,
        target: &'a str,
        location: &'a str,
        kinds: Vec<String>,
        action: &'a str,
    ) -> Self {
        Self {
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            command,
            target,
            location,
            findings: kinds,
            action,
        }
    }
}

/// Append `event` to this month's file, under a lock so parallel sessions don't interleave.
pub fn record(home: &Path, event: &Event) -> Result<(), Error> {
    let dir = ehai_core::paths::audit_dir(home);
    std::fs::create_dir_all(&dir).map_err(|e| Error::Audit(e.to_string()))?;
    let month = &ehai_core::date::civil((event.ts / 86_400) as i64)[..7];
    let path = dir.join(format!("{month}.jsonl"));
    let _lock = ehai_core::migrate::lock(&path).map_err(|e| Error::Audit(e.to_string()))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| Error::Audit(e.to_string()))?;
    let line = serde_json::to_string(event).map_err(|e| Error::Audit(e.to_string()))?;
    writeln!(f, "{line}").map_err(|e| Error::Audit(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Guard;

    #[test]
    fn appends_value_free_lines() {
        let d = tempfile::tempdir().unwrap();
        let text = "SSN 123-45-6789";
        let findings = Guard::new(&[]).unwrap().scan(text);
        for action in ["redacted", "kept"] {
            record(
                d.path(),
                &Event::new("sync", "tracker-item", "octo #1", &findings, action),
            )
            .unwrap();
        }
        let files: Vec<_> = std::fs::read_dir(d.path().join("audit"))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
            .collect();
        assert_eq!(files.len(), 1);
        let body = std::fs::read_to_string(files[0].path()).unwrap();
        assert_eq!(body.lines().count(), 2);
        assert!(
            body.contains(r#""findings":["ssn"]"#) && !body.contains("6789"),
            "{body}"
        );
    }
}
