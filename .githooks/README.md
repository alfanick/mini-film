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
- `pre-push` runs the same checks, followed by the full non-GUI Cargo test suite
  serialized to avoid races between tests that temporarily modify process state.
- `run-style-checks.sh` is the shared check runner used by both hooks.

This makes both commit and push fail early when formatting or lint checks are not clean.

Frontend checks require Node.js 24 or newer and npm. The asset hook installs
locked tooling locally when needed, then formats/lints the embedded assets and
TypeScript/TSX sources and checks review UI types. Cargo builds independently
install and compile the review UI inside Cargo's build output directory.

The pre-commit hook automatically formats staged frontend paths to the project's
120-column style. It preserves the Git index: if formatting changes a working
file, the hook lists the paths to review and re-stage before retrying the commit.
CI and pre-push check formatting without rewriting source files. Type-aware
ESLint rules reject warnings, unsafe values, unhandled promises, and suppressions.
