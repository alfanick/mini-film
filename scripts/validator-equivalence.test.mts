/** Compare compacted guards with original Ajv across actual Rust contracts, including nested malformed inputs. */
import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { Ajv } from "ajv";
import { generateContracts } from "./review-contracts.mts";
import { isRecord, readJson, required } from "./tooling.mts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const schemaDir = join(root, "frontend/review/generated");
type ValuePath = readonly (string | number)[];

/** Runtime imports must prove a callable export before arbitrary fixture values enter generated code. */
function isCallable(value: unknown): value is (input: unknown) => unknown {
  return typeof value === "function";
}

/** Avoid Array.isArray's mutable untyped element inference while inspecting untrusted JSON recursively. */
function isArray(value: unknown): value is unknown[] {
  return Array.isArray(value);
}

/** Enumerate every represented property and array position rather than mutating only top-level wrappers. */
function* locations(value: unknown, path: ValuePath = []): Generator<{ path: ValuePath; value: unknown }> {
  yield { path, value };
  if (isArray(value)) {
    for (let index = 0; index < value.length; index += 1) yield* locations(value[index], [...path, index]);
  } else if (isRecord(value)) {
    for (const key of Object.keys(value).sort()) yield* locations(value[key], [...path, key]);
  }
}

/** Build independent nested replacements without changing the fixture or carrying aliases between cases. */
function replaceAt(value: unknown, path: ValuePath, replacement: unknown, remove: boolean = false): unknown {
  if (path.length === 0) return replacement;
  const [head, ...rest] = path;
  if (isArray(value) && typeof head === "number") {
    if (remove && rest.length === 0) return value.filter((_item, index) => index !== head);
    return value.map((item, index) => (index === head ? replaceAt(item, rest, replacement, remove) : item));
  }
  if (isRecord(value) && typeof head === "string") {
    if (remove && rest.length === 0) return Object.fromEntries(Object.entries(value).filter(([key]) => key !== head));
    return { ...value, [head]: replaceAt(value[head], rest, replacement, remove) };
  }
  throw new Error(`Invalid mutation path: ${path.join("/")}`);
}

/** Cover JSON kinds, Rust integer boundaries, nonfinite numbers, literals, and malformed collection contents. */
const replacements: readonly unknown[] = [
  undefined,
  null,
  false,
  true,
  0,
  -1,
  1,
  0.5,
  255,
  256,
  65535,
  65536,
  4294967295,
  4294967296,
  Number.NaN,
  Number.POSITIVE_INFINITY,
  "",
  "unknown",
  "none",
  "done",
  "current",
  [],
  [null],
  [0],
  {},
  { type: "patch" },
];

/** Each case changes one exact location, so valid sibling values cannot hide a missing nested validator. */
function* mutations(seed: unknown): Generator<{ label: string; value: unknown }> {
  yield { label: "original", value: seed };
  for (const location of locations(seed)) {
    const path = location.path.join("/") || "<root>";
    for (const [index, replacement] of replacements.entries()) {
      yield { label: `${path}:replacement-${index}`, value: replaceAt(seed, location.path, replacement) };
    }
    if (location.path.length) {
      yield { label: `${path}:removed`, value: replaceAt(seed, location.path, undefined, true) };
    }
    if (isRecord(location.value)) {
      yield {
        label: `${path}:additional-property`,
        value: replaceAt(seed, location.path, { ...location.value, "x-contract-test": [null, 1, "extra"] }),
      };
    }
  }
}

/** Mirror only export spelling, independently deriving the complete guard list from both Rust schema catalogs. */
function guardName(direction: string, name: string): string {
  const suffix = name.replace(/(^|[_-])([a-z])/g, (_match: string, _separator: string, letter: string) =>
    letter.toUpperCase(),
  );
  return `validate${direction === "requests" ? "Request" : "Response"}${suffix}`;
}

/** Populate request-compatible examples while response fixtures remain directly serialized by Rust. */
function requestFixtures(fixtures: Record<string, unknown>): Record<string, unknown> {
  const state = fixtures["state"];
  assert.ok(isRecord(state));
  const images = state["images"];
  assert.ok(isArray(images));
  const image = images[0];
  assert.ok(isRecord(image));
  const diffusion = fixtures["diffusion_job"];
  assert.ok(isRecord(diffusion));
  const settings = diffusion["settings"];
  return {
    burst: { expanded: true },
    diffusion_apply: { image_id: 1, profile_index: 0, scope: "current", settings },
    diffusion_create: { image_id: 1, profile_index: 0, settings },
    diffusion_reset: { image_id: 1, profile_index: 0, scope: "all" },
    panorama_create: { image_ids: [1, 2], name: "Fixture", matching_mode: "automatic" },
    panorama_previews: { image_ids: [1, 2], matching_mode: "sequential" },
    panorama_render: { name: "Fixture", projection: "cylindrical" },
    panorama_update: { image_ids: [1, 2], name: "Fixture", matching_mode: "automatic", selected_projection: null },
    publish: {
      album: "Fixture",
      labels: ["red"],
      tags: ["007", "007"],
      min_rating: 2,
      main_profile_only: false,
      normalize_grain: true,
      normalize_grain_mpix: 24,
      long_edge: null,
      max_width: 1200,
      max_height: 800,
    },
    review: {
      image_id: 1,
      rating: 3,
      tags: ["007", "007"],
      label: "red",
      labels: ["red"],
      notes: "Manual",
      selected_profile_index: 0,
      enabled_profile_indexes: [0],
      publish_profile_indexes: [0],
      profile_bw_filters: [{ profile_index: 0, filter: "red" }],
      retouch: image["retouch"],
    },
    sampler_create: { image_id: 1 },
    sampler_priority: { visible_keys: ["film"], expanded_keys: ["family"] },
    sampler_select: { enabled: true, scope: "current" },
    ui: { current_image_id: 1, min_rating: 2, labels: ["red"] },
  };
}

void test("compacted validators preserve every Rust contract boolean and never mutate inputs", async (context) => {
  const temporary = await mkdtemp(join(tmpdir(), "mini-film-validator-equivalence-"));
  context.after(() => rm(temporary, { recursive: true, force: true }));
  await generateContracts({ schemaDir, outputDir: temporary, runtimeOnly: true });
  const module: unknown = await import(pathToFileURL(join(temporary, "validators.mjs")).href);
  assert.ok(isRecord(module));
  const fixtures = await readJson(join(schemaDir, "fixtures.json"));
  assert.ok(isRecord(fixtures));
  const requestValues = requestFixtures(fixtures);
  // Original Ajv retains its normal detailed errors; only boolean results are part of the browser guard contract.
  const reference = new Ajv({
    strict: true,
    strictRequired: false,
    coerceTypes: false,
    useDefaults: false,
    removeAdditional: false,
  });
  for (const format of [
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uint",
    "int8",
    "int16",
    "int32",
    "int64",
    "int",
    "float",
    "double",
  ])
    reference.addFormat(format, true);
  let comparisons = 0;
  let accepted = 0;
  let rejected = 0;
  const guards: string[] = [];
  for (const direction of ["requests", "responses"]) {
    const schema = await readJson(join(schemaDir, `${direction}.schema.json`));
    assert.ok(isRecord(schema));
    const properties = schema["properties"];
    assert.ok(isRecord(properties));
    const base = `https://mini-film.invalid/equivalence/${direction}`;
    reference.addSchema({ ...schema, $id: base }, base);
    for (const name of Object.keys(properties).sort()) {
      const exported = guardName(direction, name);
      guards.push(exported);
      const compacted: unknown = module[exported];
      assert.ok(isCallable(compacted), exported);
      const original = required(reference.getSchema<unknown>(`${base}#/properties/${name}`));
      const seeds: readonly unknown[] =
        direction === "requests"
          ? [requestValues[name]]
          : name === "message"
            ? [fixtures["state"], fixtures["patch"]]
            : [fixtures[name]];
      for (const seed of seeds) {
        assert.notEqual(seed, undefined, `Missing fixture for ${exported}`);
        assert.equal(original(seed), true, `Invalid seed for ${exported}: ${JSON.stringify(original.errors)}`);
        for (const candidate of mutations(seed)) {
          const value = structuredClone(candidate.value);
          const before = structuredClone(value);
          const expected: unknown = original(value);
          assert.equal(typeof expected, "boolean", `${exported} must be synchronous`);
          assert.deepEqual(value, before, `Original ${exported} mutated ${candidate.label}`);
          assert.equal(compacted(value), expected, `${exported} differs at ${candidate.label}`);
          assert.deepEqual(value, before, `Compacted ${exported} mutated ${candidate.label}`);
          comparisons += 1;
          if (expected === true) accepted += 1;
          else rejected += 1;
        }
      }
    }
  }
  assert.deepEqual(Object.keys(module).sort(), guards.sort(), "Every exported guard must be compared");
  assert.ok(comparisons > 10000, `Expected broad nested coverage, received ${comparisons} cases`);
  assert.ok(accepted > 100 && rejected > 1000, "Corpus must exercise both acceptance and rejection");
  context.diagnostic(`${guards.length} guards: ${comparisons} comparisons; ${accepted} accepted, ${rejected} rejected`);
});
