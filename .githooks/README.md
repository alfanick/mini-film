# Git hooks

This repository ships local Git hooks in `.githooks`.

Install them once per clone:

```sh
git config core.hooksPath .githooks
```

Installed hooks:

- `pre-commit` runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`
- `pre-push` runs the same checks
- `run-style-checks.sh` (used by both hooks) now includes a toolchain version check via `check-cargo-versions.sh`

This makes both commit and push fail early when formatting or lint checks are not clean.
