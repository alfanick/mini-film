/** Keep formatting, linting, and all three TypeScript projects aligned without long opaque package commands. */
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { errorMessage, packageBinary, requireSupportedNode, run } from "./tooling.mts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const formatPatterns = [
  "assets/**/*.{html,css,js}",
  "frontend/**/*.{ts,tsx,mts,mjs,json}",
  "scripts/*.{mts,json}",
  "*config*.{json,mjs,ts}",
];

/** Execute the pinned package's declared JavaScript binary without relying on PATH aliases. */
function binary(packageName: string, executable: string, args: readonly string[]): void {
  run(process.execPath, [packageBinary(packageName, executable, root), ...args], root);
}

/** Format every supported source category or verify that it already matches the shared layout. */
function format(write: boolean): void {
  binary("prettier", "prettier", [write ? "--write" : "--check", ...formatPatterns]);
}

/** Apply source diagnostics to browser code, typed helpers, stylesheets, and embedded HTML. */
function lint(): void {
  binary("eslint", "eslint", [
    "--max-warnings",
    "0",
    "assets/**/*.js",
    "frontend/**/*.{ts,tsx,mts,mjs}",
    "scripts/*.{mjs,mts}",
    "*config*.{mjs,ts}",
  ]);
  binary("stylelint", "stylelint", ["--max-warnings", "0", "assets/**/*.css"]);
  binary("htmlhint", "htmlhint", ["assets/**/*.html"]);
}

/** Check browser, tests, and Node helpers with native TypeScript while ESLint retains its compatible classic API. */
function typecheck(projects = ["tsconfig.review.json", "tsconfig.review-tests.json", "tsconfig.tooling.json"]): void {
  for (const project of projects) {
    binary("@typescript/native", "tsc", ["--project", project]);
  }
}

try {
  requireSupportedNode();
  const mode = process.argv[2];
  if (mode === "--write") format(true);
  else if (mode === "--check") format(false);
  else if (mode === "--lint") lint();
  else if (mode === "--typecheck") {
    const projects = new Map([
      ["review", "tsconfig.review.json"],
      ["tests", "tsconfig.review-tests.json"],
      ["tooling", "tsconfig.tooling.json"],
    ]);
    const project = projects.get(process.argv[3] ?? "");
    if (!project) throw new Error("Expected a review, tests, or tooling TypeScript project");
    typecheck([project]);
  } else if (mode === "--check-all") {
    format(false);
    lint();
    typecheck();
  } else throw new Error("Expected --write, --check, --lint, --typecheck, or --check-all");
} catch (error) {
  console.error(`Asset checks failed: ${errorMessage(error)}`);
  process.exitCode = 1;
}
