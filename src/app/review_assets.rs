//! Embedded daemon review UI assets.
//!
//! The daemon review server is intended to work from a single static binary, so
//! the page, stylesheet, and browser controller are compiled into the executable.

use include_dir::{Dir, include_dir};

static REVIEW_ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/review");

pub(crate) fn review_index_html() -> &'static str {
    read_text_asset("index.html")
}

pub(crate) fn review_styles() -> &'static str {
    read_text_asset("styles.css")
}

pub(crate) fn review_script() -> &'static str {
    read_text_asset("app.js")
}

pub(crate) fn review_text_asset(path: &str) -> Option<&'static str> {
    REVIEW_ASSETS
        .get_file(path)
        .and_then(|file| file.contents_utf8())
}

fn read_text_asset(path: &str) -> &'static str {
    let file = REVIEW_ASSETS
        .get_file(path)
        .unwrap_or_else(|| panic!("embedded review asset missing: {path}"));
    file.contents_utf8()
        .unwrap_or_else(|| panic!("embedded review asset is not valid UTF-8: {path}"))
}
