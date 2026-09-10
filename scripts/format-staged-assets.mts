// Automatically format only staged frontend paths before committing. Keep the
// index intact so formatting cannot silently stage unrelated or partial edits.
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { errorMessage, required, run } from "./tooling.mts";

const require = createRequire(import.meta.url);

/** Match source assets and configuration, excluding bundled vendor libraries. */
function isFrontendSource(path: string): boolean {
  if (path.includes("/vendor/")) return false;
  return (
    /^assets\/.*\.(?:html|css|js)$/.test(path) ||
    /^frontend\/.*\.(?:ts|tsx|mts|mjs|json)$/.test(path) ||
    /^scripts\/[^/]+\.(?:mjs|mts)$/.test(path) ||
    /^(?:[^/]*config[^/]*\.(?:json|mjs|ts)|\.prettierrc\.json|package\.json)$/.test(path)
  );
}

/** Format working copies of staged files and report which require re-staging. */
export async function formatStagedAssets(cwd: string): Promise<string[]> {
  const root = run("git", ["rev-parse", "--show-toplevel"], cwd, true).trim();
  const paths = run("git", ["diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"], root, true)
    .split("\0")
    .filter(isFrontendSource);
  if (paths.length === 0) return [];
  const original = await Promise.all(paths.map((path) => readFile(join(root, path))));
  const prettierCli = join(dirname(require.resolve("prettier/package.json")), "bin/prettier.cjs");
  process.stdout.write(run(process.execPath, [prettierCli, "--write", "--", ...paths], root, true));
  const prettier = require("prettier") as typeof import("prettier");
  const changed: string[] = [];
  for (const [index, path] of paths.entries()) {
    const file = join(root, path);
    const staged = run("git", ["show", `:${path}`], root, true);
    const options = { ...(await prettier.resolveConfig(file)), filepath: file };
    // Check the index too: retrying a failed hook without re-staging must not
    // commit the unformatted snapshot merely because the working copy is clean.
    if (!required(original[index]).equals(await readFile(file)) || !(await prettier.check(staged, options))) {
      changed.push(path);
    }
  }
  return changed;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const changed = await formatStagedAssets(process.cwd());
    if (changed.length > 0) {
      console.error("Frontend formatting differs from the Git index; review and re-stage these paths:");
      for (const path of changed) console.error(`  ${path}`);
      process.exitCode = 1;
    }
  } catch (error) {
    console.error(`Frontend formatting failed: ${errorMessage(error)}`);
    process.exitCode = 1;
  }
}
