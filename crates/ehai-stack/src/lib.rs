//! Repo scan for onboarding: which apps live where, their frameworks, package
//! managers, design-system libraries, env file *names* and team conventions.
//! Pure file inspection plus read-only git; never reads env file contents.
//! M5 extends this with design-token extraction.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ehai_core::git;
use serde::{Deserialize, Serialize};

const MAX_DEPTH: usize = 3;
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".next",
    ".expo",
    ".venv",
    "venv",
    "__pycache__",
    "vendor",
    "Pods",
    ".ehai",
    ".turbo",
    "coverage",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stack {
    Expo,
    ReactNative,
    Next,
    ViteReact,
    Cra,
    React,
    Go,
    Fastapi,
    Django,
    Flask,
    Python,
}

impl Stack {
    /// Platform tag used on tracker items (`rn`, `web`, `go`, `py`).
    pub fn platform(self) -> &'static str {
        match self {
            Stack::Expo | Stack::ReactNative => "rn",
            Stack::Next | Stack::ViteReact | Stack::Cra | Stack::React => "web",
            Stack::Go => "go",
            Stack::Fastapi | Stack::Django | Stack::Flask | Stack::Python => "py",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    /// Path relative to the repo root (`.` for the root).
    pub path: String,
    pub stack: Stack,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conventions {
    /// `conventional` or `freeform`, from recent commit subjects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_style: Option<String>,
    pub test_frameworks: BTreeSet<String>,
    pub linters: BTreeSet<String>,
    pub formatters: BTreeSet<String>,
    pub pr_template: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoScan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    pub monorepo: bool,
    pub apps: Vec<App>,
    pub design_system: BTreeSet<String>,
    /// Relative paths of `.env*` files. Names only, never contents.
    pub env_files: Vec<String>,
    pub conventions: Conventions,
}

impl RepoScan {
    pub fn scan(root: &Path) -> Self {
        let mut scan = RepoScan {
            remote: git::try_run(root, &["remote", "get-url", "origin"]),
            default_branch: default_branch(root),
            monorepo: [
                "pnpm-workspace.yaml",
                "turbo.json",
                "nx.json",
                "lerna.json",
                "go.work",
            ]
            .iter()
            .any(|f| root.join(f).is_file()),
            ..Default::default()
        };
        let mut dirs = Vec::new();
        walk(root, 0, &mut dirs);
        for dir in &dirs {
            scan.inspect_dir(root, dir);
        }
        scan.monorepo |= scan.apps.len() > 1;
        scan.apps.sort_by(|a, b| a.path.cmp(&b.path));
        scan.env_files.sort();
        scan.conventions.commit_style = commit_style(root);
        scan.conventions.pr_template = root.join(".github/pull_request_template.md").is_file()
            || root.join(".github/PULL_REQUEST_TEMPLATE.md").is_file()
            || root.join(".github/PULL_REQUEST_TEMPLATE").is_dir();
        scan
    }

    fn inspect_dir(&mut self, root: &Path, dir: &Path) {
        let rel = rel_path(root, dir);
        let entries: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        let has = |name: &str| entries.iter().any(|e| e == name);
        let c = &mut self.conventions;

        for name in &entries {
            if name == ".env" || name.starts_with(".env.") {
                self.env_files.push(join_rel(&rel, name));
            }
            if name.starts_with(".eslintrc") || name.starts_with("eslint.config.") {
                c.linters.insert("eslint".into());
            }
            if name.starts_with(".prettierrc") || name.starts_with("prettier.config.") {
                c.formatters.insert("prettier".into());
            }
            if name.starts_with("tailwind.config.") {
                self.design_system.insert("tailwind".into());
            }
        }
        if has("biome.json") || has("biome.jsonc") {
            c.linters.insert("biome".into());
            c.formatters.insert("biome".into());
        }
        if has("ruff.toml") || has(".ruff.toml") {
            c.linters.insert("ruff".into());
        }
        if has(".golangci.yml") || has(".golangci.yaml") {
            c.linters.insert("golangci-lint".into());
        }
        if has("components.json") {
            self.design_system.insert("shadcn".into());
        }

        if has("package.json") {
            self.inspect_package_json(root, dir, &rel);
        }
        if has("go.mod") {
            self.conventions.test_frameworks.insert("go test".into());
            self.conventions.formatters.insert("gofmt".into());
            self.apps.push(App {
                path: rel.clone(),
                stack: Stack::Go,
                package_manager: Some("go".into()),
            });
        }
        let py_files = ["pyproject.toml", "requirements.txt", "Pipfile", "setup.py"];
        if py_files.iter().any(|f| has(f)) {
            self.inspect_python(dir, &rel, &py_files);
        }
    }

    fn inspect_package_json(&mut self, root: &Path, dir: &Path, rel: &str) {
        let Some(pkg) = std::fs::read_to_string(dir.join("package.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        else {
            return;
        };
        if pkg.get("workspaces").is_some() {
            self.monorepo = true;
        }
        let deps: BTreeSet<&str> = ["dependencies", "devDependencies", "peerDependencies"]
            .iter()
            .filter_map(|k| pkg.get(k)?.as_object())
            .flat_map(|m| m.keys().map(String::as_str))
            .collect();
        let d = |name: &str| deps.contains(name);

        let c = &mut self.conventions;
        for (dep, test) in [
            ("jest", "jest"),
            ("vitest", "vitest"),
            ("@playwright/test", "playwright"),
            ("cypress", "cypress"),
            ("detox", "detox"),
        ] {
            if d(dep) {
                c.test_frameworks.insert(test.into());
            }
        }
        if d("eslint") {
            c.linters.insert("eslint".into());
        }
        if d("prettier") {
            c.formatters.insert("prettier".into());
        }
        if d("@biomejs/biome") {
            c.linters.insert("biome".into());
            c.formatters.insert("biome".into());
        }
        for (dep, ds) in [
            ("tailwindcss", "tailwind"),
            ("nativewind", "nativewind"),
            ("@mui/material", "mui"),
            ("@chakra-ui/react", "chakra"),
            ("@mantine/core", "mantine"),
            ("styled-components", "styled-components"),
            ("@emotion/react", "emotion"),
            ("react-native-paper", "react-native-paper"),
            ("tamagui", "tamagui"),
            ("@shopify/restyle", "restyle"),
        ] {
            if d(dep) {
                self.design_system.insert(ds.into());
            }
        }

        let stack = if d("expo") {
            Some(Stack::Expo)
        } else if d("react-native") {
            Some(Stack::ReactNative)
        } else if d("next") {
            Some(Stack::Next)
        } else if d("vite") && d("react") {
            Some(Stack::ViteReact)
        } else if d("react-scripts") {
            Some(Stack::Cra)
        } else if d("react") {
            Some(Stack::React)
        } else {
            None // tooling-only package (e.g. a monorepo root)
        };
        if let Some(stack) = stack {
            self.apps.push(App {
                path: rel.to_string(),
                stack,
                package_manager: js_package_manager(root, dir),
            });
        }
    }

    fn inspect_python(&mut self, dir: &Path, rel: &str, files: &[&str]) {
        let text: String = files
            .iter()
            .filter_map(|f| std::fs::read_to_string(dir.join(f)).ok())
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        let mentions = |name: &str| {
            text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
                .any(|w| w == name)
        };
        let stack = if mentions("fastapi") {
            Stack::Fastapi
        } else if mentions("django") {
            Stack::Django
        } else if mentions("flask") {
            Stack::Flask
        } else {
            Stack::Python
        };
        let c = &mut self.conventions;
        if mentions("pytest") {
            c.test_frameworks.insert("pytest".into());
        }
        if mentions("ruff") {
            c.linters.insert("ruff".into());
        }
        if mentions("black") {
            c.formatters.insert("black".into());
        }
        let pm = [
            ("uv.lock", "uv"),
            ("poetry.lock", "poetry"),
            ("Pipfile", "pipenv"),
            ("requirements.txt", "pip"),
        ]
        .iter()
        .find(|(f, _)| dir.join(f).is_file())
        .map(|(_, pm)| pm.to_string());
        self.apps.push(App {
            path: rel.to_string(),
            stack,
            package_manager: pm,
        });
    }

    /// Human-readable summary for the onboarding screen.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        if let Some(r) = &self.remote {
            lines.push(format!("remote:      {r}"));
        }
        if let Some(b) = &self.default_branch {
            lines.push(format!("branch:      {b}"));
        }
        lines.push(format!(
            "layout:      {}",
            if self.monorepo {
                "monorepo"
            } else {
                "single app"
            }
        ));
        if self.apps.is_empty() {
            lines.push("apps:        none detected".into());
        }
        for a in &self.apps {
            let pm = a
                .package_manager
                .as_deref()
                .map(|p| format!(" ({p})"))
                .unwrap_or_default();
            lines.push(format!("app:         {} → {:?}{pm}", a.path, a.stack));
        }
        let list = |s: &BTreeSet<String>| {
            if s.is_empty() {
                "—".to_string()
            } else {
                s.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        };
        lines.push(format!("design:      {}", list(&self.design_system)));
        lines.push(format!("env files:   {}", self.env_files.len()));
        let c = &self.conventions;
        lines.push(format!("tests:       {}", list(&c.test_frameworks)));
        lines.push(format!(
            "lint/format: {} / {}",
            list(&c.linters),
            list(&c.formatters)
        ));
        if let Some(s) = &c.commit_style {
            lines.push(format!("commits:     {s}"));
        }
        lines.join("\n")
    }

    /// Write `[detected]` into `<repo>/.ehai/profile.toml`, keeping every other
    /// section (team decisions) intact. Also drops a `.ehai/.gitignore` so local
    /// state never needs an edit to the repo's own `.gitignore`.
    pub fn write_profile(&self, repo_root: &Path) -> ehai_core::Result<PathBuf> {
        use ehai_core::Error;
        let dir = ehai_core::paths::repo_dir(repo_root);
        std::fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
        let gi = dir.join(".gitignore");
        if !gi.exists() {
            std::fs::write(&gi, "cache/\npocs/\nprefs/\n*.local.toml\n*.lock\n*.tmp\n")
                .map_err(Error::io(&gi))?;
        }
        let path = dir.join("profile.toml");
        let _lock = ehai_core::migrate::lock(&path)?;
        let mut table = if path.is_file() {
            ehai_core::config::read_table(&path)?
        } else {
            toml::Table::new()
        };
        let detected = toml::Value::try_from(self).map_err(|e| Error::Invalid(e.to_string()))?;
        table.insert("detected".into(), detected);
        let body = toml::to_string_pretty(&table).map_err(|e| Error::Invalid(e.to_string()))?;
        ehai_core::migrate::write_atomic(&path, body.as_bytes())?;
        Ok(path)
    }
}

pub fn profile_exists(repo_root: &Path) -> bool {
    ehai_core::paths::repo_dir(repo_root)
        .join("profile.toml")
        .is_file()
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    out.push(dir.to_path_buf());
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) // symlinks not followed
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| !SKIP_DIRS.contains(&n) && !(n.starts_with('.') && n != ".github"))
        })
        .map(|e| e.path())
        .collect();
    subdirs.sort();
    for d in subdirs {
        walk(&d, depth + 1, out);
    }
}

fn rel_path(root: &Path, dir: &Path) -> String {
    match dir.strip_prefix(root) {
        Ok(p) if p.as_os_str().is_empty() => ".".into(),
        Ok(p) => p.to_string_lossy().replace('\\', "/"),
        Err(_) => dir.to_string_lossy().into_owned(),
    }
}

fn join_rel(rel: &str, name: &str) -> String {
    if rel == "." {
        name.into()
    } else {
        format!("{rel}/{name}")
    }
}

/// Lockfile in the app dir, else the nearest ancestor up to the repo root.
fn js_package_manager(root: &Path, dir: &Path) -> Option<String> {
    let locks = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
    ];
    dir.ancestors()
        .take_while(|a| a.starts_with(root))
        .find_map(|a| locks.iter().find(|(f, _)| a.join(f).is_file()))
        .map(|(_, pm)| pm.to_string())
}

fn default_branch(root: &Path) -> Option<String> {
    git::try_run(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map(|r| r.trim_start_matches("origin/").to_string())
    .or_else(|| git::try_run(root, &["symbolic-ref", "--short", "HEAD"]))
}

/// `conventional` if ≥ 60% of the last 50 subjects follow Conventional Commits.
fn commit_style(root: &Path) -> Option<String> {
    let log = git::try_run(root, &["log", "-n", "50", "--format=%s"])?;
    let subjects: Vec<&str> = log.lines().filter(|l| !l.starts_with("Merge ")).collect();
    if subjects.is_empty() {
        return None;
    }
    let conventional = subjects.iter().filter(|s| is_conventional(s)).count();
    Some(
        if conventional * 10 >= subjects.len() * 6 {
            "conventional"
        } else {
            "freeform"
        }
        .into(),
    )
}

fn is_conventional(subject: &str) -> bool {
    let Some((head, rest)) = subject.split_once(": ") else {
        return false;
    };
    let head = head.strip_suffix('!').unwrap_or(head);
    let ty = head
        .split_once('(')
        .map_or(head, |(t, scope)| if scope.ends_with(')') { t } else { "" });
    !rest.is_empty()
        && [
            "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore",
            "revert",
        ]
        .contains(&ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn detects_monorepo_apps_design_and_env_names() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(
            r,
            "package.json",
            r#"{"private":true,"workspaces":["apps/*"],"devDependencies":{"prettier":"3"}}"#,
        );
        write(r, "pnpm-lock.yaml", "");
        write(
            r,
            "apps/mobile/package.json",
            r#"{"dependencies":{"expo":"52","react-native":"0.76","nativewind":"4"},"devDependencies":{"jest":"29"}}"#,
        );
        write(
            r,
            "apps/web/package.json",
            r#"{"dependencies":{"next":"15","react":"19","@mui/material":"6"},"devDependencies":{"vitest":"2"}}"#,
        );
        write(r, "apps/web/tailwind.config.ts", "");
        write(r, "apps/web/.env.local", "SECRET=do-not-read");
        write(r, "services/api/go.mod", "module api");
        write(
            r,
            "services/ml/pyproject.toml",
            "[project]\ndependencies = [\"fastapi>=0.110\", \"pytest\"]\n",
        );
        write(r, "services/ml/uv.lock", "");
        write(r, ".env.example", "SECRET=");
        write(
            r,
            "node_modules/react/package.json",
            r#"{"dependencies":{"react":"19"}}"#,
        );
        write(r, ".github/pull_request_template.md", "");

        let s = RepoScan::scan(r);
        assert!(s.monorepo);
        let apps: Vec<_> = s
            .apps
            .iter()
            .map(|a| (a.path.as_str(), a.stack, a.package_manager.as_deref()))
            .collect();
        assert_eq!(
            apps,
            vec![
                ("apps/mobile", Stack::Expo, Some("pnpm")),
                ("apps/web", Stack::Next, Some("pnpm")),
                ("services/api", Stack::Go, Some("go")),
                ("services/ml", Stack::Fastapi, Some("uv")),
            ]
        );
        for ds in ["nativewind", "mui", "tailwind"] {
            assert!(s.design_system.contains(ds), "{ds}");
        }
        assert_eq!(s.env_files, vec![".env.example", "apps/web/.env.local"]);
        for t in ["jest", "vitest", "go test", "pytest"] {
            assert!(s.conventions.test_frameworks.contains(t), "{t}");
        }
        assert!(s.conventions.formatters.contains("prettier"));
        assert!(s.conventions.pr_template);
        assert!(!s.summary().contains("do-not-read"));
        assert_eq!(Stack::Expo.platform(), "rn");
    }

    #[test]
    fn single_vite_app_and_profile_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(
            r,
            "package.json",
            r#"{"dependencies":{"react":"19"},"devDependencies":{"vite":"6"}}"#,
        );
        write(r, "yarn.lock", "");
        let s = RepoScan::scan(r);
        assert!(!s.monorepo);
        assert_eq!(s.apps[0].stack, Stack::ViteReact);
        assert_eq!(s.apps[0].package_manager.as_deref(), Some("yarn"));

        // team decisions survive a rescan
        write(
            r,
            ".ehai/profile.toml",
            "[team]\nmobile_sheets = \"bottom\"\n",
        );
        let p = s.write_profile(r).unwrap();
        let t = ehai_core::config::read_table(&p).unwrap();
        assert_eq!(t["team"]["mobile_sheets"].as_str(), Some("bottom"));
        assert_eq!(
            t["detected"]["apps"][0]["stack"].as_str(),
            Some("vite-react")
        );
        assert!(profile_exists(r));
        assert!(r.join(".ehai/.gitignore").is_file());
    }

    #[test]
    fn commit_style_detection() {
        assert!(is_conventional("feat(core): add x"));
        assert!(is_conventional("fix!: drop y"));
        assert!(!is_conventional("Fixed stuff"));
        assert!(!is_conventional("feat(core: broken"));
        assert!(!is_conventional("wip: thing"));

        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let g = |args: &[&str]| git::run(r, args).unwrap();
        g(&["init", "-q", "-b", "main"]);
        for m in ["feat: a", "fix: b", "docs: c", "random"] {
            g(&[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                m,
            ]);
        }
        let s = RepoScan::scan(r);
        assert_eq!(s.conventions.commit_style.as_deref(), Some("conventional"));
        assert_eq!(s.default_branch.as_deref(), Some("main"));
    }
}
