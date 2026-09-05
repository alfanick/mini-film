// Exercise schema generation independently of application DTO size, protecting reproducibility and real property names.
import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { generateContracts } from "./review-contracts.mjs";

test("generation preserves annotation-like fields and emits byte-stable contracts", async (t) => {
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
      { name: "item", method: "POST", path: "api/item", request: "item", response: "item", allow_empty_request: false },
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
  const { validateResponseItem } = await import(pathToFileURL(join(first, "validators.mjs")).href);
  assert.equal(validateResponseItem({ title: "Title", description: "Text", default: 1 }), true);
  assert.equal(validateResponseItem({}), false);
  assert.equal(validateResponseItem({ title: 12, description: "Text", default: 1 }), false);
  assert.equal(validateResponseItem({ title: "Title", description: "Text", default: -1 }), false);
  assert.doesNotMatch(await readFile(join(first, "responses.ts"), "utf8"), /\bany\b/);
});
