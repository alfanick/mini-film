// Exercise schema generation independently of application DTO size, protecting reproducibility and real property names.
import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import ts from "typescript";
import { compactValidationErrors, generateContracts, parseOperations } from "./review-contracts.mts";
import { isRecord } from "./tooling.mts";

void test("generation preserves annotation-like fields and emits byte-stable contracts", async (t) => {
  const temporary = await mkdtemp(join(tmpdir(), "mini-film-schema-test-"));
  t.after(() => rm(temporary, { recursive: true, force: true }));
  const schema = {
    $schema: "http://json-schema.org/draft-07/schema#",
    type: "object",
    properties: {
      item: {
        type: "object",
        required: ["title", "description", "default"],
        properties: { title: { type: "string" }, description: { type: "string" }, default: { type: "integer" } },
        dependencies: { title: { properties: { default: { type: "integer", minimum: 0 } } } },
      },
    },
    required: ["item"],
  };
  for (const direction of ["requests", "responses"]) {
    await writeFile(join(temporary, `${direction}.schema.json`), JSON.stringify(schema));
  }
  await writeFile(
    join(temporary, "operations.json"),
    JSON.stringify([
      {
        name: "item",
        method: "POST",
        path: "api/item",
        request: "item",
        response: "item",
        allow_empty_request: false,
        transport: "http",
        parameters: [],
      },
    ]),
  );
  await writeFile(join(temporary, "fixtures.json"), "{}\n");
  const first = join(temporary, "first");
  const second = join(temporary, "second");
  const files = await generateContracts({ schemaDir: temporary, outputDir: first });
  assert.deepEqual(await generateContracts({ schemaDir: temporary, outputDir: second }), files);
  for (const file of files)
    assert.equal(await readFile(join(first, file), "utf8"), await readFile(join(second, file), "utf8"));
  const runtime = join(temporary, "runtime");
  const outputs = await generateContracts({ schemaDir: temporary, outputDir: runtime, runtimeOnly: true });
  assert.deepEqual(outputs, ["validators.mjs"]);
  assert.deepEqual(await readdir(runtime), outputs);
  assert.equal(
    await readFile(join(runtime, "validators.mjs"), "utf8"),
    await readFile(join(first, "validators.mjs"), "utf8"),
  );
  const validators: unknown = await import(pathToFileURL(join(first, "validators.mjs")).href);
  assert.ok(isRecord(validators));
  const candidate = validators["validateResponseItem"];
  assert.ok(isCallable(candidate));
  const validateResponseItem = candidate;
  assert.equal(validateResponseItem({ title: "Title", description: "Text", default: 1 }), true);
  assert.equal(validateResponseItem({}), false);
  assert.equal(validateResponseItem({ title: 12, description: "Text", default: 1 }), false);
  assert.equal(validateResponseItem({ title: "Title", description: "Text", default: -1 }), false);
  assert.doesNotMatch(await readFile(join(first, "responses.ts"), "utf8"), /\bany\b/);
});

/** Route metadata must describe every placeholder exactly once and preserve each Rust identity type. */
void test("operation metadata rejects missing, duplicate, or extra path parameters", () => {
  const operation = {
    name: "item",
    method: "POST",
    path: "api/item/{job_id}",
    request: "item",
    response: "item",
    allow_empty_request: false,
    transport: "http",
    parameters: [{ name: "job_id", kind: "number" }],
  };
  assert.equal(parseOperations([operation])[0]?.parameters[0]?.kind, "number");
  for (const parameters of [
    [],
    [{ name: "wrong", kind: "number" }],
    [...operation.parameters, ...operation.parameters],
  ]) {
    assert.throws(() => parseOperations([{ ...operation, parameters }]), /parameters do not match/);
  }
  assert.throws(() => parseOperations([operation, operation]), /Duplicate operation/);
});

/** Generated modules are untrusted dynamic imports until their callable export is checked. */
function isCallable(value: unknown): value is (input: unknown) => unknown {
  return typeof value === "function";
}

/** The compaction adapter must never interpret schema-like user objects as generated validation failures. */
void test("error compaction preserves schema values and rejects an unreviewed Ajv output shape", () => {
  const payload = '{instancePath:"",schemaPath:"#/type",keyword:"type",params:{type:"string"}}';
  const source = [
    `const schema = {const:${payload}};`,
    `const err99 = ${payload};`,
    "export function validate1(data) {",
    `if(typeof data !== "string") {validate1.errors=[${payload}];return false;}`,
    `const err0=${payload}; validate1.errors=[err0]; return true;`,
    "}",
  ].join("\n");
  const compacted = compactValidationErrors(source, ts);
  assert.match(compacted, /validate1\.errors = \[\{\}\]/);
  assert.match(compacted, /const err0 = \{\}/);
  assert.match(compacted, /const err99 = \{ instancePath:/);
  assert.match(compacted, /const schema = \{ const: \{ instancePath:/);
  assert.throws(
    () =>
      compactValidationErrors(
        source.replaceAll('params:{type:"string"}', 'params:{type:"string"},message:"new field"'),
        ts,
      ),
    /Unexpected pinned Ajv error shape/,
  );
  assert.throws(
    () => compactValidationErrors(source.replaceAll("validate1", "newGenerator"), ts),
    /no recognized error payloads/,
  );
});
