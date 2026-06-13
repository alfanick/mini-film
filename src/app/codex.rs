use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::json;
use tempfile::Builder;

use crate::cli::CodexAnalysisFlags;

#[derive(Clone, Debug)]
pub(crate) struct CodexAnalysisOptions {
    pub(crate) codex_binary: PathBuf,
    pub(crate) model: String,
    pub(crate) timeout: Duration,
    pub(crate) flags: CodexAnalysisFlags,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodexAnalysisResult {
    pub(crate) tags: Vec<String>,
    pub(crate) note: Option<String>,
    pub(crate) rating: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct RawCodexAnalysis {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    rating: Option<u8>,
}

/// Run Codex on a small camera preview and normalize the structured answer.
///
/// The daemon already extracts embedded JPEG previews for review thumbnails, so
/// image analysis uses that small file instead of making Codex read the RAW. The
/// prompt is intentionally narrow and the output is constrained with Codex
/// CLI's JSON-schema option. The returned text is still validated locally
/// because command-line model output is an external process boundary and should
/// not be trusted to be perfectly shaped.
pub(crate) fn run_codex_image_analysis(
    preview: &Path,
    options: &CodexAnalysisOptions,
) -> Result<CodexAnalysisResult> {
    if !options.flags.is_enabled() {
        return Ok(CodexAnalysisResult::default());
    }
    if !preview.is_file() {
        bail!("Codex preview image is missing: {}", preview.display());
    }

    let temp_dir = Builder::new().prefix("mini-film-codex-").tempdir()?;
    let schema_path = temp_dir.path().join("schema.json");
    let output_path = temp_dir.path().join("analysis.json");
    fs::write(&schema_path, codex_output_schema())
        .with_context(|| format!("writing {}", schema_path.display()))?;

    let prompt = codex_prompt(options.flags);
    let mut child = Command::new(&options.codex_binary)
        .arg("exec")
        .arg("--ephemeral")
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--ask-for-approval")
        .arg("never")
        .arg("-m")
        .arg(&options.model)
        .arg("-i")
        .arg(preview)
        .arg("--output-schema")
        .arg(&schema_path)
        .arg("--output-last-message")
        .arg(&output_path)
        .arg(prompt)
        .current_dir(temp_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting Codex at {}", options.codex_binary.display()))?;

    let deadline = Instant::now() + options.timeout;
    loop {
        if let Some(status) = child.try_wait().context("waiting for Codex")? {
            let output = child
                .wait_with_output()
                .context("collecting Codex output")?;
            if !status.success() {
                bail!(
                    "Codex exited with status {status}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Codex image analysis timed out after {:?}", options.timeout);
        }
        thread::sleep(Duration::from_millis(50));
    }

    let text = fs::read_to_string(&output_path)
        .with_context(|| format!("reading Codex analysis {}", output_path.display()))?;
    parse_codex_analysis_output(&text, options.flags)
}

pub(crate) fn parse_codex_analysis_output(
    text: &str,
    flags: CodexAnalysisFlags,
) -> Result<CodexAnalysisResult> {
    let raw = serde_json::from_str::<RawCodexAnalysis>(text.trim())
        .with_context(|| format!("parsing Codex JSON response: {}", text.trim()))?;
    let mut result = CodexAnalysisResult::default();

    if flags.tags {
        result.tags = normalize_codex_tags(raw.tags);
        if result.tags.is_empty() {
            bail!("Codex returned no usable tags");
        }
    }
    if flags.note {
        result.note = raw
            .note
            .map(|note| normalize_codex_note(&note))
            .filter(|note| !note.is_empty());
        if result.note.is_none() {
            bail!("Codex returned no usable note");
        }
    }
    if flags.rating {
        let rating = raw
            .rating
            .ok_or_else(|| anyhow!("Codex returned no rating"))?;
        if rating > 3 {
            bail!("Codex rating must be 0..3, got {rating}");
        }
        result.rating = Some(rating);
    }

    Ok(result)
}

pub(crate) fn normalize_codex_tags(tags: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for tag in tags {
        let normalized = normalize_codex_tag(&tag);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        out.push(normalized);
        if out.len() == 10 {
            break;
        }
    }
    out
}

fn normalize_codex_tag(raw: &str) -> String {
    let mut tag = String::new();
    let mut last_dash = false;
    for ch in raw.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            tag.push(ch);
            last_dash = false;
        } else if !last_dash && !tag.is_empty() {
            tag.push('-');
            last_dash = true;
        }
        if tag.len() >= 32 {
            break;
        }
    }
    tag.trim_matches('-').to_string()
}

fn normalize_codex_note(raw: &str) -> String {
    let mut note = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if note.len() > 240 {
        note.truncate(240);
        if let Some((prefix, _)) = note.rsplit_once(' ') {
            note = prefix.to_string();
        }
    }
    note.trim().to_string()
}

fn codex_output_schema() -> String {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "tags": {
                "type": "array",
                "minItems": 0,
                "maxItems": 10,
                "items": { "type": "string" }
            },
            "note": { "type": ["string", "null"] },
            "rating": { "type": ["integer", "null"], "minimum": 0, "maximum": 3 }
        },
        "required": ["tags", "note", "rating"]
    })
    .to_string()
}

fn codex_prompt(flags: CodexAnalysisFlags) -> String {
    let mut requested = Vec::new();
    if flags.tags {
        requested.push("tags");
    }
    if flags.note {
        requested.push("note");
    }
    if flags.rating {
        requested.push("rating");
    }
    format!(
        "Analyze the attached photo preview for a photographer's review workflow.\n\
         Return JSON only.\n\
         Requested fields: {}.\n\
         tags: generate 1 to 10 concise lower-case subject/action/context tags from visible content only.\n\
         note: one simple sentence describing what is in the picture.\n\
         rating: 0 for technically bad/unusable, 1 for technically correct, 2 for interesting, 3 for truly good.\n\
         If a field is not requested, return [] for tags, null for note, and null for rating.",
        requested.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_tags_are_normalized_deduped_and_capped() {
        let tags = normalize_codex_tags(vec![
            " Sports Photo ".to_string(),
            "sports_photo".to_string(),
            "Goal!".to_string(),
            "A".repeat(80),
        ]);
        assert_eq!(tags[0], "sports-photo");
        assert!(tags[1].starts_with("goal"));
        assert!(tags[2].len() <= 32);
    }

    #[test]
    fn codex_output_parser_respects_requested_fields() {
        let flags = CodexAnalysisFlags {
            tags: true,
            note: true,
            rating: true,
        };
        let parsed = parse_codex_analysis_output(
            r#"{"tags":["Football","Goal"],"note":"A player scores a goal.","rating":2}"#,
            flags,
        )
        .unwrap();
        assert_eq!(parsed.tags, vec!["football", "goal"]);
        assert_eq!(parsed.note.as_deref(), Some("A player scores a goal."));
        assert_eq!(parsed.rating, Some(2));
    }

    #[test]
    fn codex_output_parser_rejects_missing_requested_fields() {
        let flags = CodexAnalysisFlags {
            tags: false,
            note: false,
            rating: true,
        };
        assert!(
            parse_codex_analysis_output(r#"{"tags":[],"note":null,"rating":7}"#, flags).is_err()
        );
    }
}
