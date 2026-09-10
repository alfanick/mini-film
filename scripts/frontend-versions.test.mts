/** Test the online freshness policy offline, including RC graduation, aliases, immutable pins, and registry outages. */
import assert from "node:assert/strict";
import test from "node:test";
import {
  fetchVersions,
  installedPins,
  newestEligible,
  publishedVersions,
  versionPolicies,
  type PublishedVersion,
  type VersionPolicy,
} from "./frontend-versions.mts";
import { required } from "./tooling.mts";

/** Keep registry fixtures minimal while retaining explicit deprecation state. */
function versions(...values: string[]): readonly PublishedVersion[] {
  return values.map((version) => ({ version, deprecated: false }));
}

/** Lookup a fixed policy without introducing non-null assertions in tests. */
function policy(key: string): VersionPolicy {
  return required(versionPolicies.find((entry) => entry.key === key));
}

void test("Preact checks all stable and RC majors, including RC-to-stable with stale dist-tags", () => {
  const preact = policy("preact");
  assert.equal(newestEligible(versions("10.29.8", "11.0.0-rc.2", "11.0.0-rc.3"), preact), "11.0.0-rc.3");
  assert.equal(newestEligible(versions("11.0.0-rc.2", "11.0.0"), preact), "11.0.0");
  assert.equal(newestEligible(versions("11.0.0", "12.0.0-rc.1", "13.0.0-beta.2"), preact), "12.0.0-rc.1");
  assert.equal(newestEligible(versions("11.0.0-rc.2", "12.0.0-dev.1"), preact), "11.0.0-rc.2");
  assert.equal(newestEligible([...versions("11.0.0"), { version: "12.0.0", deprecated: true }], preact), "11.0.0");
});

void test("compiler aliases, compatibility wrapper, and Signals use independent release families", () => {
  const compiler = versions("6.0.3", "6.1.0", "7.0.2", "8.0.0-rc.1");
  assert.equal(newestEligible(compiler, policy("@typescript/native")), "7.0.2");
  assert.equal(newestEligible(compiler, policy("@typescript/old")), "6.1.0");
  assert.equal(newestEligible(versions("6.0.2", "6.0.3", "7.0.0"), policy("typescript")), "6.0.3");
  assert.equal(newestEligible(versions("2.11.2", "3.0.0", "4.0.0-beta.1"), policy("@preact/signals")), "3.0.0");
});

/** Build matching root and lock manifests without accessing the developer's actual dependency files. */
function manifests(): {
  manifest: { dependencies: Record<string, string> };
  lock: { packages: Record<string, Record<string, unknown>> };
} {
  const dependencies: Record<string, string> = {};
  const packages: Record<string, Record<string, unknown>> = {};
  for (const entry of versionPolicies) {
    const version = entry.major === 6 ? "6.0.2" : "11.0.0";
    const aliased = entry.key !== entry.registryName;
    dependencies[entry.key] = aliased ? `npm:${entry.registryName}@${version}` : version;
    packages[`node_modules/${entry.key}`] = { version, name: entry.registryName, integrity: "sha512-fixture" };
  }
  packages[""] = { dependencies: { ...dependencies } };
  return { manifest: { dependencies }, lock: { packages } };
}

void test("freshness rejects moving pins, wrong aliases, missing integrity, and stale lock entries", () => {
  const good = manifests();
  assert.equal(installedPins(good.manifest, good.lock).size, 5);
  for (const pin of ["^11.0.0", "rc", "npm:other@11.0.0"]) {
    const fixture = manifests();
    fixture.manifest.dependencies["preact"] = pin;
    assert.throws(() => installedPins(fixture.manifest, fixture.lock), /exact supported version/);
  }
  const wrongAlias = manifests();
  wrongAlias.manifest.dependencies["@typescript/native"] = "npm:other@7.0.2";
  assert.throws(() => installedPins(wrongAlias.manifest, wrongAlias.lock), /Missing exact/);
  const preview = manifests();
  preview.manifest.dependencies["@typescript/native"] = "npm:typescript@8.0.0-rc.1";
  assert.throws(() => installedPins(preview.manifest, preview.lock), /unsupported prerelease/);
  const nested = manifests();
  nested.lock.packages["node_modules/typescript/node_modules/@typescript/old"] = {
    name: "typescript",
    version: "6.1.0",
  };
  assert.throws(() => installedPins(nested.manifest, nested.lock), /different TypeScript 6 implementation/);
  for (const field of ["integrity", "version"]) {
    const fixture = manifests();
    required(fixture.lock.packages["node_modules/preact"])[field] = "mismatch";
    assert.throws(() => installedPins(fixture.manifest, fixture.lock), /disagree/);
  }
});

void test("registry metadata is validated and outages fail closed after exactly two attempts", async () => {
  assert.throws(() => publishedVersions({ name: "wrong", versions: {} }, "preact"), /Malformed/);
  assert.throws(() => publishedVersions({ name: "preact", versions: { "11.0.0": {} } }, "preact"), /Malformed/);
  let attempts = 0;
  const offline: typeof fetch = (): Promise<Response> => {
    attempts += 1;
    return Promise.reject(new Error("fixture offline"));
  };
  await assert.rejects(fetchVersions("preact", offline, 10), /Cannot verify preact freshness.*fixture offline/);
  assert.equal(attempts, 2);
  const unavailable: typeof fetch = (): Promise<Response> => Promise.resolve(new Response("", { status: 503 }));
  await assert.rejects(fetchVersions("preact", unavailable, 10), /HTTP 503/);
  let canceledAttempts = 0;
  const stalled: typeof fetch = (_url, options): Promise<Response> => {
    const signal = options?.signal;
    assert.ok(signal);
    return new Promise((_resolve, reject): void => {
      signal.addEventListener(
        "abort",
        (): void => {
          canceledAttempts += 1;
          reject(new Error("fixture deadline"));
        },
        { once: true },
      );
    });
  };
  // Keep the fixture process alive while AbortSignal's intentionally unreferenced timer expires.
  const fixtureTimer = setTimeout(() => undefined, 1000);
  try {
    await assert.rejects(fetchVersions("preact", stalled, 5), /fixture deadline/);
    assert.equal(canceledAttempts, 2);
  } finally {
    clearTimeout(fixtureTimer);
  }
});
