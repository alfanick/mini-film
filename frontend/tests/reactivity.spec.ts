/** Count actual Preact component calls with a test-only entry; signal counters alone cannot detect parent fanout. */
import { build } from "esbuild";
import { expect, test, type Page } from "@playwright/test";
import { openReview, sendKeepalive, sendState } from "./harness";
import { required } from "./required";

/** Instrument the installed Preact renderer without shipping a debug global or counter in the product bundle. */
async function instrumentedBundle(): Promise<string> {
  const result = await build({
    stdin: {
      resolveDir: process.cwd(),
      loader: "tsx",
      contents: `
        import { options, type VNode } from "preact";
        import "./frontend/review/main.tsx";
        const renderOptions = options as typeof options & { __r?: (node: VNode) => void };
        const previous = renderOptions.__r;
        renderOptions.__r = (node: VNode): void => {
          if (typeof node.type === "function") performance.mark("review-render:" + node.type.name);
          previous?.(node);
        };
      `,
    },
    bundle: true,
    keepNames: true,
    write: false,
    format: "iife",
    target: ["es2022"],
    jsx: "automatic",
    jsxImportSource: "preact",
  });
  return required(result.outputFiles[0]).text;
}

/** Let scheduled Preact commits and layout observers settle before sampling the render hook. */
async function settle(page: Page): Promise<void> {
  await page.evaluate(
    (): Promise<void> =>
      new Promise((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
  );
}

/** The renderer records component calls, not DOM changes that could hide unnecessary VDOM work. */
async function renderCounts(page: Page): Promise<Record<string, number>> {
  await settle(page);
  return page.evaluate((): Record<string, number> => {
    const counts: Record<string, number> = {};
    for (const entry of performance.getEntriesByType("mark")) {
      if (!entry.name.startsWith("review-render:")) continue;
      const name = entry.name.slice("review-render:".length);
      counts[name] = (counts[name] || 0) + 1;
    }
    return counts;
  });
}

test("keepalive and client counts stay at their status leaf; one image patch renders one row", async ({
  page,
}, info): Promise<void> => {
  test.skip(
    info.project.name.endsWith("release"),
    "Render probes use an unminified test entry, not release instrumentation",
  );
  const bundle = await instrumentedBundle();
  await page.route("**/assets/app.js", (route) => route.fulfill({ contentType: "text/javascript", body: bundle }));
  const harness = await openReview(page);
  await settle(page);
  await page.evaluate((): void => performance.clearMarks());
  await sendKeepalive(page, { type: "keepalive", version: harness.data.version, datetime: "2026-09-10T12:00:00Z" });
  const pulse = await renderCounts(page);
  expect(pulse["LiveConnection"]).toBeGreaterThan(0);
  for (const name of ["ReviewWorkspace", "Viewer", "Controls", "ImageRow", "PublishForm", "PublishSelectionSection"])
    expect(pulse[name] || 0, `keepalive called ${name}`).toBe(0);

  await page.evaluate((): void => performance.clearMarks());
  await sendState(page, { type: "patch", version: harness.data.version, client_count: 9 });
  const clients = await renderCounts(page);
  expect(clients["LiveConnection"]).toBeGreaterThan(0);
  for (const name of ["ReviewWorkspace", "Viewer", "Controls", "ImageRow", "PublishForm"])
    expect(clients[name] || 0, `client count called ${name}`).toBe(0);

  await page.evaluate((): void => performance.clearMarks());
  const image = { ...required(harness.data.images[1]), notes: "Only this row changed" };
  await sendState(page, { type: "patch", version: harness.data.version, images: [image] });
  const changed = await renderCounts(page);
  expect(changed["ImageRow"], JSON.stringify(changed)).toBe(1);
  for (const name of ["ReviewWorkspace", "Viewer", "Controls", "PublishForm"])
    expect(changed[name] || 0, `non-current image patch called ${name}: ${JSON.stringify(changed)}`).toBe(0);
  expect(harness.errors).toEqual([]);
});
