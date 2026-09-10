// Reproduce CI's two-pass visual checks without writing dependencies, screenshots, or build output into source paths.
import { cp, mkdir, mkdtemp } from "node:fs/promises";
import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { errorMessage, isMissingFile, requireSupportedNode } from "./tooling.mts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Forward subprocess diagnostics while failing the outer check if either test pass fails. */
function run(command: string, args: readonly string[], cwd: string): Promise<void> {
  return new Promise<void>((resolveRun, reject) => {
    const child = spawn(command, args, { cwd, stdio: "inherit", windowsHide: true });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolveRun();
      else reject(new Error(`${command} failed (${signal ?? `exit ${code}`})`));
    });
  });
}

/** Install from the lockfile in disposable output, then compare both bundles against fresh same-browser references. */
async function checkInsideContainer(args: readonly string[]): Promise<void> {
  const workspace = await mkdtemp(join(root, "target/run-"));
  console.info(`Visual workspace and artifacts: ${workspace}`);
  const sources = [
    "assets",
    "frontend",
    "scripts",
    "package.json",
    "package-lock.json",
    "tsconfig.json",
    "tsconfig.review.json",
    "tsconfig.review-tests.json",
    "tsconfig.tooling.json",
    "eslint.config.mjs",
    "playwright.config.ts",
    "playwright.pure.config.ts",
    ".prettierrc.json",
  ];
  await Promise.all(
    sources.map((source) =>
      cp(join(root, source), join(workspace, source), {
        recursive: true,
        // Prove that fresh checkouts work without copying an already generated runtime validator.
        filter: (path) => path !== join(root, "frontend/review/generated/validators.mjs"),
      }),
    ),
  );
  await run(
    "npm",
    ["ci", "--ignore-scripts", "--include=dev", "--include=optional", "--no-audit", "--no-fund"],
    workspace,
  );
  await run(process.execPath, ["scripts/review-contracts.mts", "--runtime"], workspace);
  const playwright = join(workspace, "node_modules/@playwright/test/cli.js");
  await run(process.execPath, [playwright, "test", "--config", "playwright.pure.config.ts"], workspace);
  await run(
    process.execPath,
    [
      playwright,
      "test",
      "review.spec.ts",
      "--project=chromium-debug",
      "--project=webkit-debug",
      "--update-snapshots=all",
    ],
    workspace,
  );
  await run(process.execPath, [playwright, "test", ...args, "--update-snapshots=none"], workspace);
}

/** Expose only ignored target output as writable to the pinned browser container. */
async function checkInPinnedContainer(args: readonly string[]): Promise<void> {
  const artifacts = join(root, "target/review-ci");
  await mkdir(artifacts, { recursive: true });
  console.info(`Container workspaces and artifacts remain under ${artifacts}`);
  await run(
    process.env["CONTAINER_ENGINE"] || "docker",
    [
      "run",
      "--rm",
      "--ipc=host",
      "--env",
      "CI=1",
      "--env",
      "REVIEW_VISUAL=1",
      "--volume",
      `${root}:/work:ro`,
      "--volume",
      `${artifacts}:/work/target:rw`,
      "--workdir",
      "/work",
      "mcr.microsoft.com/playwright:v1.63.0-noble",
      "node",
      "scripts/review-visual.mts",
      "--inside-container",
      ...args,
    ],
    root,
  );
}

try {
  requireSupportedNode();
  const args = process.argv.slice(2);
  if (args[0] === "--inside-container") await checkInsideContainer(args.slice(1));
  else await checkInPinnedContainer(args);
} catch (error) {
  console.error(`Visual checks failed: ${errorMessage(error)}`);
  if (isMissingFile(error)) console.error("Install Docker or set CONTAINER_ENGINE=podman.");
  process.exitCode = 1;
}
