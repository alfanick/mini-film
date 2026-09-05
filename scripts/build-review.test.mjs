// Exercise the real frontend build with disposable source checkouts. These
// checks protect Cargo caching, error propagation, and the single-file contract.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { copyFile, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { assertSelfContained, buildReview } from "./build-review.mjs";
import { contractInputs } from "./review-contracts.mjs";
import ts from "typescript";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// A single lifecycle proves failed builds cannot leave a valid cache marker.
test("Cargo staging installs, invalidates, checks types, and emits one runtime file", async (context) => {
  const temporary = await mkdtemp(join(tmpdir(), "mini-film review build "));
  context.after(() => rm(temporary, { recursive: true, force: true }));
  const sourceDir = join(temporary, "source checkout");
  const outputDir = join(temporary, "cargo output");
  await mkdir(join(sourceDir, "frontend/review"), { recursive: true });
  await mkdir(join(sourceDir, "scripts"));
  await mkdir(join(sourceDir, "frontend/review/core"));
  await mkdir(join(sourceDir, "frontend/review/generated"));
  for (const path of [
    "package.json",
    "package-lock.json",
    "tsconfig.review.json",
    "eslint.config.mjs",
    "scripts/build-review.mjs",
    "scripts/review-contracts.mjs",
    "frontend/review/core/transport.ts",
  ]) {
    await copyFile(join(root, path), join(sourceDir, path));
  }
  for (const file of contractInputs) {
    await copyFile(join(root, "frontend/review/generated", file), join(sourceDir, "frontend/review/generated", file));
  }
  await copyFile(join(root, "frontend/review/tsconfig.json"), join(sourceDir, "frontend/review/tsconfig.json"));
  const entry = join(sourceDir, "frontend/review/main.tsx");
  const view = join(sourceDir, "frontend/review/view.tsx");
  await writeFile(
    entry,
    'import { render } from "preact"; import { view } from "./view"; render(view, document.body);\n',
  );
  await writeFile(view, "export const view = <button>Build fixture</button>;\n");

  const options = { sourceDir, outputDir };
  const first = await buildReview(options);
  assert.equal(first.installed, true);
  assert.equal(first.rebuilt, true);
  assert.equal(existsSync(join(sourceDir, "node_modules")), false);
  const initialTime = (await stat(first.bundlePath)).mtimeMs;
  const initialBundle = await readFile(first.bundlePath, "utf8");
  assert.match(initialBundle, /Build fixture/);
  assert.match(initialBundle, /Preact/);
  assert.match(initialBundle, /Permission is hereby granted/);
  assert.doesNotMatch(initialBundle, /sourceMappingURL|from ["']preact|import\(/);
  assert.deepEqual(await readdir(dirname(first.bundlePath)), ["app.js"]);

  const unchanged = await buildReview(options);
  assert.equal(unchanged.rebuilt, false);
  assert.equal((await stat(first.bundlePath)).mtimeMs, initialTime);

  await writeFile(view, "export const view = <button>Updated fixture</button>;\n");
  assert.equal((await buildReview(options)).installed, false);
  assert.match(await readFile(first.bundlePath, "utf8"), /Updated fixture/);

  const addedModule = join(sourceDir, "frontend/review/added.ts");
  await writeFile(addedModule, 'export const added: number = "invalid added module";\n');
  await assert.rejects(buildReview(options), /tsc.*failed/);
  await rm(addedModule);
  assert.equal((await buildReview(options)).rebuilt, true);
  assert.equal(existsSync(join(outputDir, "review-workspace/frontend/review/added.ts")), false);

  const configPath = join(sourceDir, "tsconfig.review.json");
  const config = JSON.parse(await readFile(configPath, "utf8"));
  config.compilerOptions.noUnusedLocals = true;
  await writeFile(configPath, JSON.stringify(config));
  assert.equal((await buildReview(options)).rebuilt, true);

  const packagePath = join(sourceDir, "package.json");
  const lockPath = join(sourceDir, "package-lock.json");
  const manifest = JSON.parse(await readFile(packagePath, "utf8"));
  const lock = JSON.parse(await readFile(lockPath, "utf8"));
  manifest.version = "0.0.0-build-fixture";
  lock.version = manifest.version;
  lock.packages[""].version = manifest.version;
  await writeFile(packagePath, JSON.stringify(manifest));
  await writeFile(lockPath, JSON.stringify(lock));
  assert.equal((await buildReview(options)).installed, true);

  await writeFile(view, 'export const view: number = "intentional type error";\n');
  await assert.rejects(buildReview(options), /tsc.*failed/);
  assert.match(await readFile(first.bundlePath, "utf8"), /Updated fixture/);
  await writeFile(view, "export const view = <button>Updated fixture</button>;\n");
  const release = await buildReview({ ...options, profile: "release" });
  assert.equal(release.rebuilt, true);
  assert.equal(release.installed, false);
  assert.ok((await stat(release.bundlePath)).size < Buffer.byteLength(initialBundle));

  const independent = await buildReview({
    ...options,
    outputDir: join(temporary, "other feature build"),
    profile: "release",
  });
  assert.equal(independent.installed, true);
  assert.equal(independent.rebuilt, true);
  assert.deepEqual(await readFile(independent.bundlePath), await readFile(release.bundlePath));
  assert.equal(existsSync(join(sourceDir, "node_modules")), false);

  // Cargo must enforce the same type-aware source rules as npm and editors.
  for (const invalidSource of [
    "const unsafe: any = {}; unsafe();\n",
    'const unsafe = JSON.parse("{}"); unsafe.missing();\n',
    "Promise.resolve();\n",
    'import { h, render } from "preact"; render(h("div", null, "imperative markup"), document.body);\n',
  ]) {
    await writeFile(entry, invalidSource);
    await assert.rejects(buildReview(options), /eslint.*failed/);
  }

  await writeFile(entry, 'import "missing-review-module";\n');
  await assert.rejects(buildReview(options), /failed/);

  const mismatchedManifest = { ...manifest, dependencies: { preact: "0.0.0-invalid-fixture" } };
  await writeFile(packagePath, JSON.stringify(mismatchedManifest));
  await assert.rejects(buildReview(options), /failed/);
});

// Import metadata alone misses computed dynamic imports; the final emitted syntax must also be checked.
test("single-file validation rejects computed imports but accepts import-like UI strings", () => {
  assert.throws(() => assertSelfContained("const path = location.hash; void import(path);", ts), /external imports/);
  assert.throws(() => assertSelfContained('export { view } from "./chunk.js";', ts), /external imports/);
  assert.doesNotThrow(() => assertSelfContained('const text = "import(path)"; document.title = text;', ts));
});
