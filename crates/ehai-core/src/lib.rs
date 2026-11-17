//! ehai core: configuration, paths, brand name, schema migrations.

pub mod brand;
pub mod config;
pub mod error;
pub mod git;
pub mod migrate;
pub mod paths;
pub mod process;

pub use brand::Brand;
pub use config::{Config, LoadOptions};
pub use error::{Error, Result};

/// Crate version, shared by every ehai crate (`version.workspace = true`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
