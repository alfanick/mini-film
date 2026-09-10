// Shared Cargo and standalone frontend builder. Isolated staging keeps npm and
// generated JavaScript out of the checkout and makes source packages buildable.
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { contractInputs, generateContracts } from "./review-contracts.mts";
import { errorMessage, isMissingFile, npm, packageBinary, required, requireSupportedNode, run } from "./tooling.mts";

const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const inputFiles = [
  "package.json",
  "package-lock.json",
  "tsconfig.review.json",
  "tsconfig.tooling.json",
  "eslint.config.mjs",
  "scripts/build-review.mts",
  "scripts/review-contracts.mts",
  "scripts/tooling.mts",
  "scripts/tsconfig.json",
];

/** Enumerate source inputs deterministically so additions and removals invalidate builds. */
async function filesUnder(root: string, relative: string): Promise<string[]> {
  const entries = await readdir(join(root, relative), { withFileTypes: true });
  const paths: string[] = [];
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    const path = `${relative}/${entry.name}`;
    if (entry.isDirectory()) paths.push(...(await filesUnder(root, path)));
    else if (entry.isFile()) paths.push(path);
    else throw new Error(`review build input must be a regular file or directory: ${path}`);
  }
  return paths;
}

/** Length-prefix each input to prevent ambiguous concatenations in cache keys. */
function digest(parts: readonly (string | Buffer)[]): string {
  const hash = createHash("sha256");
  for (const part of parts) {
    hash.update(String(part.length));
    hash.update(":");
    hash.update(part);
  }
  return hash.digest("hex");
}

/** Read an optional cache marker while preserving real filesystem errors. */
async function textIfPresent(path: string): Promise<string | undefined> {
  try {
    return await readFile(path, "utf8");
  } catch (error) {
    if (isMissingFile(error)) return undefined;
    throw error;
  }
}

/** Preserve unchanged output timestamps so Rust need not re-embed identical bytes. */
async function writeIfChanged(path: string, contents: string | Buffer): Promise<void> {
  let existing: Buffer | undefined;
  try {
    existing = await readFile(path);
  } catch (error) {
    if (!isMissingFile(error)) throw error;
  }
  const bytes = Buffer.isBuffer(contents) ? contents : Buffer.from(contents);
  if (!existing?.equals(bytes)) {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, bytes);
  }
}

/** Inputs shared by Cargo and standalone builds; optional locations remain explicit. */
export interface BuildOptions {
  sourceDir?: string;
  outputDir?: string;
  profile?: "debug" | "release";
  contractsDir?: string;
}

/** Report whether cached output or installed dependencies could be reused. */
export interface BuildResult {
  bundlePath: string;
  rebuilt: boolean;
  installed: boolean;
}

/** Stage, lint, type-check, and bundle one self-contained review application for Cargo. */
export async function buildReview({
  sourceDir = sourceRoot,
  outputDir,
  profile = "debug",
  contractsDir,
}: BuildOptions): Promise<BuildResult> {
  requireSupportedNode();
  const output = resolve(outputDir ?? join(sourceDir, "target/review-frontend"));
  const workspace = join(output, "review-workspace");
  const bundlePath = join(output, "review/app.js");
  const sources = [...inputFiles, ...(await filesUnder(sourceDir, "frontend/review"))];
  const contents = await Promise.all(sources.map((path) => readFile(join(sourceDir, path))));
  const schemas = contractsDir ?? join(sourceDir, "frontend/review/generated");
  const schemaContents = await Promise.all(contractInputs.map((path) => readFile(join(schemas, path))));
  const npmVersion = npm(["--version"], sourceDir, true);
  // Dependencies follow manifests and the host toolchain; source edits reuse
  // that install but invalidate the separately fingerprinted compiled bundle.
  const dependencyHash = digest([
    required(contents[0]),
    required(contents[1]),
    process.version,
    npmVersion,
    process.platform,
    process.arch,
  ]);
  const buildHash = digest([
    dependencyHash,
    profile,
    ...schemaContents,
    ...sources.flatMap((path, index) => [path, required(contents[index])]),
  ]);
  const dependencyMarker = join(workspace, ".dependencies.sha256");
  const buildMarker = join(workspace, ".build.sha256");
  const dependenciesReady =
    (await textIfPresent(dependencyMarker)) === dependencyHash &&
    existsSync(join(workspace, "node_modules/@typescript/native/bin/tsc")) &&
    existsSync(join(workspace, "node_modules/esbuild/package.json"));
  if (dependenciesReady && (await textIfPresent(buildMarker)) === buildHash && existsSync(bundlePath)) {
    return { bundlePath, rebuilt: false, installed: false };
  }
  await rm(buildMarker, { force: true });

  // Prune only our staged source subtree, never the checkout or node_modules.
  const frontendPath = join(workspace, "frontend/review");
  if (existsSync(frontendPath)) {
    for (const path of await filesUnder(workspace, "frontend/review")) {
      if (!sources.includes(path)) await rm(join(workspace, path));
    }
  }
  for (let index = 0; index < sources.length; index += 1) {
    await writeIfChanged(join(workspace, required(sources[index])), required(contents[index]));
  }
  if (!dependenciesReady) {
    console.error("Installing locked review UI build dependencies in Cargo build output...");
    npm(["ci", "--ignore-scripts", "--include=dev", "--include=optional", "--no-audit", "--no-fund"], workspace);
    await writeIfChanged(dependencyMarker, dependencyHash);
  }

  console.error(`Checking and bundling the review UI (${profile})...`);
  const require = createRequire(join(workspace, "package.json"));
  await generateContracts({
    schemaDir: schemas,
    outputDir: join(frontendPath, "generated"),
    dependenciesDir: workspace,
  });
  const eslint = join(dirname(require.resolve("eslint/package.json")), "bin/eslint.js");
  run(
    process.execPath,
    [eslint, "--max-warnings", "0", "frontend/review/**/*.{ts,tsx,mts}", "scripts/*.mts"],
    workspace,
  );
  run(
    process.execPath,
    [packageBinary("@typescript/native", "tsc", workspace), "--project", "tsconfig.review.json"],
    workspace,
  );
  run(
    process.execPath,
    [packageBinary("@typescript/native", "tsc", workspace), "--project", "tsconfig.tooling.json"],
    workspace,
  );
  // Cargo installs these locked packages in staging; their bundled declarations type the dynamic Node boundary.
  const { build } = require("esbuild") as typeof import("esbuild");
  const result = await build({
    absWorkingDir: workspace,
    entryPoints: [
      profile !== "release" && existsSync(join(frontendPath, "debug.tsx"))
        ? "frontend/review/debug.tsx"
        : "frontend/review/main.tsx",
    ],
    outfile: bundlePath,
    bundle: true,
    splitting: false,
    platform: "browser",
    format: "esm",
    target: "es2020",
    minify: profile === "release",
    sourcemap: false,
    legalComments: "inline",
    metafile: true,
    write: false,
    logLevel: "warning",
  });
  const outputs = Object.values(result.metafile.outputs);
  if (result.outputFiles.length !== 1 || outputs.some((item) => item.imports.length !== 0)) {
    throw new Error("review UI must compile to one JavaScript file without external imports");
  }
  const ts = require("typescript") as typeof import("typescript");
  const javascript = required(result.outputFiles[0]).text;
  assertSelfContained(javascript, ts);
  const licenses = await bundledLicenses(workspace, result.metafile.inputs);
  const bundle = `${licenses}\n${javascript}`;
  const gzipBytes = gzipSync(bundle, { level: 9 }).length;
  if (profile === "release" && gzipBytes > 80 * 1024) {
    throw new Error(`review UI exceeds its 80 KiB gzip budget: ${gzipBytes} bytes`);
  }
  console.error(`Review bundle: ${Buffer.byteLength(bundle)} bytes, ${gzipBytes} bytes gzip`);
  await writeIfChanged(bundlePath, bundle);
  await writeIfChanged(buildMarker, buildHash);
  return { bundlePath, rebuilt: true, installed: !dependenciesReady };
}

/** Reject every remaining module load, including computed imports omitted from esbuild's import metadata. */
export function assertSelfContained(source: string, ts: typeof import("typescript")): void {
  const file = ts.createSourceFile("app.js", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  /** Walk emitted syntax instead of matching strings that may occur inside comments or UI text. */
  function visit(node: import("typescript").Node): void {
    if (
      ts.isImportDeclaration(node) ||
      (ts.isExportDeclaration(node) && node.moduleSpecifier) ||
      (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword)
    ) {
      throw new Error("review UI must compile to one JavaScript file without external imports");
    }
    ts.forEachChild(node, visit);
  }
  visit(file);
}

/** Retain licenses for exactly the runtime packages present in the embedded bundle. */
async function bundledLicenses(workspace: string, inputs: import("esbuild").Metafile["inputs"]): Promise<string> {
  const packages = new Set(
    Object.keys(inputs).flatMap((path) => {
      const match = /(?:^|\/)node_modules\/((?:@[^/]+\/)?[^/]+)/.exec(path);
      return match?.[1] ? [match[1]] : [];
    }),
  );
  const licenses: string[] = [];
  for (const name of [...packages].sort()) {
    const directory = join(workspace, "node_modules", name);
    const filename = (await readdir(directory)).find((file) => /^licen[sc]e(?:\.(?:txt|md))?$/i.test(file));
    if (!filename) throw new Error(`Bundled dependency ${name} has no license file`);
    const label = name === "preact" ? "Preact" : name;
    licenses.push(`/*! ${label}\n${(await readFile(join(directory, filename), "utf8")).trim()}\n*/`);
  }
  return licenses.join("\n");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    let outputDir: string | undefined;
    let contractsDir: string | undefined;
    let profile: "debug" | "release" = "debug";
    const args = process.argv.slice(2);
    while (args.length) {
      const option = args.shift();
      if (option === "--cargo-out-dir" || option === "--out-dir") outputDir = args.shift();
      else if (option === "--contracts-dir") contractsDir = args.shift();
      else if (option === "--profile") {
        const value = args.shift();
        if (value !== "debug" && value !== "release") throw new Error("Expected --profile debug or release");
        profile = value;
      } else throw new Error(`unknown review build option: ${option}`);
      if (!outputDir && (option === "--cargo-out-dir" || option === "--out-dir")) {
        throw new Error(`missing value for ${option}`);
      }
      if (option === "--contracts-dir" && !contractsDir) throw new Error(`missing value for ${option}`);
    }
    await buildReview({
      profile,
      ...(outputDir ? { outputDir } : {}),
      ...(contractsDir ? { contractsDir } : {}),
    });
  } catch (error) {
    console.error(`Review UI build failed: ${errorMessage(error)}`);
    process.exitCode = 1;
  }
}
