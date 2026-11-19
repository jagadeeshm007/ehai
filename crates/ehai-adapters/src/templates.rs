//! Tiny `{{key}}` substitution. Templates are embedded at compile time, so a
//! shipped binary always carries matching templates. No template engine: the
//! templates are prose with a handful of placeholders.

use crate::Ctx;

const ASK_USER: &str = include_str!("../../../templates/shared/ask-user.md");
const ROLE: &str = include_str!("../../../templates/shared/role.md");
const INSIGHTS: &str = include_str!("../../../templates/shared/insights.md");

/// Render `tpl` for `ctx`. Shared blocks (`{{ask_block}}`, `{{role_block}}`) are
/// expanded first so they can use the same placeholders. Every command
/// template carries `{{role_block}}`, so the Insights footer rides along with it.
pub fn render(tpl: &str, ctx: &Ctx) -> String {
    let brand = ctx.brand.name.as_str();
    let prefix = ctx.brand.command_prefix();
    let role = ctx
        .role
        .as_deref()
        .unwrap_or("not set yet — ask the user (pm / dev / both)");
    tpl.replace("{{ask_block}}", ASK_USER.trim_end())
        .replace("{{insights_block}}", INSIGHTS.trim_end())
        .replace(
            "{{role_block}}",
            &format!("{}\n\n{}", ROLE.trim_end(), INSIGHTS.trim_end()),
        )
        .replace("{{brand}}", brand)
        .replace("{{prefix}}", &prefix)
        .replace("{{role}}", role)
        .replace("{{version}}", ehai_core::VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ehai_core::Brand;

    #[test]
    fn substitutes_everything() {
        let ctx = Ctx {
            brand: Brand::new("acme").unwrap(),
            role: None,
            home: "/h".into(),
            exe: "/h/bin/ehai".into(),
        };
        let out = render(
            "{{brand}} {{prefix}}x {{role}}\n{{ask_block}}\n{{role_block}}",
            &ctx,
        );
        assert!(out.starts_with("acme acme:x not set yet"));
        assert!(out.contains("AskUserQuestion"));
        assert!(
            out.contains("★ Insights"),
            "insights footer rides with role_block"
        );
        assert!(!out.contains("{{"), "no placeholder left:\n{out}");
    }
}
