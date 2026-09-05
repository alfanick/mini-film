//! Export the application's shared wire schemas without building the app.
//! This development-only workspace member is never a dependency of mini-film.

#[path = "../../../src/review_contract/mod.rs"]
pub mod review_contract;
#[path = "../../../build-support/review_schema.rs"]
pub mod review_schema;

use std::{env, error::Error, path::PathBuf};

/// Write deterministic schemas to the explicitly selected output directory.
fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--out-dir")) {
        return Err("usage: review-contract-export --out-dir <directory>".into());
    }
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("missing output directory")?;
    if arguments.next().is_some() {
        return Err("unexpected schema exporter argument".into());
    }
    review_schema::export(&output)?;
    Ok(())
}
