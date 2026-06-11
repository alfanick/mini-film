# Git hooks

This repository ships local Git hooks in `.githooks`.

Install them once per clone:

```sh
git config core.hooksPath .githooks
```

Installed hooks:

- `pre-commit` checks toolchain versions, direct Cargo dependency freshness,
  frontend assets, Rust formatting, CLI-only clippy, and GUI clippy when the
  local WebKitGTK development packages are installed.
- `pre-push` runs the same checks.
- `run-style-checks.sh` is the shared check runner used by both hooks.

This makes both commit and push fail early when formatting or lint checks are not clean.
