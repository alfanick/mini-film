/** Verify tool request lifecycles against both implementations so reactive editing preserves export behavior. */
import { expect, test } from "@playwright/test";
import type { ReviewPanoramaProject } from "../review/core/types";
import { openReview } from "./harness";

test("failed diffusion settings writes retain the preview and error until another edit", async ({ page }) => {
  const harness = await openReview(page);
  await page.route("**/api/diffusion/settings", async (route) => {
    await route.fulfill({ status: 409, json: { error: "Settings are locked" } });
  });
  await page.locator("#diffusion").click();
  await expect(page.getByRole("button", { name: "Apply to current", exact: true })).toBeEnabled();
  const previews = harness.requests.filter((request) => request.path === "diffusion/jobs").length;
  await page.getByRole("button", { name: "Apply to current", exact: true }).click();
  await expect(page.locator(".diffusion-status")).toContainText("Could not apply diffusion: Settings are locked");
  await page.waitForTimeout(700);
  expect(harness.requests.filter((request) => request.path === "diffusion/jobs")).toHaveLength(previews);
  await expect(page.locator(".diffusion-status")).toContainText("Settings are locked");
  await expect(page.locator("#diffusion-overlay")).toBeVisible();
  await page.locator("#diffusion-softness").fill("40");
  await expect
    .poll(() => harness.requests.filter((request) => request.path === "diffusion/jobs").length)
    .toBe(previews + 1);
  await expect(page.locator(".diffusion-status")).not.toContainText("Settings are locked");
  // Browsers log the intentionally failed HTTP response; the application must not add uncaught errors.
  expect(harness.errors.every((error) => /409/.test(error))).toBe(true);
});

test("panorama source order and choices survive project creation, previews, and final rendering", async ({ page }) => {
  const harness = await openReview(page);
  const requests: { method: string; path: string; body: unknown }[] = [];
  const project: ReviewPanoramaProject = {
    id: 7,
    name: "Mountain panorama",
    status: "draft",
    matching_mode: "sequential",
    selected_projection: "cylindrical",
    output_file_name: null,
    result_image_id: null,
    progress_stage: null,
    progress_completed: 0,
    progress_total: 0,
    error: null,
    created_at: "2026-09-05T10:00:00Z",
    updated_at: "2026-09-05T10:00:00Z",
    image_ids: [2, 1],
    previews: [],
  };
  await page.route(/\/api\/panoramas(?:\/|$)/, async (route) => {
    const path = new URL(route.request().url()).pathname.split("/api/")[1];
    const body: unknown = route.request().postDataJSON();
    requests.push({ method: route.request().method(), path, body });
    if (path.endsWith("/previews")) {
      project.status = "ready";
      project.previews = [
        {
          matching_mode: "sequential",
          projection: "cylindrical",
          status: "done",
          url: harness.data.images[0].preview_url,
          duration_ms: 100,
          error: null,
          updated_at: project.updated_at,
        },
      ];
    } else if (path.endsWith("/render")) {
      project.status = "complete";
      project.result_image_id = 3;
    }
    harness.data.panorama.projects = [project];
    await route.fulfill({ json: harness.data });
  });
  await page.locator("#panorama").click();
  await page.getByLabel("Name", { exact: true }).fill("Mountain panorama");
  await page.locator(".panorama-settings select").selectOption("sequential");
  await page.getByRole("button", { name: "Move frame-2.NEF earlier", exact: true }).click();
  await page.locator(".panorama-source").filter({ hasText: "frame-3.NEF" }).locator("input").uncheck();
  await page.getByRole("button", { name: "Generate previews", exact: true }).click();
  await expect(page.getByRole("button", { name: "Render full TIFF", exact: true })).toBeEnabled();
  expect(requests).toEqual([
    {
      method: "POST",
      path: "panoramas",
      body: { name: "Mountain panorama", image_ids: [2, 1], matching_mode: "sequential" },
    },
    { method: "POST", path: "panoramas/7/previews", body: { image_ids: [2, 1], matching_mode: "sequential" } },
  ]);
  await page.getByRole("button", { name: "Render full TIFF", exact: true }).click();
  await expect(page.getByRole("button", { name: "Open result", exact: true })).toBeVisible();
  expect(requests[requests.length - 1]).toEqual({
    method: "POST",
    path: "panoramas/7/render",
    body: { name: "Mountain panorama", projection: "cylindrical" },
  });
  await page.getByRole("button", { name: "Open result", exact: true }).click();
  await expect(page.locator("#panorama-overlay")).toBeHidden();
  await expect(page.locator("#image-title")).toHaveText("frame-3.NEF");
  expect(harness.errors).toEqual([]);
});
