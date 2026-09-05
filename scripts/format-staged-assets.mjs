// Automatically format only staged frontend paths before committing. Keep the
// index intact so formatting cannot silently stage unrelated or partial edits.
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);

/** Run Git or Prettier directly, without shell expansion of staged filenames. */
function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", windowsHide: true });
  if (result.error || result.status !== 0) {
    throw new Error(result.error?.message ?? result.stderr?.trim() ?? `${command} failed`);
  }
  return result.stdout;
}

/** Match source assets and configuration, excluding bundled vendor libraries. */
function isFrontendSource(path) {
  if (path.includes("/vendor/")) return false;
  return (
    /^assets\/.*\.(?:html|css|js)$/.test(path) ||
    /^frontend\/.*\.(?:ts|tsx|mjs|json)$/.test(path) ||
    /^scripts\/[^/]+\.mjs$/.test(path) ||
    /^(?:[^/]*config[^/]*\.(?:json|mjs|ts)|\.prettierrc\.json|package\.json)$/.test(path)
  );
}

/** Format working copies of staged files and report which require re-staging. */
export async function formatStagedAssets(cwd) {
  const root = run("git", ["rev-parse", "--show-toplevel"], cwd).trim();
  const paths = run("git", ["diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"], root)
    .split("\0")
    .filter(isFrontendSource);
  if (paths.length === 0) return [];
  const original = await Promise.all(paths.map((path) => readFile(join(root, path))));
  const prettierCli = join(dirname(require.resolve("prettier/package.json")), "bin/prettier.cjs");
  process.stdout.write(run(process.execPath, [prettierCli, "--write", "--", ...paths], root));
  const prettier = require("prettier");
  const changed = [];
  for (let index = 0; index < paths.length; index += 1) {
    const file = join(root, paths[index]);
    const staged = run("git", ["show", `:${paths[index]}`], root);
    const options = { ...(await prettier.resolveConfig(file)), filepath: file };
    // Check the index too: retrying a failed hook without re-staging must not
    // commit the unformatted snapshot merely because the working copy is clean.
    if (!original[index].equals(await readFile(file)) || !(await prettier.check(staged, options))) {
      changed.push(paths[index]);
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
    console.error(`Frontend formatting failed: ${error.message}`);
    process.exitCode = 1;
  }
}
