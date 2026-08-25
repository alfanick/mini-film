//! Embedded daemon review UI assets.
//!
//! The daemon review server is intended to work from a single static binary, so
//! the page, stylesheet, and browser controller are compiled into the executable.

use include_dir::{Dir, include_dir};

static REVIEW_ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/review");

pub(crate) fn review_index_html(font_stylesheet_href: Option<&str>) -> String {
    let html = read_text_asset("index.html")
        .replace(
            "assets/styles.css",
            concat!("assets/styles.css?v=", env!("CARGO_PKG_VERSION")),
        )
        .replace(
            "assets/app.js",
            concat!("assets/app.js?v=", env!("CARGO_PKG_VERSION")),
        );
    inject_font_stylesheet(html, font_stylesheet_href)
}

pub(crate) fn review_styles() -> &'static str {
    read_text_asset("styles.css")
}

pub(crate) fn review_script() -> &'static str {
    read_text_asset("app.js")
}

pub(crate) fn review_tv_html(font_stylesheet_href: Option<&str>) -> String {
    inject_font_stylesheet(read_text_asset("tv.html").to_string(), font_stylesheet_href)
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

fn inject_font_stylesheet(html: String, font_stylesheet_href: Option<&str>) -> String {
    let Some(href) = font_stylesheet_href else {
        return html;
    };
    html.replacen(
        "  </head>",
        &format!("    <link rel=\"stylesheet\" href=\"{href}\" />\n  </head>"),
        1,
    )
}
