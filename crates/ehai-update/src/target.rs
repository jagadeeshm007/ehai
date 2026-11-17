//! The release-asset target triple for the platform ehai is running on, matching
//! `.github/workflows/release.yml`'s build matrix.

/// `None` means this OS/arch combination has no published build.
pub fn current_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_this_test_machine() {
        // whatever this is, it must not be "unknown" on a supported CI/dev box
        assert_ne!(
            current_target(),
            "unknown",
            "{} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }
}
