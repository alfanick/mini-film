# AGENTS.md

Guidance for AI agents and humans working on `mini-film`.

Start by reading:

1. `README.md` for the product shape, commands, external tools, and user-visible behavior.
2. `Cargo.toml` for crate version, features, and dependency boundaries.
3. Relevant source modules before changing behavior:
   - `src/app/apply.rs` for the shared RAW -> render -> metadata pipeline.
   - `src/app/profile.rs`, `src/xmp.rs`, and `src/pp3.rs` for profile/XMP/RawTherapee mapping.
   - `src/app/review/` for daemon review state, Codex analysis, publish, and UI behavior.
   - `src/app/nikon_wtu.rs` for Nikon transfer protocol handling.

## Working contract

### 1. Version discipline

- Bump versions exactly as requested.
- Patch bump means increment `Z`; minor bump means increment `Y`; breaking bump means increment `X`.
- Keep all project version surfaces in sync:
  - `Cargo.toml`
  - `Cargo.lock`
  - `package.json`
- A rework keeps the same version only when explicitly requested.
- If the user names an expected baseline version and the repo differs, stop and reconcile before editing.

### 2. Minimal scope

- Do only what was asked.
- Do not add opportunistic refactors, comment churn, or formatting churn.
- Preserve existing behavior unless the requested change requires altering it.
- If an unrelated improvement becomes obvious, mention it as a follow-up instead of expanding the current patch.
- Do not remove supported concise syntax or compatibility paths just because a cleaner shape exists.

### 3. Keep docs, help, and behavior aligned

- User-visible behavior changes require README updates.
- CLI option changes require clap help text updates.
- If a command example changes, update the nearest README example too.
- Keep generated/apply metadata behavior documented; photographers rely on EXIF/XMP round-tripping.

### 4. Output and metadata rules

- Preserve source RAW metadata unless `--strip-metadata` is explicitly active.
- Treat review metadata as user-owned data.
- Manual review edits always win over automated analysis.
- Camera-provided ratings, labels, and notes must be imported when available and must not be overwritten by Codex suggestions.
- Do not invent EXIF/XMP fields or fake adjustment values. Write only values mini-film actually knows.
- Prefer standard Adobe/ExifTool-compatible XMP names before adding custom namespaces.

### 5. RAW pipeline rules

- `apply_resolved` is the shared render path. Put common render/export/metadata behavior there when possible so `apply`, `batch`, daemon, and review publish stay consistent.
- `sampler` has a thumbnail-specific path; mirror shared behavior there only when that path does not call `apply_resolved`.
- RawTherapee remains the RAW engine. Do not silently replace it with another renderer.
- Hald CLUTs should stay RGBTable-only; spatial operations like sharpening belong in generated RawTherapee profiles or later image stages.
- Deterministic grain seeds matter for repeatability. Avoid introducing time-sensitive nondeterminism unless the command already uses it intentionally.

### 6. Nikon ingest rules

- Nikon WTU transfer is protocol-sensitive. Keep tricky comments and doc comments near protocol code.
- Expected camera-offline/reconnect states should stay quiet; do not spam logs for normal camera-off cases.
- Unsupported JPEG queue items may be expected; consume/discard them when needed so the camera queue advances.
- Do not change pairing, GUID, or queue-drain semantics without validating against the actual protocol behavior.

### 7. Logging and UX

- Progress and error output should be copy/paste friendly.
- Reserve error-level wording for actionable failures.
- In daemon/review flows, avoid noisy logs for expected offline, skipped, or unsupported cases.
- UI behavior should remain usable on desktop and mobile. Keep review controls keyboard-friendly and touch-friendly.

### 8. Frontend and assets

- The review/gallery UI uses embedded assets. Keep HTML/CSS/JS lightweight and compatible with current browser engines.
- Do not introduce frontend frameworks.
- If touching assets, run the same asset checks used by the repo hooks when validating.
- Keep light/dark/system theme behavior intact unless the task is explicitly visual redesign.

### 9. ASCII and formatting

- Prefer ASCII in code, docs, scripts, and generated text unless the file already requires non-ASCII.
- Use `cargo fmt --all` only when formatting is requested or required by hooks/checks.
- Avoid broad formatting-only diffs.

### 10. Validation

Use the smallest validation that proves the change, unless the user asks for the full suite.

Common checks:

```sh
cargo check --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --all-targets --no-default-features -- -D warnings
cargo clippy --all-targets -- -D warnings
npm run check:assets
```

For metadata changes, prefer validating with `exiftool` on a real output when feasible.
For Nikon ingest changes, prefer real-process/protocol validation over startup-only checks.
For README/help changes, compare the documented command with actual clap behavior when practical.

### 11. Commit and push workflow

- Commit only when explicitly asked.
- Stage only files that belong to the requested change.
- Use concise commit messages.
- When asked to push to both origins, push the current branch to both configured remotes, normally `origin` and `github`.
- Do not amend or rewrite history unless explicitly asked.
- If hooks run checks, let them run and fix real failures rather than bypassing them.

### 12. Git identity

When committing via Codex, set the commit identity for that commit only if needed:

```sh
git -c user.name='Codex' -c user.email='juskowiak+ai@amadeusz.me' commit ...
```

Do not change the user's global git config.
