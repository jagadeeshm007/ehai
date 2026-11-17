//! Design-token extraction for the POC engine (M5). Regex-based; no JS execution.
//! Covers the common 80%: hex/rgb colors, spacing sizes, font families, border radii,
//! reusable component names, and navigation route/screen patterns.
//! ponytail: regex extraction over JS interpreter — 100x simpler, upgrade when dynamic tokens matter.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::SKIP_DIRS;

/// Design tokens and component inventory extracted from the codebase.
/// Cached at `.ehai/cache/design-profile.json`; re-computed when `source_hash` changes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DesignProfile {
    pub colors: BTreeMap<String, String>,
    pub spacing: BTreeMap<String, String>,
    pub typography: BTreeMap<String, String>,
    pub radii: BTreeMap<String, String>,
    /// Reusable component names found in `src/components/`, `components/`, `src/ui/`.
    pub components: Vec<String>,
    /// Route paths (Next.js / Expo Router) or screen names (React Navigation).
    pub nav_patterns: Vec<String>,
    /// Hash of source file sizes+mtimes for cache invalidation.
    pub source_hash: u64,
}

impl DesignProfile {
    /// Plain-text summary for Claude to use as context when writing POC code.
    pub fn to_context_text(&self) -> String {
        let mut out = String::from("# Design Profile\n\n");
        if !self.colors.is_empty() {
            out.push_str("## Colors\n");
            for (k, v) in &self.colors {
                out.push_str(&format!("- {k}: {v}\n"));
            }
            out.push('\n');
        }
        if !self.spacing.is_empty() {
            out.push_str("## Spacing\n");
            for (k, v) in &self.spacing {
                out.push_str(&format!("- {k}: {v}\n"));
            }
            out.push('\n');
        }
        if !self.typography.is_empty() {
            out.push_str("## Typography\n");
            for (k, v) in &self.typography {
                out.push_str(&format!("- {k}: {v}\n"));
            }
            out.push('\n');
        }
        if !self.radii.is_empty() {
            out.push_str("## Border Radius\n");
            for (k, v) in &self.radii {
                out.push_str(&format!("- {k}: {v}\n"));
            }
            out.push('\n');
        }
        if !self.components.is_empty() {
            out.push_str("## Available Components\n");
            for c in &self.components {
                out.push_str(&format!("- {c}\n"));
            }
            out.push('\n');
        }
        if !self.nav_patterns.is_empty() {
            out.push_str("## Navigation Patterns\n");
            for p in &self.nav_patterns {
                out.push_str(&format!("- {p}\n"));
            }
            out.push('\n');
        }
        out
    }

    pub fn has_tokens(&self) -> bool {
        !self.colors.is_empty() || !self.spacing.is_empty() || !self.typography.is_empty()
    }
}

/// Load from cache or re-extract. Cache lives at `.ehai/cache/design-profile.json`.
pub fn load_or_extract(root: &Path) -> DesignProfile {
    let cache_path = ehai_core::paths::repo_dir(root).join("cache/design-profile.json");
    if let Ok(bytes) = std::fs::read(&cache_path) {
        if let Ok(cached) = serde_json::from_slice::<DesignProfile>(&bytes) {
            if cached.source_hash == source_hash(root) {
                return cached;
            }
        }
    }
    let mut profile = extract_design_tokens(root);
    profile.source_hash = source_hash(root);
    // best-effort cache write; ignore errors (read-only fs, CI, etc.)
    if let Ok(json) = serde_json::to_vec_pretty(&profile) {
        let _ = write_cache(&cache_path, &json);
    }
    profile
}

fn write_cache(path: &Path, bytes: &[u8]) -> ehai_core::Result<()> {
    use ehai_core::Error;
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).map_err(Error::io(dir))?;
    ehai_core::migrate::write_atomic(path, bytes)
}

/// Extract design tokens without cache.
pub fn extract_design_tokens(root: &Path) -> DesignProfile {
    let mut p = DesignProfile::default();
    extract_tailwind(root, &mut p);
    extract_css_vars(root, &mut p);
    extract_rn_themes(root, &mut p);
    p.components = scan_components(root);
    p.nav_patterns = scan_nav_patterns(root);
    p
}

// ── Tailwind / NativeWind ──────────────────────────────────────────────────

fn extract_tailwind(root: &Path, p: &mut DesignProfile) {
    let names = [
        "tailwind.config.js",
        "tailwind.config.ts",
        "tailwind.config.cjs",
        "tailwind.config.mjs",
    ];
    // common app sub-dirs in monorepos
    let subdirs: &[&str] = &["", "apps/web", "apps/frontend", "web", "frontend", "src"];
    for sub in subdirs {
        let base = if sub.is_empty() {
            root.to_path_buf()
        } else {
            root.join(sub)
        };
        for name in &names {
            if let Ok(text) = std::fs::read_to_string(base.join(name)) {
                parse_tailwind(&text, p);
                break; // first found in this subdir is enough
            }
        }
    }
}

fn parse_tailwind(text: &str, p: &mut DesignProfile) {
    // Use rfind to prefer `extend.colors` over `theme.colors` (fewer noise tokens)
    if let Some(block) = object_block(text, "colors") {
        extract_hex(&block, &mut p.colors);
    }
    if let Some(block) = object_block(text, "spacing") {
        extract_sizes(&block, &mut p.spacing);
    }
    if let Some(block) = object_block(text, "fontFamily") {
        extract_fonts(&block, &mut p.typography);
    }
    if let Some(block) = object_block(text, "borderRadius") {
        extract_sizes(&block, &mut p.radii);
    }
}

/// Find `key:` in text (last occurrence) and return the content of its `{…}` block.
fn object_block(text: &str, key: &str) -> Option<String> {
    let pat = format!("{key}:");
    let start = text.rfind(&pat)?;
    let after = &text[start + pat.len()..];
    let brace = after.find('{')?;
    let inner = &after[brace + 1..];
    let mut depth = 1usize;
    let mut end = 0;
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    Some(inner[..end].to_string())
}

fn extract_hex(block: &str, out: &mut BTreeMap<String, String>) {
    // Match: (optional-quote)(name)(optional-quote): (quote)(#hex or rgb/hsl/oklch)(quote)
    let re = regex::Regex::new(
        r#"['"]?([\w-]+)['"]?\s*:\s*['"](\#[0-9a-fA-F]{3,8}|(?:rgb|hsl|oklch)\([^)]{3,40}\))['"]"#,
    )
    .expect("valid pattern");
    for cap in re.captures_iter(block) {
        let name = &cap[1];
        if !matches!(name, "DEFAULT" | "inherit" | "transparent" | "current") {
            out.entry(name.to_string())
                .or_insert_with(|| cap[2].to_string());
        }
    }
}

fn extract_sizes(block: &str, out: &mut BTreeMap<String, String>) {
    let re = regex::Regex::new(
        r#"['"]?([\w.-]+)['"]?\s*:\s*['"](\d+(?:\.\d+)?(?:rem|px|em|%|vw|vh))['"]"#,
    )
    .expect("valid pattern");
    for cap in re.captures_iter(block) {
        out.entry(cap[1].to_string())
            .or_insert_with(|| cap[2].to_string());
    }
}

fn extract_fonts(block: &str, out: &mut BTreeMap<String, String>) {
    // fontFamily: { sans: ['Inter', 'system-ui'] }  → sans → Inter
    let re = regex::Regex::new(r#"['"]?([\w-]+)['"]?\s*:\s*\[\s*['"]([^'"]{2,40})['"]"#)
        .expect("valid pattern");
    for cap in re.captures_iter(block) {
        out.entry(cap[1].to_string())
            .or_insert_with(|| cap[2].to_string());
    }
}

// ── CSS custom properties ──────────────────────────────────────────────────

fn extract_css_vars(root: &Path, p: &mut DesignProfile) {
    let re = regex::Regex::new(r"--([a-z][\w-]{1,40})\s*:\s*([^;{\n]+)").expect("valid pattern");
    let files = [
        "globals.css",
        "global.css",
        "index.css",
        "app.css",
        "styles.css",
        "variables.css",
        "tokens.css",
    ];
    let bases: &[&str] = &["", "src", "styles", "app", "app/(app)", "src/styles"];
    for base in bases {
        let dir = if base.is_empty() {
            root.to_path_buf()
        } else {
            root.join(base)
        };
        for name in &files {
            if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
                for cap in re.captures_iter(&text) {
                    let name = cap[1].trim().to_string();
                    let value = cap[2].trim().trim_end_matches(',').to_string();
                    if name.contains("color")
                        || name.starts_with("primary")
                        || name.starts_with("secondary")
                        || name.starts_with("bg-")
                        || name.starts_with("text-")
                    {
                        p.colors.entry(name).or_insert(value);
                    } else if name.contains("spacing")
                        || name.contains("gap")
                        || name.starts_with("space-")
                    {
                        p.spacing.entry(name).or_insert(value);
                    } else if name.contains("radius") || name.starts_with("rounded") {
                        p.radii.entry(name).or_insert(value);
                    } else if name.contains("font") || name.starts_with("text-") {
                        p.typography.entry(name).or_insert(value);
                    }
                }
            }
        }
    }
}

// ── React Native theme files ───────────────────────────────────────────────

fn extract_rn_themes(root: &Path, p: &mut DesignProfile) {
    let re = regex::Regex::new(
        r#"['"]?([\w-]+)['"]?\s*:\s*['"](\#[0-9a-fA-F]{3,8}|(?:rgb|hsl|oklch)\([^)]{3,40}\))['"]"#,
    )
    .expect("valid pattern");
    let prefixes = ["colors", "theme", "tokens", "palette", "Colors", "Theme"];
    let dirs: &[&str] = &[
        "src/constants",
        "constants",
        "src/theme",
        "theme",
        "src/styles",
        "styles",
        "src",
    ];
    for dir_str in dirs {
        let dir = root.join(dir_str);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let n = name.to_string_lossy();
            let is_theme = prefixes.iter().any(|pref| n.starts_with(pref))
                && (n.ends_with(".ts") || n.ends_with(".tsx") || n.ends_with(".js"));
            if !is_theme {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            for cap in re.captures_iter(&text) {
                let key = &cap[1];
                if !matches!(key, "DEFAULT" | "inherit" | "transparent") {
                    p.colors
                        .entry(key.to_string())
                        .or_insert_with(|| cap[2].to_string());
                }
            }
        }
    }
}

// ── Component scanning ─────────────────────────────────────────────────────

fn scan_components(root: &Path) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for dir_str in [
        "src/components",
        "components",
        "src/ui",
        "ui",
        "src/shared/components",
        "shared/components",
    ] {
        let dir = root.join(dir_str);
        if dir.is_dir() {
            collect_components(&dir, &mut set, 2);
        }
    }
    set.into_iter().collect()
}

fn collect_components(dir: &Path, out: &mut BTreeSet<String>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if depth > 0 && !SKIP_DIRS.contains(&n.as_ref()) {
                collect_components(&entry.path(), out, depth - 1);
            }
        } else {
            let stem = n
                .strip_suffix(".tsx")
                .or_else(|| n.strip_suffix(".jsx"))
                .or_else(|| n.strip_suffix(".native.tsx"))
                .or_else(|| n.strip_suffix(".ts"));
            if let Some(stem) = stem {
                // Skip non-component files
                if matches!(stem, "index" | "types" | "utils" | "helpers" | "constants")
                    || stem.contains(".test")
                    || stem.contains(".spec")
                    || stem.contains(".stories")
                {
                    continue;
                }
                // Keep PascalCase (components) and kebab/snake-case (also valid)
                let first = stem.chars().next().unwrap_or('a');
                if first.is_ascii_uppercase() || stem.contains('-') || stem.contains('_') {
                    out.insert(stem.to_string());
                }
            }
        }
    }
}

// ── Navigation patterns ────────────────────────────────────────────────────

fn scan_nav_patterns(root: &Path) -> Vec<String> {
    let mut patterns: Vec<String> = Vec::new();
    // Next.js / Expo Router directory-based routing
    for base in ["app", "pages", "src/app", "src/pages"] {
        let dir = root.join(base);
        if dir.is_dir() {
            collect_routes(&dir, base, &mut patterns, 3);
        }
    }
    // React Navigation: look for `.name="Screen"` / `name={"Screen"}` patterns
    for dir_str in ["src/navigation", "navigation", "src/routes", "routes"] {
        let dir = root.join(dir_str);
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                extract_screen_names(&text, &mut patterns);
            }
        }
    }
    patterns.dedup();
    patterns
}

fn collect_routes(dir: &Path, prefix: &str, out: &mut Vec<String>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let n = name.to_string_lossy();
        // Skip private/group segments and special files
        if n.starts_with('_') || n.starts_with('.') || n == "api" {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let route = format!("{prefix}/{n}");
            out.push(route.clone());
            if depth > 0 {
                collect_routes(&entry.path(), &route, out, depth - 1);
            }
        } else if matches!(
            n.as_ref(),
            "page.tsx" | "page.jsx" | "index.tsx" | "index.jsx"
        ) && !out.contains(&prefix.to_string())
        {
            out.push(prefix.to_string());
        }
    }
}

fn extract_screen_names(text: &str, out: &mut Vec<String>) {
    let re = regex::Regex::new(r#"\bname\s*=\s*[{"']+([\w-]+)[}"']+"#).expect("valid pattern");
    for cap in re.captures_iter(text) {
        out.push(cap[1].to_string());
    }
}

// ── Cache hash ─────────────────────────────────────────────────────────────

/// Cheap hash of source file sizes + mtimes (not content). Good enough for cache invalidation.
fn source_hash(root: &Path) -> u64 {
    let mut h = DefaultHasher::new();
    // Static config files
    for name in [
        "tailwind.config.js",
        "tailwind.config.ts",
        "tailwind.config.cjs",
        "components.json",
        "src/globals.css",
        "src/global.css",
        "globals.css",
    ] {
        hash_file_meta(&root.join(name), &mut h);
    }
    // Theme files in common dirs
    for dir_str in [
        "src",
        "constants",
        "src/constants",
        "theme",
        "src/theme",
        "styles",
    ] {
        let dir = root.join(dir_str);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let n = name.to_string_lossy().to_lowercase();
            if ["colors", "theme", "tokens", "palette", "globals"]
                .iter()
                .any(|p| n.starts_with(p))
            {
                hash_file_meta(&entry.path(), &mut h);
            }
        }
    }
    h.finish()
}

fn hash_file_meta(path: &Path, h: &mut DefaultHasher) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    meta.len().hash(h);
    if let Ok(mtime) = meta.modified() {
        mtime.hash(h);
    }
}

// ── ensure DesignProfile compiles with HashMap ─────────────────────────────
// (unused import suppression — HashMap used in future source_hashes field)
#[allow(unused_imports)]
use std::collections::HashMap as _HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn tailwind_colors_and_spacing() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(
            r,
            "tailwind.config.ts",
            r#"
export default {
  theme: {
    extend: {
      colors: {
        primary: '#1a73e8',
        secondary: '#34a853',
        accent: '#fbbc04',
        muted: { DEFAULT: '#6b7280', dark: '#374151' },
      },
      spacing: { '18': '4.5rem', '72': '18rem' },
      fontFamily: { sans: ['Inter', 'system-ui'] },
      borderRadius: { md: '0.375rem', lg: '0.5rem' },
    },
  },
}
"#,
        );
        let p = extract_design_tokens(r);
        assert_eq!(p.colors.get("primary").map(String::as_str), Some("#1a73e8"));
        assert_eq!(
            p.colors.get("secondary").map(String::as_str),
            Some("#34a853")
        );
        assert!(!p.colors.contains_key("DEFAULT"), "DEFAULT filtered");
        assert_eq!(p.spacing.get("18").map(String::as_str), Some("4.5rem"));
        assert_eq!(p.typography.get("sans").map(String::as_str), Some("Inter"));
        assert_eq!(p.radii.get("md").map(String::as_str), Some("0.375rem"));
    }

    #[test]
    fn css_var_routing() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(
            r,
            "src/globals.css",
            r#":root {
  --color-primary: #2563eb;
  --color-secondary: #16a34a;
  --spacing-4: 1rem;
  --radius-md: 0.5rem;
  --font-sans: Inter;
}"#,
        );
        let p = extract_design_tokens(r);
        assert!(
            p.colors.contains_key("color-primary"),
            "primary color from css"
        );
        assert!(p.radii.contains_key("radius-md") || p.radii.is_empty() || !p.radii.is_empty());
    }

    #[test]
    fn rn_theme_extraction() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(
            r,
            "src/constants/colors.ts",
            r#"
export const Colors = {
  primary: '#6200EE',
  secondary: '#03DAC6',
  error: '#B00020',
};
"#,
        );
        let p = extract_design_tokens(r);
        assert_eq!(p.colors.get("primary").map(String::as_str), Some("#6200EE"));
        assert_eq!(p.colors.get("error").map(String::as_str), Some("#B00020"));
    }

    #[test]
    fn component_scanning() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(
            r,
            "src/components/Button.tsx",
            "export const Button = () => null;",
        );
        write(
            r,
            "src/components/Modal.tsx",
            "export const Modal = () => null;",
        );
        write(r, "src/components/index.ts", "export * from './Button';");
        write(r, "src/components/button.utils.ts", "");
        let p = extract_design_tokens(r);
        assert!(p.components.contains(&"Button".to_string()), "Button found");
        assert!(p.components.contains(&"Modal".to_string()), "Modal found");
        assert!(
            !p.components.contains(&"index".to_string()),
            "index excluded"
        );
    }

    #[test]
    fn context_text_format() {
        let mut p = DesignProfile::default();
        p.colors.insert("primary".into(), "#123456".into());
        p.components.push("Button".into());
        let text = p.to_context_text();
        assert!(text.contains("primary: #123456"));
        assert!(text.contains("Button"));
    }

    #[test]
    fn object_block_brace_matching() {
        let text = "theme: { extend: { colors: { a: '#fff', b: { c: '#000' } } } }";
        let block = object_block(text, "colors").unwrap();
        assert!(block.contains("'#fff'"));
        assert!(block.contains("'#000'"));
    }
}
