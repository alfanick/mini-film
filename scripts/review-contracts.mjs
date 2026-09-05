// Generate editor and build-time contracts from Rust-owned wire schemas. Cargo
// supplies fresh schemas; standalone frontend builds use the checked-in mirror.
import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const contractInputs = ["requests.schema.json", "responses.schema.json", "operations.json", "fixtures.json"];
const banner = "/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */";

/** Keep generated identifiers stable, including endpoint names containing separators. */
function identifier(value) {
  return value.replace(/(^|[_-])([a-z])/g, (_match, _separator, letter) => letter.toUpperCase());
}

/** Required lists already describe optionality; defaults otherwise create redundant enum/string intersections. */
function declarationSchema(value) {
  return withoutAnnotations(value, new Set(["default"]));
}

/** Keep documentation in editor schemas/types, not in runtime tables used only for validation constraints. */
function validationSchema(value) {
  return withoutAnnotations(value, new Set(["title", "description", "default", "examples", "$comment"]));
}

/** Traverse schema positions, preserving properties literally named title, description, default, or examples. */
function withoutAnnotations(value, annotations) {
  if (Array.isArray(value)) return value.map((child) => withoutAnnotations(child, annotations));
  if (value === null || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value)
      .filter(([key]) => !annotations.has(key))
      .map(([key, child]) => {
        if (
          ["properties", "definitions", "$defs", "patternProperties", "dependencies", "dependentSchemas"].includes(key)
        ) {
          return [
            key,
            Object.fromEntries(
              Object.entries(child).map(([name, schema]) => [name, withoutAnnotations(schema, annotations)]),
            ),
          ];
        }
        if (key === "enum" || key === "const") return [key, child];
        return [key, withoutAnnotations(child, annotations)];
      }),
  );
}

/** Generate TypeScript, named operations and ahead-of-time validators without runtime schema loading. */
export async function generateContracts({ schemaDir, outputDir, dependenciesDir = root, runtimeOnly = false }) {
  const require = createRequire(join(dependenciesDir, "package.json"));
  const { compile } = require("json-schema-to-typescript");
  const Ajv = require("ajv");
  const standalone = require("ajv/dist/standalone").default;
  const prettier = require("prettier");
  const inputs = await Promise.all(contractInputs.map((file) => readFile(join(schemaDir, file), "utf8")));
  const [requests, responses, operations] = inputs.map((input) => JSON.parse(input));
  const catalogs = { requests, responses };
  const ajv = new Ajv({
    strict: true,
    strictRequired: false,
    allErrors: false,
    // Inline tiny primitive references, but share larger image/profile validators
    // across HTTP, SSE, full-state and patch decoders to keep the bundle bounded.
    inlineRefs: 3,
    loopRequired: 5,
    messages: false,
    coerceTypes: false,
    useDefaults: false,
    removeAdditional: false,
    code: { source: true, esm: true, optimize: 2 },
  });
  // Schemars formats annotate Rust numeric storage, while JSON constraints still
  // enforce integer/range validity. They are not string-format validators.
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
  ]) {
    ajv.addFormat(format, true);
  }
  const exports = {};
  const declarations = [
    banner,
    'import type { RequestContracts } from "./requests";',
    'import type { ResponseContracts } from "./responses";',
  ];
  const generated = new Map();
  for (const [direction, schema] of Object.entries(catalogs)) {
    const catalogName = direction === "requests" ? "RequestContracts" : "ResponseContracts";
    const base = `https://mini-film.invalid/contracts/${direction}`;
    ajv.addSchema({ ...validationSchema(schema), $id: base }, base);
    generated.set(
      `${direction}.ts`,
      await compile(declarationSchema(schema), catalogName, {
        bannerComment: banner,
        unknownAny: true,
        additionalProperties: false,
        strictIndexSignatures: true,
        enableConstEnums: false,
        style: { printWidth: 120, trailingComma: "all" },
      }),
    );
    for (const name of Object.keys(schema.properties ?? {}).sort()) {
      const validator = `validate${direction === "requests" ? "Request" : "Response"}${identifier(name)}`;
      exports[validator] = `${base}#/properties/${name}`;
      declarations.push(
        `/** Validate the ${name} ${direction} boundary before consuming unknown JSON. */`,
        `export function ${validator}(value: unknown): value is ${catalogName}[${JSON.stringify(name)}];`,
      );
    }
  }
  generated.set("validators.mjs", `${banner}\n${standalone(ajv, exports)}\n`);
  generated.set("validators.d.mts", declarations.join("\n"));
  const operationLines = [
    banner,
    'import type { RequestContracts } from "./requests";',
    'import type { ResponseContracts } from "./responses";',
    'import * as validators from "./validators.mjs";',
    "/** Endpoint bindings originate in Rust, preventing caller-selected response assertions. */",
    "export interface OperationContracts {",
  ];
  for (const operation of operations) {
    const request = operation.request === null ? "undefined" : `RequestContracts[${JSON.stringify(operation.request)}]`;
    operationLines.push(
      `${JSON.stringify(operation.name)}: { request: ${request};`,
      `response: ResponseContracts[${JSON.stringify(operation.response)}] };`,
    );
  }
  operationLines.push(
    "}",
    "/** Concrete decoders and route templates for every supported JSON operation. */",
    "export const operations = {",
  );
  for (const operation of operations) {
    operationLines.push(
      `${JSON.stringify(operation.name)}: {`,
      `method: ${JSON.stringify(operation.method)}, path: ${JSON.stringify(operation.path)},`,
      `allowEmptyRequest: ${Boolean(operation.allow_empty_request)},`,
      `decode: validators.validateResponse${identifier(operation.response)},`,
      `hasRequest: ${operation.request !== null},`,
      "},",
    );
  }
  operationLines.push("} as const;");
  generated.set("operations.ts", operationLines.join("\n"));
  const client = [
    banner,
    'import { createOperation, type OperationOptions } from "../core/transport";',
    'import { operations, type OperationContracts } from "./operations";',
    "/** Named API methods prevent callers from inventing response types or forgetting route identities. */",
    "export const reviewApi = {",
  ];
  for (const operation of operations.filter((operation) => operation.transport !== "sse")) {
    const key = JSON.stringify(operation.name);
    const parameters = [...operation.path.matchAll(/\{([a-z_]+)\}/g)]
      .map((match) => `${match[1]}: string | number`)
      .join("; ");
    const request = `OperationContracts[${key}]["request"]${operation.allow_empty_request ? " | undefined" : ""}`;
    const response = `OperationContracts[${key}]["response"]`;
    const params = parameters ? `{ ${parameters} }` : "Record<never, never>";
    // The variable annotation documents each public function as well as its implementation's generic binding.
    client.push(`${key}: createOperation<${request}, ${response}, ${params}>(operations[${key}]),`);
  }
  client.push(
    "} as const;",
    "/** Derive exact call options for helper functions composing an existing operation. */",
    "export type ReviewOperationOptions<Request, Params> = OperationOptions<Request, Params>;",
  );
  generated.set("client.ts", client.join("\n"));
  for (let index = 0; index < contractInputs.length; index += 1) generated.set(contractInputs[index], inputs[index]);
  await mkdir(outputDir, { recursive: true });
  const outputs = [...generated].filter(([file]) => !runtimeOnly || file === "validators.mjs");
  for (const [file, source] of outputs) {
    // Standalone validator JS is machine output, not hand-maintained application
    // code; its declaration and the consuming TS remain fully type-checked.
    const contents = await prettier.format(source, {
      parser: file.endsWith(".json") ? "json" : file.endsWith(".mjs") ? "babel" : "typescript",
      printWidth: 120,
      trailingComma: "all",
    });
    await writeFile(join(outputDir, file), contents);
  }
  return outputs.map(([file]) => file).sort();
}

/** Regenerate ignored runtime output, or export Rust schemas to update/verify the tracked editor mirror. */
async function main(mode) {
  const mirror = join(root, "frontend/review/generated");
  if (mode === "--runtime") {
    await generateContracts({ schemaDir: mirror, outputDir: mirror, runtimeOnly: true });
    return;
  }
  if (mode !== "--generate" && mode !== "--check") throw new Error("Expected --generate, --check, or --runtime");
  const temporary = await mkdtemp(join(tmpdir(), "mini-film-contracts-"));
  try {
    const schemas = join(temporary, "schemas");
    const exported = spawnSync(
      "cargo",
      ["run", "--locked", "-p", "review-contract-export", "--", "--out-dir", schemas],
      {
        cwd: root,
        stdio: "inherit",
        windowsHide: true,
      },
    );
    if (exported.error || exported.status !== 0) throw new Error("Rust review contract export failed");
    const destination = mode === "--generate" ? mirror : join(temporary, "generated");
    const files = await generateContracts({ schemaDir: schemas, outputDir: destination });
    if (mode === "--check") {
      const trackedFiles = files.filter((file) => file !== "validators.mjs");
      assert.deepEqual(
        (await readdir(mirror)).filter((file) => file !== "validators.mjs").sort(),
        trackedFiles,
        "Generated contract file list is stale",
      );
      for (const file of trackedFiles) {
        assert.equal(
          await readFile(join(mirror, file), "utf8"),
          await readFile(join(destination, file), "utf8"),
          `${file} is stale; run npm run contracts:generate`,
        );
      }
    }
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv[2]).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
