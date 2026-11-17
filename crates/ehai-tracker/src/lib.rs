//! Tracker abstraction (spec §5.1). Every adapter implements [`Tracker`] and must
//! pass the shared contract suite (`tests/contract.rs`). GitHub Projects v2 is the
//! default; Linear and Jira come later behind the same trait.
//!
//! Items carry a stable `ehai-id` in an invisible footer of their body, so the
//! [`sync`] engine can match local plans to remote items and never duplicate.

use std::future::Future;

use serde::{Deserialize, Serialize};

pub mod auth;
#[cfg(feature = "fake")]
pub mod fake;
pub mod github;
pub mod memory;
pub mod sync;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GitHub authentication: {0}")]
    Auth(String),
    #[error("tracker API error: {0}")]
    Api(String),
    #[error("network error: {0}")]
    Http(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("not configured: {0}")]
    Config(String),
    #[error("not supported by this tracker: {0}")]
    Unsupported(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Single-select field values ehai manages. `LABELS` are what the tracker shows.
macro_rules! labeled_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub const LABELS: &'static [&'static str] = &[$($label),+];

            pub fn label(self) -> &'static str {
                match self { $($name::$variant => $label),+ }
            }

            /// Case-insensitive match on the label.
            pub fn from_label(s: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.label().eq_ignore_ascii_case(s.trim()))
            }
        }
    };
}

labeled_enum!(
    /// Board column.
    Status { Backlog => "Backlog", Todo => "Todo", InProgress => "In Progress", InReview => "In Review", Done => "Done" }
);
labeled_enum!(Priority { P0 => "P0", P1 => "P1", P2 => "P2", P3 => "P3" });
labeled_enum!(Size { Xs => "XS", S => "S", M => "M", L => "L", Xl => "XL" });
labeled_enum!(
    /// Work item type (`Type` field).
    Kind { Epic => "Epic", Story => "Story", Task => "Task", Bug => "Bug" }
);

/// A work item as the tracker has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Tracker-native id (GitHub: project item node id).
    pub id: String,
    pub number: Option<u64>,
    pub url: Option<String>,
    pub title: String,
    /// Body without the ehai footer.
    pub body: String,
    pub ehai_id: Option<String>,
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub size: Option<Size>,
    pub kind: Option<Kind>,
    pub closed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewItem {
    pub ehai_id: Option<String>,
    pub title: String,
    pub body: String,
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub size: Option<Size>,
    pub kind: Option<Kind>,
}

/// Fields to change; `None` leaves a field as it is. Status moves via
/// [`Tracker::set_status`] so planning never drags cards developers moved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemPatch {
    pub title: Option<String>,
    pub body: Option<String>,
    pub priority: Option<Priority>,
    pub size: Option<Size>,
    pub kind: Option<Kind>,
}

impl ItemPatch {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub status: Option<Status>,
    pub kind: Option<Kind>,
    pub ehai_id: Option<String>,
}

impl Filter {
    pub fn matches(&self, i: &Item) -> bool {
        self.status.is_none_or(|s| i.status == Some(s))
            && self.kind.is_none_or(|k| i.kind == Some(k))
            && self
                .ehai_id
                .as_ref()
                .is_none_or(|id| i.ehai_id.as_ref() == Some(id))
    }
}

/// What `ensure_schema` changed. Never destructive: existing fields and options
/// are kept; only missing ones are added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SchemaReport {
    pub created_fields: Vec<String>,
    pub added_options: Vec<String>,
    pub warnings: Vec<String>,
}

impl SchemaReport {
    pub fn is_noop(&self) -> bool {
        self.created_fields.is_empty() && self.added_options.is_empty()
    }
}

/// The tracker contract. Implementations must be idempotent where it makes sense
/// (`ensure_schema`) and must round-trip the `ehai-id`.
pub trait Tracker: Sync {
    /// Create missing Status/Priority/Size/Type/Iteration fields and options.
    fn ensure_schema(&self) -> impl Future<Output = Result<SchemaReport>> + Send;
    fn create_item(&self, item: NewItem) -> impl Future<Output = Result<Item>> + Send;
    fn update_item(&self, id: &str, patch: ItemPatch) -> impl Future<Output = Result<Item>> + Send;
    fn set_status(&self, id: &str, status: Status) -> impl Future<Output = Result<()>> + Send;
    fn list_items(&self, filter: &Filter) -> impl Future<Output = Result<Vec<Item>>> + Send;
    fn get_item(&self, id: &str) -> impl Future<Output = Result<Item>> + Send;
    fn comment(&self, id: &str, body: &str) -> impl Future<Output = Result<()>> + Send;
    fn link_pr(&self, id: &str, pr_url: &str) -> impl Future<Output = Result<()>> + Send;
}

const FOOTER_START: &str = "<!-- ehai-id: ";
const FOOTER_END: &str = " -->";

/// `ehai-id`s are `[A-Za-z0-9._-]{1,64}`: they live inside an HTML comment, so
/// nothing that could close it (`-->`) or inject markup is allowed.
pub fn valid_ehai_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !id.contains("--")
}

/// Append the invisible `<!-- ehai-id: … -->` footer.
pub fn with_footer(body: &str, ehai_id: Option<&str>) -> String {
    match ehai_id.filter(|id| valid_ehai_id(id)) {
        Some(id) => format!("{}\n\n{FOOTER_START}{id}{FOOTER_END}", body.trim_end()),
        None => body.to_string(),
    }
}

/// Split a stored body into (visible body, ehai-id).
pub fn split_footer(stored: &str) -> (String, Option<String>) {
    let Some(start) = stored.rfind(FOOTER_START) else {
        return (stored.to_string(), None);
    };
    let rest = &stored[start + FOOTER_START.len()..];
    let Some(end) = rest.find(FOOTER_END) else {
        return (stored.to_string(), None);
    };
    let id = &rest[..end];
    if !valid_ehai_id(id) || !rest[end + FOOTER_END.len()..].trim().is_empty() {
        return (stored.to_string(), None);
    }
    (stored[..start].trim_end().to_string(), Some(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn labels_round_trip() {
        for s in Status::ALL {
            assert_eq!(Status::from_label(s.label()), Some(*s));
        }
        assert_eq!(Status::from_label("in progress"), Some(Status::InProgress));
        assert_eq!(Size::from_label("xl"), Some(Size::Xl));
        assert_eq!(Kind::from_label("Chore"), None);
    }

    #[test]
    fn footer_rules() {
        let b = with_footer("Do the thing\n", Some("story-1.2"));
        assert_eq!(b, "Do the thing\n\n<!-- ehai-id: story-1.2 -->");
        assert_eq!(
            split_footer(&b),
            ("Do the thing".into(), Some("story-1.2".into()))
        );
        // hostile ids never make it into a body
        assert_eq!(with_footer("x", Some("a --> <script>")), "x");
        assert_eq!(with_footer("x", Some("a--b")), "x");
        // a footer followed by user text is not ours
        let edited = format!("{b}\nmore text");
        assert_eq!(split_footer(&edited).1, None);
        assert_eq!(split_footer("plain").1, None);
    }

    proptest! {
        #[test]
        fn footer_round_trips(body in "[^<]{0,200}", id in "[a-z0-9][a-z0-9._]{0,30}") {
            let stored = with_footer(&body, Some(&id));
            let (b, got) = split_footer(&stored);
            prop_assert_eq!(got, Some(id));
            prop_assert_eq!(b, body.trim_end());
        }
    }

    #[test]
    fn filter_matches() {
        let item = Item {
            id: "1".into(),
            number: None,
            url: None,
            title: "t".into(),
            body: String::new(),
            ehai_id: Some("a".into()),
            status: Some(Status::Todo),
            priority: None,
            size: None,
            kind: Some(Kind::Task),
            closed: false,
        };
        assert!(Filter::default().matches(&item));
        assert!(Filter {
            status: Some(Status::Todo),
            kind: Some(Kind::Task),
            ehai_id: Some("a".into())
        }
        .matches(&item));
        assert!(!Filter {
            status: Some(Status::Done),
            ..Default::default()
        }
        .matches(&item));
    }
}
