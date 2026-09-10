/** Validate the embedded frontend against a relocated daemon, persistent review data, and its native SSE stream. */
import { expect, test } from "@playwright/test";
import { execFile, spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdir, mkdtemp, symlink } from "node:fs/promises";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import { decodeStateMessage } from "../review/core/api";
import type { ReviewStateSnapshot } from "../review/generated/responses";
import { required } from "./required";

const execute = promisify(execFile);

/** Reserve an ephemeral loopback port, releasing it immediately before starting the single test daemon. */
async function availablePort(): Promise<number> {
  const reservation = createServer();
  reservation.listen(0, "127.0.0.1");
  await once(reservation, "listening");
  const address = reservation.address();
  if (address === null || typeof address === "string") throw new Error("Loopback port was not allocated");
  await new Promise<void>((resolveClose, reject): void => {
    reservation.close((error): void => {
      if (error) reject(error);
      else resolveClose();
    });
  });
  return address.port;
}

/** Resolve trusted local image tools before the daemon receives its Node-free executable search path. */
async function toolPath(name: string): Promise<string> {
  const result = await execute("which", [name]);
  return result.stdout.trim();
}

/** Require a Rust-validated complete snapshot rather than asserting an arbitrary HTTP object. */
async function snapshot(url: string): Promise<ReviewStateSnapshot> {
  const response = await fetch(`${url}api/state`);
  if (!response.ok) throw new Error(`State HTTP ${response.status}`);
  const message = decodeStateMessage(await response.text());
  if ("type" in message) throw new Error("State route returned a patch without a baseline");
  return message;
}

test("relocated binary serves one bundle, real SSE, persisted edits and publish without Node or npm", async ({
  page,
  context,
}, info): Promise<void> => {
  const directory = await mkdtemp(resolve("target/review-daemon-"));
  const input = join(directory, "input");
  const output = join(directory, "output");
  const tools = join(directory, "tools");
  await Promise.all([mkdir(input), mkdir(output), mkdir(tools)]);
  const executable = join(directory, "mini-film");
  await copyFile(resolve("target/debug/mini-film"), executable);
  const [convert, exiftool, rawtherapee, bash] = await Promise.all([
    toolPath("convert"),
    toolPath("exiftool"),
    toolPath("rawtherapee-cli"),
    toolPath("bash"),
  ]);
  await symlink(exiftool, join(tools, "exiftool"));
  await symlink(bash, join(tools, "bash"));
  await execute(convert, ["-size", "640x400", "gradient:#50657c-#b8b39b", join(input, "smoke.jpg")]);
  const port = await availablePort();
  const url = `http://127.0.0.1:${port}/`;
  const daemon = spawn(
    executable,
    [
      "daemon",
      input,
      output,
      "--input-jpg-only",
      "--jobs",
      "1",
      "--review-address",
      `127.0.0.1:${port}`,
      "--convert",
      convert,
      "--rawtherapee",
      rawtherapee,
      "--no-grain",
      "--no-normalize-grain",
    ],
    { cwd: directory, env: { ...process.env, PATH: tools }, stdio: ["ignore", "pipe", "pipe"] },
  );
  const logs: string[] = [];
  daemon.stdout?.on("data", (chunk: Buffer): void => {
    logs.push(chunk.toString());
  });
  daemon.stderr?.on("data", (chunk: Buffer): void => {
    logs.push(chunk.toString());
  });
  const exited = once(daemon, "exit");
  const errors: string[] = [];
  const scripts = new Set<string>();
  page.on("pageerror", (error): void => {
    errors.push(error.message);
  });
  page.on("console", (message): void => {
    if (message.type() === "error" || message.type() === "warning") errors.push(message.text());
  });
  page.on("request", (request): void => {
    if (request.resourceType() === "script") scripts.add(request.url());
  });
  try {
    await expect
      .poll(
        async (): Promise<number> => {
          if (daemon.exitCode !== null) throw new Error(`Daemon exited: ${logs.join("")}`);
          try {
            return (await snapshot(url)).images.length;
          } catch {
            return 0;
          }
        },
        { timeout: 45_000 },
      )
      .toBe(1);
    await page.goto(url);
    await expect(page.locator("#image-title")).toHaveText("smoke.jpg");
    await expect(page.locator("#live-dot")).toHaveClass(/connected/);
    await expect
      .poll(() =>
        page
          .locator("#main-image")
          .evaluate((image: HTMLImageElement): boolean => image.complete && image.naturalWidth > 0),
      )
      .toBe(true);
    expect([...scripts].map((source) => new URL(source).origin + new URL(source).pathname)).toEqual([
      `${url}assets/app.js`,
    ]);

    // Real listener responses negotiate representations and revalidate without weakening API cache behavior.
    const asset = await context.request.get(`${url}assets/app.js`, { headers: { "Accept-Encoding": "gzip" } });
    expect(asset.headers()["content-encoding"]).toBe("gzip");
    expect(asset.headers()["vary"]).toBe("Accept-Encoding");
    const etag = required(asset.headers()["etag"]);
    const cached = await context.request.get(`${url}assets/app.js`, {
      headers: { "Accept-Encoding": "gzip", "If-None-Match": etag },
    });
    expect(cached.status()).toBe(304);
    expect(await cached.body()).toHaveLength(0);
    expect((await context.request.get(`${url}api/state`)).headers()["cache-control"]).toContain("no-store");

    const second = await context.newPage();
    await second.goto(url);
    await expect(second.locator("#image-title")).toHaveText("smoke.jpg");
    await page.locator("#notes").fill("Real daemon note survives reload");
    await expect(second.locator("#notes")).toHaveValue("Real daemon note survives reload");
    await page.reload();
    await expect(page.locator("#notes")).toHaveValue("Real daemon note survives reload");
    expect(required((await snapshot(url)).images[0]).notes).toBe("Real daemon note survives reload");

    await page.locator("#notes").fill("Committed immediately before publish");
    await page.locator("#publish").click();
    await page.locator("#publish-album").fill("integration-publish");
    await page.locator("#publish-submit").click();
    await expect
      .poll(async (): Promise<string | undefined> => (await snapshot(url)).publish_jobs[0]?.status, { timeout: 60_000 })
      .toBe("done");
    expect(required((await snapshot(url)).images[0]).notes).toBe("Committed immediately before publish");
    const metadata = await execute(exiftool, [
      "-s",
      "-s",
      "-s",
      "-XMP:Description",
      join(output, "integration-publish", "smoke.jpg"),
    ]);
    expect(metadata.stdout.trim()).toBe("Committed immediately before publish");
    expect(errors).toEqual([]);
    await second.close();
  } finally {
    daemon.kill("SIGTERM");
    await exited;
    await info.attach("daemon-log", { body: logs.join(""), contentType: "text/plain" });
  }
});
