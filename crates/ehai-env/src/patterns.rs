//! Glob-ish pattern matching for the env file allow/deny lists (spec §6). `*`
//! matches any run of characters (including `/`); everything else is literal.

use regex::Regex;

/// Built-in patterns for gitignored config a fresh worktree needs to run.
/// Matched against the path relative to the repo root.
pub const BUILTIN: &[&str] = &[
    "*.env",
    "*.env.*",
    "*.local.*",
    "*config/local*",
    "*.npmrc",
    "*.yarnrc.yml",
    "*local.properties",
    "*google-services.json",
    "*GoogleService-Info.plist",
    "*app.config.local.*",
    "*.python-version",
    "*.tool-versions",
    "*docker-compose.override.yml",
    "*.vscode/settings.json",
];

/// `.env.example`/`.env.sample` are templates, never secrets — always excluded
/// even though they'd otherwise match `*.env.*`.
pub const ALWAYS_EXCLUDE: &[&str] = &["*.env.example", "*.env.sample"];

pub fn matches_any(path: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| matches(path, p))
}

pub fn matches(path: &str, pattern: &str) -> bool {
    glob_regex(pattern).is_match(path)
}

fn glob_regex(pattern: &str) -> Regex {
    let mut out = String::from("(?i)^");
    for (i, part) in pattern.split('*').enumerate() {
        if i > 0 {
            out.push_str(".*");
        }
        out.push_str(&regex::escape(part));
    }
    out.push('$');
    Regex::new(&out).expect("glob-derived regex is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_env_files_and_flags_examples_for_exclusion() {
        assert!(matches_any("apps/web/.env", BUILTIN));
        assert!(matches_any(".env.local", BUILTIN));
        assert!(matches_any("apps/web/.env.local", BUILTIN));
        // .env.example also matches the *.env.* built-in on its own; discover()
        // is what subtracts ALWAYS_EXCLUDE to keep it from being materialised.
        assert!(matches_any(".env.example", ALWAYS_EXCLUDE));
        assert!(matches_any("apps/web/.env.sample", ALWAYS_EXCLUDE));
    }

    #[test]
    fn matches_dotfiles_and_native_config() {
        assert!(matches_any(".npmrc", BUILTIN));
        assert!(matches_any("services/api/local.properties", BUILTIN));
        assert!(matches_any("android/app/google-services.json", BUILTIN));
        assert!(matches_any(".tool-versions", BUILTIN));
        assert!(!matches_any("README.md", BUILTIN));
    }

    #[test]
    fn config_globs_match_literally_with_star_wildcard() {
        assert!(matches(
            "apps/web/config.local.yaml",
            "apps/*/config.local.yaml"
        ));
        assert!(!matches("apps/web/config.yaml", "apps/*/config.local.yaml"));
    }
}
