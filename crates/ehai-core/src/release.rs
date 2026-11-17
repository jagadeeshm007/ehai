/// Minisign public key used to verify ehai release binaries.
/// Generated 2026-11-17 with `minisign -G -W -p ~/.ehai/minisign.pub -s ~/.ehai/minisign.key`.
/// The corresponding private key is stored in GitHub Actions secret `MINISIGN_KEY`
/// in the `jagadeeshm007/ehai` repository.
pub const MINISIGN_PUBLIC_KEY: &str = "RWTKHW5dGCGJOywz9PXOM27FdVwe+Eqj9qrgNrFh0faYS9LKS0kL6uhC";

/// `https://github.com/<owner>/<repo>`, from the workspace manifest.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// The GitHub REST API root for this repo's releases, e.g.
/// `https://api.github.com/repos/jagadeeshm007/ehai`. `EHAI_RELEASES_API`
/// overrides it (tests point it at a fake or a dead address).
pub fn releases_api() -> String {
    if let Ok(v) = std::env::var("EHAI_RELEASES_API") {
        return v;
    }
    let slug = REPOSITORY
        .trim_end_matches('/')
        .rsplit_once("github.com/")
        .map_or(REPOSITORY, |(_, s)| s);
    format!("https://api.github.com/repos/{slug}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_releases_api_root_from_the_repository_url() {
        assert_eq!(
            releases_api(),
            "https://api.github.com/repos/jagadeeshm007/ehai"
        );
    }
}
