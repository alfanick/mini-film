/** Exercise native compiler and editor diagnostics against the same generated API that Cargo embeds. */
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isRecord, packageBinary } from "./tooling.mts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Keep deliberately invalid source outside tracked paths while using the checked-in strict compiler configuration. */
async function fixtureDirectory(): Promise<string> {
  await mkdir(join(root, "target"), { recursive: true });
  const directory = await mkdtemp(join(root, "target/compiler-contract-"));
  await writeFile(
    join(directory, "tsconfig.json"),
    JSON.stringify({
      extends: "../../tsconfig.review.json",
      compilerOptions: { allowImportingTsExtensions: true },
      include: ["*.ts"],
      exclude: [],
    }),
  );
  return directory;
}

void test("native compiler preserves request, response, parameter, and test-recorder correlation", async (context) => {
  const directory = await fixtureDirectory();
  context.after(() => rm(directory, { recursive: true, force: true }));
  const imports = [
    'import {reviewApi} from "../../frontend/review/generated/client.ts";',
    'import type {RecordedOperation} from "../../frontend/review/generated/request-decoders.ts";',
    'import type {ReviewModelValue} from "../../frontend/review/core/model.ts";',
    'import type {PublishActions} from "../../frontend/review/features/publish/model.ts";',
    "export type Recorded = RecordedOperation;",
    "export declare const model: ReviewModelValue;",
    "export declare const publish: PublishActions;",
  ].join("\n");
  const validCalls = [
    "void reviewApi.state({});",
    "void reviewApi.sampler_get({params:{job_id:42}});",
    "void reviewApi.publish({keepalive:true});",
    "void reviewApi.panorama_create({body:{image_ids:[]}}).then(value => value.created_project_id.toFixed());",
    "void reviewApi.publish({}).then(value => value.created_job_id.toFixed());",
    "void model.catalog.value?.images.length; void publish.publishForm.value.labels.length;",
  ];
  const cases: readonly [string, boolean][] = [
    [validCalls.join("\n"), true],
    ['void reviewApi.sampler_get({params:{job_id:"not-a-number"}});', false],
    ['void reviewApi.state({params:{unused:"not-allowed"}});', false],
    ["void reviewApi.state({body:{arbitrary:true}});", false],
    ["void reviewApi.sampler_get({});", false],
    ['void reviewApi.publish({keepalive:"yes"});', false],
    ["void reviewApi.publish({}).then(value => value.created_project_id);", false],
    ['export const wrong:Recorded = {name:"sampler_get",body:{image_id:"image-a"}};', false],
    ["model.catalog.value?.images.push();", false],
    ["const image=model.image(1).value; if(image) image.retouch.adjustments.exposure=1;", false],
    ['model.field("labelFilters").value.add("red");', false],
    ["model.dirtyRetouchIds.value.add(1);", false],
    ['publish.publishForm.value.labels.push("red");', false],
  ];
  for (const [source, succeeds] of cases) {
    await writeFile(join(directory, "fixture.ts"), `${imports}\n${source}\n`);
    const result = spawnSync(
      process.execPath,
      [packageBinary("@typescript/native", "tsc", root), "--project", directory],
      { encoding: "utf8", cwd: root },
    );
    assert.equal(result.error, undefined);
    assert.equal(result.status === 0, succeeds, `${source}\n${result.stdout}\n${result.stderr}`);
    if (!succeeds) assert.match(result.stdout, /fixture\.ts\(\d+,\d+\): error TS\d+/);
  }
});

/** Pending responses carry unknown JSON until the individual assertion validates its protocol shape. */
interface PendingResponse {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
}

void test("native LSP reports the same assignment diagnostic used by compiler and Neovim", async (context) => {
  const directory = await fixtureDirectory();
  context.after(() => rm(directory, { recursive: true, force: true }));
  const filename = join(directory, "fixture.ts");
  const source = 'export const answer: number = "not a number";\n';
  await writeFile(filename, source);
  const child = spawn(process.execPath, [packageBinary("@typescript/native", "tsc", root), "--lsp", "--stdio"], {
    cwd: directory,
    stdio: ["pipe", "pipe", "pipe"],
  });
  context.after(() => {
    child.kill();
  });
  const pending = new Map<number, PendingResponse>();
  let buffer = Buffer.alloc(0);
  let nextId = 0;
  let stderr = "";
  const received: unknown[] = [];
  child.stderr.on("data", (chunk: Buffer): void => {
    stderr += chunk.toString();
  });
  child.on("exit", (): void => {
    for (const response of pending.values()) response.reject(new Error(`Native LSP exited: ${stderr}`));
  });
  child.stdout.on("data", (chunk: Buffer): void => {
    buffer = Buffer.concat([buffer, chunk]);
    while (true) {
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const length = Number(/Content-Length: (\d+)/i.exec(buffer.subarray(0, headerEnd).toString())?.[1]);
      if (!Number.isFinite(length) || buffer.length < headerEnd + 4 + length) return;
      const value: unknown = JSON.parse(buffer.subarray(headerEnd + 4, headerEnd + 4 + length).toString());
      received.push(value);
      buffer = buffer.subarray(headerEnd + 4 + length);
      if (!isRecord(value)) continue;
      if (value["method"] === "client/registerCapability") {
        send({ id: value["id"], result: null });
        continue;
      }
      if (value["method"] === "workspace/configuration") {
        const params = value["params"];
        const items = isRecord(params) ? params["items"] : undefined;
        send({ id: value["id"], result: Array.isArray(items) ? items.map(() => ({})) : [] });
        continue;
      }
      if (typeof value["id"] !== "number") continue;
      const response = pending.get(value["id"]);
      pending.delete(value["id"]);
      if (value["error"] !== undefined) response?.reject(new Error(JSON.stringify(value["error"])));
      else response?.resolve(value["result"]);
    }
  });

  /** Frame JSON-RPC messages using byte lengths rather than Unicode string lengths. */
  function send(message: Record<string, unknown>): void {
    const json = JSON.stringify({ jsonrpc: "2.0", ...message });
    child.stdin.write(`Content-Length: ${Buffer.byteLength(json)}\r\n\r\n${json}`);
  }

  /** Bound each editor request so missing diagnostics cannot hang local hooks or CI. */
  async function request(method: string, params: unknown): Promise<unknown> {
    const id = ++nextId;
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await new Promise<unknown>((resolve, reject): void => {
        pending.set(id, { resolve, reject });
        timer = setTimeout(
          () => reject(new Error(`Native LSP timed out on ${method}: ${stderr}\n${JSON.stringify(received)}`)),
          10_000,
        );
        send({ id, method, params });
      });
    } finally {
      clearTimeout(timer);
      pending.delete(id);
    }
  }

  const initialized = await request("initialize", {
    processId: process.pid,
    rootUri: pathToFileURL(root).href,
    capabilities: { textDocument: { diagnostic: { dynamicRegistration: false } } },
  });
  assert.ok(isRecord(initialized) && isRecord(initialized["serverInfo"]));
  assert.match(String(initialized["serverInfo"]["version"]), /^7\./);
  send({ method: "initialized", params: {} });
  send({
    method: "textDocument/didOpen",
    params: {
      textDocument: { uri: pathToFileURL(filename).href, languageId: "typescript", version: 1, text: source },
    },
  });
  const diagnostic = await request("textDocument/diagnostic", { textDocument: { uri: pathToFileURL(filename).href } });
  assert.ok(isRecord(diagnostic) && Array.isArray(diagnostic["items"]));
  assert.ok(diagnostic["items"].some((item: unknown) => isRecord(item) && item["code"] === 2322));
  for (const relative of [
    "frontend/review/main.tsx",
    "frontend/review/core/types.ts",
    "frontend/tests/harness.ts",
    "scripts/build-review.mts",
    "playwright.config.ts",
  ]) {
    const file = join(root, relative);
    const uri = pathToFileURL(file).href;
    send({
      method: "textDocument/didOpen",
      params: {
        textDocument: {
          uri,
          languageId: relative.endsWith(".tsx") ? "typescriptreact" : "typescript",
          version: 1,
          text: await readFile(file, "utf8"),
        },
      },
    });
    const result = await request("textDocument/diagnostic", { textDocument: { uri } });
    assert.ok(isRecord(result) && Array.isArray(result["items"]));
    assert.deepEqual(result["items"], [], `${relative} must remain diagnostic-free in the native editor server`);
    send({ method: "textDocument/didClose", params: { textDocument: { uri } } });
  }
  await request("shutdown", undefined);
  send({ method: "exit" });
});
