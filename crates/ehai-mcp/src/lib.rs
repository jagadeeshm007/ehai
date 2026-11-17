//! ehai's MCP server. Harnesses spawn `ehai mcp` per session over stdio, so there is
//! no long-lived daemon. Every handler runs behind [`guard`], which turns a panic
//! into a structured error instead of taking the server down.
//!
//! Compatibility rule (spec §11): within a major version tools and resources are
//! only ever *added*; existing names, inputs and outputs never change shape.

use std::future::Future;
use std::path::PathBuf;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    ErrorData, Implementation, InitializeResult, ListResourcesResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{tool, tool_handler, tool_router, Json, RoleServer, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Serialize;

pub const PREFS_URI: &str = "ehai://prefs";

/// What the server knows about its environment, resolved once at startup.
#[derive(Debug, Clone)]
pub struct Context {
    pub brand: String,
    pub role: Option<String>,
    pub home: PathBuf,
}

#[derive(Debug, Clone)]
pub struct EhaiServer {
    ctx: Context,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Pong {
    pub ok: bool,
    pub name: String,
    pub version: String,
}

#[tool_router(router = tool_router)]
impl EhaiServer {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "ping",
        description = "Check that the ehai engine is reachable. Returns its name and version."
    )]
    pub async fn ping(&self) -> Result<Json<Pong>, String> {
        let name = self.ctx.brand.clone();
        guard(async move {
            Ok(Json(Pong {
                ok: true,
                name,
                version: ehai_core::VERSION.into(),
            }))
        })
        .await
    }

    /// Body of the `ehai://prefs` resource: role plus the user's preference file.
    /// Preference learning lands in M6; until then this is the explicit profile only.
    pub fn prefs_text(&self) -> String {
        let mut out = String::from("# ehai user preferences (edit with /ehai:prefs)\n");
        if let Some(role) = &self.ctx.role {
            out.push_str(&format!("role = \"{role}\"\n"));
        }
        let file = ehai_core::paths::prefs_dir(&self.ctx.home).join("user.toml");
        match std::fs::read_to_string(&file) {
            Ok(body) => out.push_str(&body),
            Err(_) => out.push_str("# nothing learned yet\n"),
        }
        out
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EhaiServer {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            self.ctx.brand.clone(),
            ehai_core::VERSION,
        ))
        .with_instructions(format!(
            "{} plans product work, syncs it to the team tracker and builds isolated UI \
             proof-of-concepts. Read {PREFS_URI} for the user's role and preferences before \
             producing output for them. When you need input from the user, ask with your \
             harness's multiple-choice question tool.",
            self.ctx.brand
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
mod tests {
    use super::*;

    fn server(home: PathBuf) -> EhaiServer {
        EhaiServer::new(Context {
            brand: "acme".into(),
            role: Some("pm".into()),
            home,
        })
    }

    #[tokio::test]
    async fn ping_reports_brand_and_version() {
        let d = tempfile::tempdir().unwrap();
        let Json(p) = server(d.path().into()).ping().await.unwrap();
        assert!(p.ok);
        assert_eq!(p.name, "acme");
        assert_eq!(p.version, ehai_core::VERSION);
    }

    #[test]
    fn prefs_text_includes_role_and_file() {
        let d = tempfile::tempdir().unwrap();
        let s = server(d.path().into());
        assert!(s.prefs_text().contains("role = \"pm\""));
        assert!(s.prefs_text().contains("nothing learned yet"));
        std::fs::create_dir_all(d.path().join("prefs")).unwrap();
        std::fs::write(d.path().join("prefs/user.toml"), "detail = \"short\"\n").unwrap();
        assert!(s.prefs_text().contains("detail = \"short\""));
    }

    #[tokio::test]
    async fn guard_turns_panics_into_errors() {
        let r: Result<(), String> = guard(async { panic!("boom") }).await;
        assert!(r.unwrap_err().contains("panicked"));
        assert_eq!(guard(async { Ok::<_, String>(7) }).await, Ok(7));
    }

    #[test]
    fn tools_have_schemas() {
        let d = tempfile::tempdir().unwrap();
        let tools = server(d.path().into()).tool_router.list_all();
        let ping = tools.iter().find(|t| t.name == "ping").unwrap();
        assert!(ping.output_schema.is_some());
    }
}
