/** Fail closed on new supported frontend releases, independently of deterministic Cargo builds and npm installs. */
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compare, major, prerelease, valid } from "semver";
import { errorMessage, isRecord, readJson, requireSupportedNode } from "./tooling.mts";

/** Each npm alias is checked against its real registry package and deliberately selected release family. */
export interface VersionPolicy {
  key: string;
  registryName: string;
  releaseCandidates: boolean;
  major?: number;
}

/** Compiler implementation and compatibility wrapper advance independently; neither is compared to native major 7. */
export const versionPolicies: readonly VersionPolicy[] = [
  { key: "preact", registryName: "preact", releaseCandidates: true },
  { key: "@preact/signals", registryName: "@preact/signals", releaseCandidates: false },
  { key: "@typescript/native", registryName: "typescript", releaseCandidates: false },
  { key: "typescript", registryName: "@typescript/typescript6", releaseCandidates: false, major: 6 },
  { key: "@typescript/old", registryName: "typescript", releaseCandidates: false, major: 6 },
];

/** Validated registry metadata exposes no executable content or arbitrary object coercions. */
export interface PublishedVersion {
  version: string;
  deprecated: boolean;
}

/** Enforce immutable root specifications, lock agreement, and integrity before consulting changing registry state. */
export function installedPins(manifest: unknown, lock: unknown): ReadonlyMap<string, string> {
  if (!isRecord(manifest) || !isRecord(lock) || !isRecord(lock["packages"])) {
    throw new Error("Invalid frontend manifests; regenerate package-lock.json with npm install");
  }
  const packages = lock["packages"];
  const lockRoot = packages[""];
  if (!isRecord(lockRoot)) throw new Error("The lockfile is missing its root package");
  const dependencies = {
    ...(isRecord(manifest["dependencies"]) ? manifest["dependencies"] : {}),
    ...(isRecord(manifest["devDependencies"]) ? manifest["devDependencies"] : {}),
  };
  const lockedDependencies = {
    ...(isRecord(lockRoot["dependencies"]) ? lockRoot["dependencies"] : {}),
    ...(isRecord(lockRoot["devDependencies"]) ? lockRoot["devDependencies"] : {}),
  };
  const pins = new Map<string, string>();
  for (const policy of versionPolicies) {
    const specification = dependencies[policy.key];
    const prefix = policy.key === policy.registryName ? "" : `npm:${policy.registryName}@`;
    if (typeof specification !== "string" || !specification.startsWith(prefix)) {
      throw new Error(`Missing exact ${policy.key} dependency (${policy.registryName})`);
    }
    const version = specification.slice(prefix.length);
    if (valid(version) !== version || (policy.major !== undefined && major(version) !== policy.major)) {
      throw new Error(`${policy.key} must pin an exact supported version, not a range or moving tag`);
    }
    const preview = prerelease(version);
    if (preview !== null && (!policy.releaseCandidates || preview[0] !== "rc")) {
      throw new Error(`${policy.key} is pinned to an unsupported prerelease channel`);
    }
    const entry = packages[`node_modules/${policy.key}`];
    if (
      lockedDependencies[policy.key] !== specification ||
      !isRecord(entry) ||
      entry["version"] !== version ||
      (prefix !== "" && entry["name"] !== policy.registryName) ||
      typeof entry["integrity"] !== "string" ||
      !entry["integrity"].startsWith("sha512-")
    ) {
      throw new Error(`${policy.key} manifest and integrity-locked package disagree; run npm install`);
    }
    pins.set(policy.key, version);
  }
  const nestedClassic = packages["node_modules/typescript/node_modules/@typescript/old"];
  if (
    nestedClassic !== undefined &&
    (!isRecord(nestedClassic) ||
      nestedClassic["version"] !== pins.get("@typescript/old") ||
      nestedClassic["name"] !== "typescript")
  ) {
    throw new Error(
      "The compatibility wrapper resolves a different TypeScript 6 implementation than its exact root pin",
    );
  }
  return pins;
}

/** Inspect published versions, not mutable dist-tags, so an RC always upgrades to its eventual stable release. */
export function publishedVersions(metadata: unknown, registryName: string): readonly PublishedVersion[] {
  if (!isRecord(metadata) || metadata["name"] !== registryName || !isRecord(metadata["versions"])) {
    throw new Error(`Malformed registry metadata for ${registryName}`);
  }
  return Object.entries(metadata["versions"]).map(([version, details]): PublishedVersion => {
    if (valid(version) !== version || !isRecord(details) || details["version"] !== version) {
      throw new Error(`Malformed published version for ${registryName}`);
    }
    return { version, deprecated: typeof details["deprecated"] === "string" && details["deprecated"] !== "" };
  });
}

/** Ignore betas/nightlies, but consider every eligible major and do not let a stale latest tag hide newer RCs. */
export function newestEligible(versions: readonly PublishedVersion[], policy: VersionPolicy): string {
  const candidates = versions
    .filter(({ version, deprecated }) => {
      const preview = prerelease(version);
      return (
        !deprecated &&
        (policy.major === undefined || major(version) === policy.major) &&
        (preview === null || (policy.releaseCandidates && preview[0] === "rc"))
      );
    })
    .map(({ version }) => version)
    .sort(compare);
  const newest = candidates.at(-1);
  if (newest === undefined) throw new Error(`No supported releases found for ${policy.registryName}`);
  return newest;
}

/** Use a fixed public registry, two bounded attempts, and one overall deadline; an outage is never treated as fresh. */
export async function fetchVersions(
  registryName: string,
  fetcher: typeof fetch = fetch,
  timeoutMilliseconds = 12_000,
): Promise<readonly PublishedVersion[]> {
  let failure: unknown;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      const response = await fetcher(`https://registry.npmjs.org/${encodeURIComponent(registryName)}`, {
        headers: { Accept: "application/vnd.npm.install-v1+json" },
        signal: AbortSignal.timeout(timeoutMilliseconds),
      });
      if (!response.ok) throw new Error(`Registry returned HTTP ${response.status} for ${registryName}`);
      const metadata: unknown = await response.json();
      return publishedVersions(metadata, registryName);
    } catch (error) {
      failure = error;
    }
  }
  throw new Error(
    `Cannot verify ${registryName} freshness: ${errorMessage(failure)}; retry when the registry is available`,
  );
}

/** Check all policy families in parallel while sharing the real TypeScript registry response across aliases. */
export async function checkVersions(directory: string): Promise<boolean> {
  const pins = installedPins(
    await readJson(resolve(directory, "package.json")),
    await readJson(resolve(directory, "package-lock.json")),
  );
  const registries = new Map<string, Promise<readonly PublishedVersion[]>>();
  for (const { registryName } of versionPolicies) {
    if (!registries.has(registryName)) registries.set(registryName, fetchVersions(registryName));
  }
  const results = await Promise.all(
    versionPolicies.map(async (policy): Promise<boolean> => {
      const installed = pins.get(policy.key);
      const lookup = registries.get(policy.registryName);
      if (installed === undefined || lookup === undefined) throw new Error(`Missing version policy for ${policy.key}`);
      const newest = newestEligible(await lookup, policy);
      const outdated = compare(newest, installed) > 0;
      console.log(`${policy.key}: ${installed}; newest eligible ${newest}${outdated ? " (update required)" : ""}`);
      return outdated;
    }),
  );
  return results.every((outdated) => !outdated);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  requireSupportedNode();
  checkVersions(resolve(dirname(fileURLToPath(import.meta.url)), "..")).then(
    (fresh) => {
      process.exitCode = fresh ? 0 : 1;
    },
    (error: unknown) => {
      console.error(errorMessage(error));
      process.exitCode = 2;
    },
  );
}
