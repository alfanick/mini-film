//! Embedded batch gallery assets.
//!
//! Keeping gallery HTML/CSS/JS inside embedded assets lets the binary remain
//! self-contained while still allowing a clean runtime path for templates.

use crate::cli::GalleryTemplate;
use include_dir::{Dir, include_dir};

static BATCH_ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/batch");

pub(crate) fn gallery_html_page_template(template: GalleryTemplate) -> &'static str {
    read_text_asset(&template_page_path(template))
}

pub(crate) fn gallery_html_styles(template: GalleryTemplate) -> &'static str {
    read_text_asset(&template_styles_path(template))
}

pub(crate) fn gallery_html_script(template: GalleryTemplate) -> &'static str {
    read_text_asset(&template_script_path(template))
}

pub(crate) fn gallery_html_item_template() -> &'static str {
    read_text_asset("item.html.hbs")
}

fn read_text_asset(path: &str) -> &'static str {
    let file = BATCH_ASSETS
        .get_file(path)
        .unwrap_or_else(|| panic!("embedded batch asset missing: {path}"));
    file.contents_utf8()
        .unwrap_or_else(|| panic!("embedded batch asset is not valid UTF-8: {path}"))
}

fn template_asset_path(template: GalleryTemplate, file_name: &str) -> String {
    format!("templates/{}/{}", template, file_name)
}

fn template_page_path(template: GalleryTemplate) -> String {
    template_asset_path(template, "page.html.hbs")
}

fn template_styles_path(template: GalleryTemplate) -> String {
    template_asset_path(template, "styles.css")
}

fn template_script_path(template: GalleryTemplate) -> String {
    template_asset_path(template, "app.js")
}
