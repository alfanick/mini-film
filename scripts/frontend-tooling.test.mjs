// Verify editor project discovery and the shared lint contract without depending
// on the current UI implementation or launching an editor-specific plugin.
import assert from "node:assert/strict";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { ESLint } from "eslint";
import ts from "typescript";
import { formatStagedAssets } from "./format-staged-assets.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const projects = [
  ["frontend/review/core/types.ts", "frontend/review/tsconfig.json", []],
  ["frontend/review/generated/validators.d.mts", "frontend/review/tsconfig.json", []],
  ["frontend/tests/review.spec.ts", "frontend/tests/tsconfig.json", ["node"]],
  ["playwright.config.ts", "tsconfig.json", ["node"]],
];

// Each source category must resolve a checked-in configuration, not an inferred project.
test("TypeScript discovers browser, test, and Playwright projects with strict options", () => {
  for (const [relative, expectedConfig, types] of projects) {
    const file = join(root, relative);
    const configPath = ts.findConfigFile(dirname(file), ts.sys.fileExists);
    assert.equal(configPath, join(root, expectedConfig));
    const config = ts.readConfigFile(configPath, ts.sys.readFile);
    assert.equal(config.error, undefined);
    const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, dirname(configPath));
    assert.deepEqual(parsed.errors, []);
    assert.ok(parsed.fileNames.includes(file));
    assert.equal(parsed.options.strict, true);
    assert.equal(parsed.options.noUnusedLocals, true);
    assert.equal(parsed.options.noUnusedParameters, true);
    assert.equal(parsed.options.noUncheckedIndexedAccess, true);
    assert.equal(parsed.options.exactOptionalPropertyTypes, true);
    assert.equal(parsed.options.noPropertyAccessFromIndexSignature, true);
    assert.deepEqual(parsed.options.types, types);
    assert.equal(parsed.options.jsx, ts.JsxEmit.ReactJSX);
    assert.equal(parsed.options.jsxImportSource, "preact");
  }
});

// Apply identical unsafe-value and promise rules to application and test TypeScript.
test("type-aware linting rejects unsafe values, h views, suppressions, and unused declarations", async () => {
  const eslint = new ESLint({ cwd: root });
  for (const [relative] of projects) {
    const config = await eslint.calculateConfigForFile(join(root, relative));
    assert.equal(config.languageOptions.parserOptions.projectService, true);
    assert.equal(config.linterOptions.noInlineConfig, true);
    for (const rule of [
      "@typescript-eslint/no-explicit-any",
      "@typescript-eslint/no-floating-promises",
      "@typescript-eslint/no-misused-promises",
      "@typescript-eslint/no-unsafe-argument",
      "@typescript-eslint/no-unsafe-assignment",
      "@typescript-eslint/no-unsafe-call",
      "@typescript-eslint/no-unsafe-member-access",
      "@typescript-eslint/no-unsafe-return",
      "@typescript-eslint/no-unused-vars",
      "@typescript-eslint/ban-ts-comment",
      "no-restricted-imports",
      "no-restricted-syntax",
      "@stylistic/max-len",
      "react-hooks/rules-of-hooks",
      "react-hooks/exhaustive-deps",
    ]) {
      assert.equal(config.rules[rule][0], 2, `${relative}: ${rule} is an error`);
    }
  }
});

/** Execute isolated fixture Git commands while exposing setup failures clearly. */
function git(cwd, ...args) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

// Automatic formatting must preserve partial staging and keep rejecting stale index bytes.
test("pre-commit formatting touches staged paths but preserves index and unstaged files", async (context) => {
  const temporary = await mkdtemp(join(tmpdir(), "mini-film staged formatter "));
  context.after(() => rm(temporary, { recursive: true, force: true }));
  git(temporary, "init", "--quiet");
  await mkdir(join(temporary, "frontend"));
  await copyFile(join(root, ".prettierrc.json"), join(temporary, ".prettierrc.json"));
  const path = "frontend/staged file.ts";
  const staged = "export const values=[1,2,3];\n";
  const unstaged = "export const untouched=[4,5];\n";
  await writeFile(join(temporary, path), staged);
  await writeFile(join(temporary, "frontend/unstaged.ts"), unstaged);
  git(temporary, "add", "--", path);
  await writeFile(join(temporary, path), `${staged}export const partial=[6,7];\n`);

  assert.deepEqual(await formatStagedAssets(temporary), [path]);
  assert.equal(git(temporary, "show", `:${path}`), staged);
  assert.equal(await readFile(join(temporary, "frontend/unstaged.ts"), "utf8"), unstaged);
  assert.match(await readFile(join(temporary, path), "utf8"), /export const partial = \[6, 7\]/);
  assert.deepEqual(await formatStagedAssets(temporary), [path]);
  git(temporary, "add", "--", path);
  assert.deepEqual(await formatStagedAssets(temporary), []);
});
