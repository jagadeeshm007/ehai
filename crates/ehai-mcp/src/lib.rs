//! ehai's MCP server. Harnesses spawn `ehai mcp` per session over stdio, so there is
//! no long-lived daemon. Every handler runs behind [`guard`], which turns a panic
//! into a structured error instead of taking the server down.
//!
//! - Tools (`tools.rs`): ping, PRD / breakdown / sync / status / triage, guard check.
//! - Prompts: the same flows as the Claude Code skills, rendered from the shared
//!   templates. This is how Claude desktop gets the PM flows (it can't install
//!   skills from disk).
//! - Resource: `ehai://prefs`.
//!
//! Compatibility rule (spec §11): within a major version tools, prompts and
//! resources are only ever *added*; existing names, inputs and outputs keep shape.

use std::future::Future;
use std::path::PathBuf;

use ehai_core::Config;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    ErrorData, GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation,
    InitializeResult, ListPromptsResult, ListResourcesResult, PaginatedRequestParams, Prompt,
    PromptArgument, PromptMessage, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, Role, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{tool_handler, RoleServer, ServerHandler, ServiceExt};

mod tools;

pub use tools::PhiChoice;

pub const PREFS_URI: &str = "ehai://prefs";

/// How tools reach GitHub.
#[derive(Debug, Clone)]
pub enum Github {
    /// `gh auth token`, then `GITHUB_TOKEN`/`GH_TOKEN`; `EHAI_GITHUB_API` honoured.
    FromEnv,
    /// Fixed endpoint and token (tests).
    Fixed { api: String, token: String },
}

/// What the server knows about its environment, resolved once at startup.
#[derive(Debug, Clone)]
pub struct Context {
    pub config: Config,
    pub home: PathBuf,
    /// Where the harness started us; used to find the repo for PRD files.
    pub cwd: Option<PathBuf>,
    pub github: Github,
}

impl Context {
    pub fn brand(&self) -> &str {
        &self.config.brand.name
    }

    /// The role that actually applies — `dev` unless the viewer administers
    /// the connected project (`Config::effective_role`).
    pub fn role(&self) -> &'static str {
        use ehai_core::config::Role;
        match self.config.effective_role() {
            Role::Pm => "pm",
            Role::Dev => "dev",
            Role::Both => "both",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EhaiServer {
    ctx: Context,
    tool_router: ToolRouter<Self>,
}

impl EhaiServer {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx,
            tool_router: Self::tool_router(),
        }
    }

    /// Body of the `ehai://prefs` resource: role (and why it applies) plus the
    /// user's preference file. Preference learning lands in M6; until then
    /// this is the explicit profile only.
    pub fn prefs_text(&self) -> String {
        let mut out = String::from("# ehai user preferences (edit with /ehai:prefs)\n");
        out.push_str(&format!("role = \"{}\"\n", self.ctx.role()));
        out.push_str(&format!(
            "project_admin = {}\n",
            self.ctx
                .config
                .access
                .project_admin
                .map_or("unknown".to_string(), |b| b.to_string())
        ));
        out.push_str(
            "role_source = \"only an admin of the connected project chooses a role; \
             everyone else is dev\"\n",
        );
        let file = ehai_core::paths::prefs_dir(&self.ctx.home).join("user.toml");
        match std::fs::read_to_string(&file) {
            Ok(body) => out.push_str(&body),
            Err(_) => out.push_str("# nothing learned yet\n"),
        }
        out
    }

    fn adapter_ctx(&self) -> ehai_adapters::Ctx {
        ehai_adapters::Ctx {
            brand: self.ctx.config.brand.clone(),
            role: Some(self.ctx.role().to_string()),
            home: self.ctx.home.clone(),
            exe: std::env::current_exe().unwrap_or_default(),
        }
    }

    /// The flows, as MCP prompts: (name, description, takes an argument).
    fn prompts() -> Vec<(&'static str, String, bool)> {
        ehai_adapters::claude_code::SKILLS
            .iter()
            .map(|(name, tpl)| {
                let fm = frontmatter(tpl);
                let desc = fm.get("description").cloned().unwrap_or_default();
                (*name, desc, fm.contains_key("argument-hint"))
            })
            .collect()
    }

    /// A rendered prompt: the skill body with `$ARGUMENTS` filled in.
    pub fn prompt_text(&self, name: &str, input: Option<&str>) -> Option<String> {
        let (_, tpl) = ehai_adapters::claude_code::SKILLS
            .iter()
            .find(|(n, _)| *n == name)?;
        let rendered = ehai_adapters::render(tpl, &self.adapter_ctx());
        let body = strip_frontmatter(&rendered);
        Some(
            body.replace(
                "$ARGUMENTS",
                input
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("(none given — ask the user)"),
            ),
        )
    }
}

fn frontmatter(tpl: &str) -> std::collections::BTreeMap<String, String> {
    tpl.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
        .map(|(fm, _)| {
            fm.lines()
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn strip_frontmatter(s: &str) -> &str {
    s.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(s, |(_, body)| body)
        .trim_start()
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EhaiServer {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new(self.ctx.brand().to_string(), ehai_core::VERSION))
        .with_instructions(format!(
            "{brand} plans product work (PRDs, breakdowns), syncs it to the team tracker, reports \
             status and triages bugs. Read {PREFS_URI} for the user's role before producing \
             output. Ask the user through your multiple-choice question tool. When a tool returns \
             \"status\": \"needs_input\", ask its question, then call the same tool again with \
             `retry_param` set to the chosen value. Never put real patient data in anything you write.",
            brand = self.ctx.brand()
        ))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(vec![Resource::new(
            PREFS_URI, "prefs",
        )
        .with_title("User preferences")
        .with_description("The user's role and learned preferences (TOML).")
        .with_mime_type("application/toml")]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri != PREFS_URI {
            return Err(ErrorData::resource_not_found(
                format!("unknown resource {}", request.uri),
                None,
            ));
        }
        let this = self.clone();
        let text = guard(async move { Ok(this.prefs_text()) })
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, PREFS_URI)]).into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let prompts = Self::prompts()
            .into_iter()
            .map(|(name, desc, takes_input)| {
                let args = takes_input.then(|| {
                    vec![PromptArgument::new("input")
                        .with_description("What to work on (an idea, a PRD name, a bug report)")]
                });
                Prompt::new(name, Some(desc), args)
            })
            .collect();
        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let input = request
            .arguments
            .as_ref()
            .and_then(|a| a.get("input"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let text = self
            .prompt_text(&request.name, input.as_deref())
            .ok_or_else(|| {
                ErrorData::invalid_params(format!("unknown prompt {}", request.name), None)
            })?;
        Ok(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
                .with_description(format!("{} {}", self.ctx.brand(), request.name))
                .into(),
        )
    }
}

/// Run `fut` on its own task so a panic inside a handler becomes an `Err` for
/// that one request; the server keeps serving.
pub async fn guard<T, F>(fut: F) -> Result<T, String>
where
    F: Future<Output = Result<T, String>> + Send + 'static,
    T: Send + 'static,
{
    match tokio::spawn(fut).await {
        Ok(result) => result,
        Err(e) if e.is_panic() => {
            tracing::error!("tool handler panicked");
            Err("internal error: the handler panicked; see ~/.ehai/logs".into())
        }
        Err(e) => Err(format!("internal error: {e}")),
    }
}

/// Serve over stdin/stdout until the client disconnects.
pub async fn serve_stdio(ctx: Context) -> std::io::Result<()> {
    let service = EhaiServer::new(ctx)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(std::io::Error::other)?;
    service.waiting().await.map_err(std::io::Error::other)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    pub(crate) fn ctx(home: PathBuf) -> Context {
        let mut config = Config::default();
        config.brand = ehai_core::Brand::new("acme").unwrap();
        config.role = Some(ehai_core::config::Role::Pm);
        config.access.project_admin = Some(true); // so effective_role() is "pm", not "dev"
        Context {
            config,
            home,
            cwd: None,
            github: Github::Fixed {
                api: "http://127.0.0.1:9".into(),
                token: "t".into(),
            },
        }
    }

    #[test]
    fn prefs_text_includes_role_and_file() {
        let d = tempfile::tempdir().unwrap();
        let s = EhaiServer::new(ctx(d.path().into()));
        assert!(s.prefs_text().contains("role = \"pm\""));
        assert!(s.prefs_text().contains("project_admin = true"));
        assert!(s.prefs_text().contains("nothing learned yet"));
        std::fs::create_dir_all(d.path().join("prefs")).unwrap();
        std::fs::write(d.path().join("prefs/user.toml"), "detail = \"short\"\n").unwrap();
        assert!(s.prefs_text().contains("detail = \"short\""));
    }

    #[test]
    fn non_admins_are_always_dev_regardless_of_stored_role() {
        let d = tempfile::tempdir().unwrap();
        let mut c = ctx(d.path().into());
        c.config.access.project_admin = Some(false);
        assert_eq!(c.role(), "dev");
        c.config.access.project_admin = None; // no project connected yet
        assert_eq!(c.role(), "dev");
    }

    #[test]
    fn prompts_mirror_the_skills() {
        let d = tempfile::tempdir().unwrap();
        let s = EhaiServer::new(ctx(d.path().into()));
        let names: Vec<_> = EhaiServer::prompts().into_iter().map(|p| p.0).collect();
        for n in [
            "plan",
            "breakdown",
            "sync",
            "status",
            "triage",
            "help",
            "prefs",
        ] {
            assert!(names.contains(&n), "{n}");
        }
        let text = s.prompt_text("plan", Some("online booking")).unwrap();
        assert!(
            text.starts_with("# Plan a feature with acme"),
            "frontmatter stripped: {text}"
        );
        assert!(text.contains("Idea from the user: online booking"));
        assert!(text.contains("Configured role: **pm**"));
        assert!(!text.contains("{{") && !text.contains("$ARGUMENTS"));
        assert!(s
            .prompt_text("status", None)
            .unwrap()
            .contains("status_report"));
        assert!(s.prompt_text("nope", None).is_none());
    }

    #[tokio::test]
    async fn guard_turns_panics_into_errors() {
        let r: Result<(), String> = guard(async { panic!("boom") }).await;
        assert!(r.unwrap_err().contains("panicked"));
        assert_eq!(guard(async { Ok::<_, String>(7) }).await, Ok(7));
    }
}
