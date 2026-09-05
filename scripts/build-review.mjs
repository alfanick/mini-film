// Shared Cargo and standalone frontend builder. Isolated staging keeps npm and
// generated JavaScript out of the checkout and makes source packages buildable.
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const inputFiles = [
  "package.json",
  "package-lock.json",
  "tsconfig.review.json",
  "eslint.config.mjs",
  "scripts/build-review.mjs",
];

/** Run a build tool with inherited diagnostics, optionally collecting a version. */
function run(command, args, cwd, capture = false) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: capture ? ["ignore", "pipe", "pipe"] : "inherit",
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.error || result.status !== 0) {
    const detail = result.error?.message ?? result.stderr?.trim() ?? `exit ${result.status}`;
    throw new Error(`${command} ${args.join(" ")} failed: ${detail}`);
  }
  return result.stdout?.trim();
}

/** Locate npm without shell-interpolating filesystem paths, including Windows. */
function npm(args, cwd, capture = false) {
  // npm.cmd needs a shell on Windows. Prefer its JavaScript entry point so
  // paths containing spaces work without shell interpolation on either OS.
  const candidates = [
    process.env.npm_execpath,
    join(dirname(process.execPath), "node_modules/npm/bin/npm-cli.js"),
    join(dirname(process.execPath), "../lib/node_modules/npm/bin/npm-cli.js"),
  ];
  const cli = candidates.find((path) => path && existsSync(path));
  if (cli) return run(process.execPath, [cli, ...args], cwd, capture);
  if (process.platform === "win32") {
    return run(process.env.ComSpec || "cmd.exe", ["/d", "/s", "/c", "npm", ...args], cwd, capture);
  }
  return run("npm", args, cwd, capture);
}

/** Enumerate source inputs deterministically so additions and removals invalidate builds. */
async function filesUnder(root, relative) {
  const entries = await readdir(join(root, relative), { withFileTypes: true });
  const paths = [];
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    const path = `${relative}/${entry.name}`;
    if (entry.isDirectory()) paths.push(...(await filesUnder(root, path)));
    else if (entry.isFile()) paths.push(path);
    else throw new Error(`review build input must be a regular file or directory: ${path}`);
  }
  return paths;
}

/** Length-prefix each input to prevent ambiguous concatenations in cache keys. */
function digest(parts) {
  const hash = createHash("sha256");
  for (const part of parts) {
    hash.update(String(part.length));
    hash.update(":");
    hash.update(part);
  }
  return hash.digest("hex");
}

/** Read an optional cache marker while preserving real filesystem errors. */
async function textIfPresent(path) {
  try {
    return await readFile(path, "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return undefined;
    throw error;
  }
}

/** Preserve unchanged output timestamps so Rust need not re-embed identical bytes. */
async function writeIfChanged(path, contents) {
  let existing;
  try {
    existing = await readFile(path);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  const bytes = Buffer.isBuffer(contents) ? contents : Buffer.from(contents);
  if (!existing?.equals(bytes)) {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, bytes);
  }
}

/** Stage, lint, type-check, and bundle one self-contained review application for Cargo. */
export async function buildReview({ sourceDir = sourceRoot, outputDir, profile = "debug" }) {
  if (Number(process.versions.node.split(".")[0]) < 24) {
    throw new Error("building the review UI requires Node.js 24 or newer");
  }
  const output = resolve(outputDir ?? join(sourceDir, "target/review-frontend"));
  const workspace = join(output, "review-workspace");
  const bundlePath = join(output, "review/app.js");
  const sources = [...inputFiles, ...(await filesUnder(sourceDir, "frontend/review"))];
  const contents = await Promise.all(sources.map((path) => readFile(join(sourceDir, path))));
  const npmVersion = npm(["--version"], sourceDir, true);
  // Dependencies follow manifests and the host toolchain; source edits reuse
  // that install but invalidate the separately fingerprinted compiled bundle.
  const dependencyHash = digest([
    contents[0],
    contents[1],
    process.version,
    npmVersion,
    process.platform,
    process.arch,
  ]);
  const buildHash = digest([dependencyHash, profile, ...sources.flatMap((path, index) => [path, contents[index]])]);
  const dependencyMarker = join(workspace, ".dependencies.sha256");
  const buildMarker = join(workspace, ".build.sha256");
  const dependenciesReady =
    (await textIfPresent(dependencyMarker)) === dependencyHash &&
    existsSync(join(workspace, "node_modules/typescript/bin/tsc")) &&
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
    await writeIfChanged(join(workspace, sources[index]), contents[index]);
  }
  if (!dependenciesReady) {
    console.error("Installing locked review UI build dependencies in Cargo build output...");
    npm(["ci", "--ignore-scripts", "--include=dev", "--include=optional", "--no-audit", "--no-fund"], workspace);
    await writeIfChanged(dependencyMarker, dependencyHash);
  }

  console.error(`Checking and bundling the review UI (${profile})...`);
  const require = createRequire(join(workspace, "package.json"));
  const eslint = join(dirname(require.resolve("eslint/package.json")), "bin/eslint.js");
  run(process.execPath, [eslint, "--max-warnings", "0", "frontend/review/**/*.{ts,tsx}"], workspace);
  run(process.execPath, [require.resolve("typescript/bin/tsc"), "--project", "tsconfig.review.json"], workspace);
  const { build } = require("esbuild");
  const license = await readFile(join(workspace, "node_modules/preact/LICENSE"), "utf8");
  const result = await build({
    absWorkingDir: workspace,
    entryPoints: ["frontend/review/main.tsx"],
    outfile: bundlePath,
    bundle: true,
    splitting: false,
    platform: "browser",
    format: "esm",
    target: "es2020",
    minify: profile === "release",
    sourcemap: false,
    legalComments: "inline",
    banner: { js: `/*! Preact\n${license.trim()}\n*/` },
    metafile: true,
    write: false,
    logLevel: "warning",
  });
  const outputs = Object.values(result.metafile.outputs);
  if (result.outputFiles.length !== 1 || outputs.some((item) => item.imports.length !== 0)) {
    throw new Error("review UI must compile to one JavaScript file without external imports");
  }
  await writeIfChanged(bundlePath, result.outputFiles[0].contents);
  await writeIfChanged(buildMarker, buildHash);
  return { bundlePath, rebuilt: true, installed: !dependenciesReady };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    let outputDir;
    let profile = "debug";
    const args = process.argv.slice(2);
    while (args.length) {
      const option = args.shift();
      if (option === "--cargo-out-dir" || option === "--out-dir") outputDir = args.shift();
      else if (option === "--profile") profile = args.shift();
      else throw new Error(`unknown review build option: ${option}`);
      if (!outputDir && option !== "--profile") throw new Error(`missing value for ${option}`);
      if (!profile) throw new Error(`missing value for ${option}`);
    }
    await buildReview({ outputDir, profile });
  } catch (error) {
    console.error(`Review UI build failed: ${error.message}`);
    process.exitCode = 1;
  }
}
