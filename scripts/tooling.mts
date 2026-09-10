/** Shared typed filesystem and subprocess boundaries for Node's erasable-TypeScript build tools. */
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";

/** Narrow parsed objects without trusting JSON.parse's untyped return value. */
export function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** Preserve actionable errors without invoking arbitrary object stringification. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : typeof error === "string" ? error : "Unknown failure";
}

/** Recognize missing files while allowing permission and I/O failures to propagate. */
export function isMissingFile(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

/** Read JSON through an unknown boundary so every consumer must validate its shape. */
export async function readJson(path: string): Promise<unknown> {
  const value: unknown = JSON.parse(await readFile(path, "utf8"));
  return value;
}

/** Follow a package's public executable metadata, including the native compiler's npm alias. */
export function packageBinary(packageName: string, executable: string, cwd: string): string {
  const require = createRequire(join(cwd, "package.json"));
  const manifestPath = require.resolve(`${packageName}/package.json`);
  const manifest: unknown = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (!isRecord(manifest)) throw new Error(`Invalid ${packageName} manifest`);
  const bin = manifest["bin"];
  const relative = typeof bin === "string" ? bin : isRecord(bin) ? bin[executable] : undefined;
  if (typeof relative !== "string") throw new Error(`${packageName} does not expose ${executable}`);
  return join(dirname(manifestPath), relative);
}

/** Require a fixture or build input that must exist before continuing. */
export function required<T>(value: T | undefined, description = "Required build input"): T {
  if (value === undefined) throw new Error(`${description} is missing`);
  return value;
}

/** Run a tool without shell interpolation, forwarding diagnostics unless output is requested. */
export function run(command: string, args: readonly string[], cwd: string, capture = false): string {
  const result = spawnSync(command, args, {
    cwd,
    stdio: capture ? ["ignore", "pipe", "pipe"] : "inherit",
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.error || result.status !== 0) {
    const detail = result.error?.message ?? result.stderr?.trim() ?? `exit ${String(result.status)}`;
    throw new Error(`${command} ${args.join(" ")} failed: ${detail}`);
  }
  return result.stdout ?? "";
}

/** Resolve npm's JavaScript entry point on Windows as well as Unix installations. */
export function npm(args: readonly string[], cwd: string, capture = false): string {
  const candidates = [
    process.env["npm_execpath"],
    join(dirname(process.execPath), "node_modules/npm/bin/npm-cli.js"),
    join(dirname(process.execPath), "../lib/node_modules/npm/bin/npm-cli.js"),
  ];
  const cli = candidates.find((path) => path !== undefined && existsSync(path));
  if (cli) return run(process.execPath, [cli, ...args], cwd, capture);
  if (process.platform === "win32") {
    return run(process.env["ComSpec"] ?? "cmd.exe", ["/d", "/s", "/c", "npm", ...args], cwd, capture);
  }
  return run("npm", args, cwd, capture);
}

/** Enforce the Node version whose stable type stripping runs these tools without generated bootstrap files. */
export function requireSupportedNode(): void {
  const [major = 0, minor = 0] = process.versions.node.split(".").map(Number);
  if (major < 24 || (major === 24 && minor < 12)) throw new Error("Review tooling requires Node.js 24.12 or newer");
}
