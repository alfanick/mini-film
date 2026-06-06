//! Embedded sampler assets.
//!
//! These are included at compile time so the CLI can produce sampler HTML output
//! from a single binary without any external template/style/script files.

use include_dir::{Dir, include_dir};

static SAMPLER_ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/sampler");

pub(crate) fn html_page_template() -> &'static str {
    read_text_asset("page.html.hbs")
}

pub(crate) fn html_section_template() -> &'static str {
    read_text_asset("section.html.hbs")
}

pub(crate) fn html_grid_template() -> &'static str {
    read_text_asset("grid.html.hbs")
}

pub(crate) fn html_tile_template() -> &'static str {
    read_text_asset("tile.html.hbs")
}

pub(crate) fn html_children_template() -> &'static str {
    read_text_asset("children.html.hbs")
}

pub(crate) fn html_styles() -> &'static str {
    read_text_asset("styles.css")
}

pub(crate) fn html_script() -> &'static str {
    read_text_asset("app.js")
}

fn read_text_asset(path: &str) -> &'static str {
    let file = SAMPLER_ASSETS
        .get_file(path)
        .unwrap_or_else(|| panic!("embedded sampler asset missing: {path}"));
    file.contents_utf8()
        .unwrap_or_else(|| panic!("embedded sampler asset is not valid UTF-8: {path}"))
}
