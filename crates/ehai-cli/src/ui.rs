// Question methods are consumed by the M1 `ehai init` wizard; exercised by tests now.
#![cfg_attr(not(test), allow(dead_code))]

//! UI kit. Wizards talk to a [`Prompter`], never to the terminal directly, so their
//! logic is testable with scripted answers and degrades to plain text in CI.
//!
//! - [`ClackPrompter`]: interactive TTY (cliclack; `console` honours `NO_COLOR`).
//! - [`PlainPrompter`]: non-TTY / `--yes`. Accepts defaults, prints plain lines.
//!   With [`PlainPrompter::scripted`] it is also the test fake, so tests snapshot
//!   exactly what a CI user would see.

use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};

#[derive(Debug, Clone)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub hint: String,
    /// Default for `select` (first one wins) / pre-checked for `multiselect`.
    pub default: bool,
}

impl Choice {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            hint: String::new(),
            default: false,
        }
    }

    pub fn hint(mut self, hint: &str) -> Self {
        self.hint = hint.into();
        self
    }

    pub fn default(mut self) -> Self {
        self.default = true;
        self
    }
}

/// Outcome of one checklist step.
pub type StepResult = Result<String, StepError>;

#[derive(Debug, Clone)]
pub struct StepError {
    pub message: String,
    /// What the user can do about it.
    pub fix: Option<String>,
}

pub trait Prompter {
    fn intro(&mut self, title: &str) -> io::Result<()>;
    fn outro(&mut self, message: &str) -> io::Result<()>;
    fn note(&mut self, title: &str, body: &str) -> io::Result<()>;
    fn warn(&mut self, message: &str) -> io::Result<()>;
    fn select(&mut self, prompt: &str, choices: &[Choice]) -> io::Result<String>;
    fn multiselect(&mut self, prompt: &str, choices: &[Choice]) -> io::Result<Vec<String>>;
    fn confirm(&mut self, prompt: &str, default: bool) -> io::Result<bool>;
    fn input(&mut self, prompt: &str, default: &str) -> io::Result<String>;
    /// Run `work` with a spinner, then render ✔ / ✖ (+ fix hint). Returns success.
    fn step(&mut self, label: &str, work: &mut dyn FnMut() -> StepResult) -> io::Result<bool>;
}

/// The "Insights" footer that ends command output: next steps, warnings,
/// notes. Same shape as the footer the `/ehai:` skills end with.
pub fn insights(lines: &[String]) -> String {
    let body: String = lines.iter().map(|l| format!("  {l}\n")).collect();
    format!("\n★ Insights {}\n{body}{}", "─".repeat(37), "─".repeat(48))
}

/// Interactive when stdin and stdout are terminals and `--yes` wasn't given.
pub fn prompter(assume_yes: bool) -> Box<dyn Prompter> {
    if !assume_yes && io::stdin().is_terminal() && io::stdout().is_terminal() {
        Box::new(ClackPrompter)
    } else {
        Box::new(PlainPrompter::new(io::stdout()))
    }
}

fn default_select(choices: &[Choice]) -> String {
    choices
        .iter()
        .find(|c| c.default)
        .or(choices.first())
        .map(|c| c.value.clone())
        .unwrap_or_default()
}

fn default_multi(choices: &[Choice]) -> Vec<String> {
    choices
        .iter()
        .filter(|c| c.default)
        .map(|c| c.value.clone())
        .collect()
}

pub struct ClackPrompter;

/// Key help after each prompt title, so nobody is left guessing how to pick
/// (users didn't know Space toggles a multiselect).
fn keys(prompt: &str, help: &str) -> String {
    format!("{prompt}  {}", console::style(format!("({help})")).dim())
}

impl Prompter for ClackPrompter {
    fn intro(&mut self, title: &str) -> io::Result<()> {
        cliclack::intro(console::style(format!(" {title} ")).on_cyan().black())
    }

    fn outro(&mut self, message: &str) -> io::Result<()> {
        cliclack::outro(message)
    }

    fn note(&mut self, title: &str, body: &str) -> io::Result<()> {
        cliclack::note(title, body)
    }

    fn warn(&mut self, message: &str) -> io::Result<()> {
        cliclack::log::warning(message)
    }

    fn select(&mut self, prompt: &str, choices: &[Choice]) -> io::Result<String> {
        let mut s = cliclack::select(keys(prompt, "↑/↓ move · Enter choose · Esc back"))
            .initial_value(default_select(choices));
        for c in choices {
            s = s.item(c.value.clone(), &c.label, &c.hint);
        }
        s.interact()
    }

    fn multiselect(&mut self, prompt: &str, choices: &[Choice]) -> io::Result<Vec<String>> {
        let mut s = cliclack::multiselect(keys(
            prompt,
            "↑/↓ move · Space select/unselect · Enter confirm · Esc back",
        ))
        .initial_values(default_multi(choices))
        .required(false);
        for c in choices {
            s = s.item(c.value.clone(), &c.label, &c.hint);
        }
        s.interact()
    }

    fn confirm(&mut self, prompt: &str, default: bool) -> io::Result<bool> {
        cliclack::confirm(keys(prompt, "←/→ or y/n · Enter confirm"))
            .initial_value(default)
            .interact()
    }

    fn input(&mut self, prompt: &str, default: &str) -> io::Result<String> {
        cliclack::input(keys(prompt, "type to change · Enter accept"))
            .default_input(default)
            .required(false)
            .interact()
    }

    fn step(&mut self, label: &str, work: &mut dyn FnMut() -> StepResult) -> io::Result<bool> {
        let spinner = cliclack::spinner();
        spinner.start(label);
        match work() {
            Ok(detail) => {
                spinner.stop(join(label, &detail));
                Ok(true)
            }
            Err(e) => {
                spinner.error(join(label, &e.message));
                if let Some(fix) = e.fix {
                    cliclack::log::remark(format!("fix: {fix}"))?;
                }
                Ok(false)
            }
        }
    }
}

fn join(label: &str, detail: &str) -> String {
    if detail.is_empty() {
        label.to_string()
    } else {
        format!("{label} — {detail}")
    }
}

pub struct PlainPrompter<W: Write> {
    out: W,
    /// Scripted answers (tests). Empty → every question takes its default.
    answers: VecDeque<String>,
}

impl<W: Write> PlainPrompter<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            answers: VecDeque::new(),
        }
    }

    /// Answers are consumed in order: `select` → value, `multiselect` → comma-separated
    /// values, `confirm` → `y`/`n`, `input` → text. Missing answers fall back to defaults.
    #[cfg(test)]
    pub fn scripted(out: W, answers: &[&str]) -> Self {
        Self {
            out,
            answers: answers.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[cfg(test)]
    pub fn into_inner(self) -> W {
        self.out
    }

    fn answer(&mut self, prompt: &str, shown: &str) -> io::Result<Option<String>> {
        let a = self.answers.pop_front();
        writeln!(self.out, "? {prompt} → {}", a.as_deref().unwrap_or(shown))?;
        Ok(a)
    }
}

impl<W: Write> Prompter for PlainPrompter<W> {
    fn intro(&mut self, title: &str) -> io::Result<()> {
        writeln!(self.out, "== {title} ==")
    }

    fn outro(&mut self, message: &str) -> io::Result<()> {
        writeln!(self.out, "-- {message}")
    }

    fn note(&mut self, title: &str, body: &str) -> io::Result<()> {
        writeln!(self.out, "[{title}]")?;
        for line in body.lines() {
            writeln!(self.out, "  {line}")?;
        }
        Ok(())
    }

    fn warn(&mut self, message: &str) -> io::Result<()> {
        writeln!(self.out, "! {message}")
    }

    fn select(&mut self, prompt: &str, choices: &[Choice]) -> io::Result<String> {
        let def = default_select(choices);
        let picked = self.answer(prompt, &def)?.unwrap_or(def);
        if choices.iter().any(|c| c.value == picked) {
            Ok(picked)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{picked:?} is not an option for {prompt:?}"),
            ))
        }
    }

    fn multiselect(&mut self, prompt: &str, choices: &[Choice]) -> io::Result<Vec<String>> {
        let def = default_multi(choices);
        let picked = match self.answer(prompt, &def.join(","))? {
            Some(a) => a
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            None => def,
        };
        if let Some(bad) = picked
            .iter()
            .find(|p| !choices.iter().any(|c| &c.value == *p))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{bad:?} is not an option for {prompt:?}"),
            ));
        }
        Ok(picked)
    }

    fn confirm(&mut self, prompt: &str, default: bool) -> io::Result<bool> {
        let shown = if default { "y" } else { "n" };
        Ok(match self.answer(prompt, shown)? {
            Some(a) => matches!(a.as_str(), "y" | "yes" | "true"),
            None => default,
        })
    }

    fn input(&mut self, prompt: &str, default: &str) -> io::Result<String> {
        Ok(self
            .answer(prompt, default)?
            .unwrap_or_else(|| default.into()))
    }

    fn step(&mut self, label: &str, work: &mut dyn FnMut() -> StepResult) -> io::Result<bool> {
        match work() {
            Ok(detail) => {
                writeln!(self.out, "✔ {}", join(label, &detail))?;
                Ok(true)
            }
            Err(e) => {
                writeln!(self.out, "✖ {}", join(label, &e.message))?;
                if let Some(fix) = e.fix {
                    writeln!(self.out, "  fix: {fix}")?;
                }
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles() -> Vec<Choice> {
        vec![
            Choice::new("pm", "PM").hint("plain language, visuals"),
            Choice::new("dev", "Developer").default(),
            Choice::new("both", "Both"),
        ]
    }

    fn harnesses() -> Vec<Choice> {
        vec![
            Choice::new("claude-code", "Claude Code").default(),
            Choice::new("codex", "Codex CLI").default(),
            Choice::new("cursor", "Cursor"),
        ]
    }

    /// A miniature wizard, as M1's `ehai init` will be built.
    fn demo(p: &mut dyn Prompter) -> io::Result<(String, Vec<String>, bool, String)> {
        p.intro("ehai 0.1.0-alpha.1")?;
        p.note("Detected", "git 2.53\ngh: not found")?;
        p.warn("Claude desktop not found")?;
        let role = p.select("Your role", &roles())?;
        let hs = p.multiselect("Harnesses to wire up", &harnesses())?;
        let strict = p.confirm("Strict compliance?", false)?;
        let name = p.input("Project name", "my-app")?;
        p.step("Write config", &mut || Ok("~/.ehai/config.toml".into()))?;
        p.step("Register MCP", &mut || {
            Err(StepError {
                message: "claude not on PATH".into(),
                fix: Some("install Claude Code, then rerun".into()),
            })
        })?;
        p.outro("You're ready")?;
        Ok((role, hs, strict, name))
    }

    #[test]
    fn defaults_when_unscripted() {
        let mut p = PlainPrompter::new(Vec::new());
        let got = demo(&mut p).unwrap();
        assert_eq!(
            got,
            (
                "dev".into(),
                vec!["claude-code".into(), "codex".into()],
                false,
                "my-app".into()
            )
        );
        insta::assert_snapshot!(String::from_utf8(p.into_inner()).unwrap());
    }

    #[test]
    fn scripted_answers_are_used() {
        let mut p = PlainPrompter::scripted(Vec::new(), &["pm", "cursor", "y", "clinic"]);
        let got = demo(&mut p).unwrap();
        assert_eq!(
            got,
            ("pm".into(), vec!["cursor".into()], true, "clinic".into())
        );
    }

    #[test]
    fn invalid_scripted_answers_error() {
        let mut p = PlainPrompter::scripted(Vec::new(), &["admin"]);
        assert!(p.select("Your role", &roles()).is_err());
        let mut p = PlainPrompter::scripted(Vec::new(), &["codex,vim"]);
        assert!(p.multiselect("Harnesses", &harnesses()).is_err());
    }
}
