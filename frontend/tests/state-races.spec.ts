/** Exercise the real hook/queue integration under delayed responses, shared navigation, and unrelated live events. */
import { expect, test } from "@playwright/test";
import type { ReviewUpdateRequest } from "../review/core/types";
import { openReview, sendState } from "./harness";
import { required } from "./required";

test("rapid availability changes compose after the earlier request is acknowledged", async ({
  page,
}): Promise<void> => {
  const harness = await openReview(page);
  const writes: ReviewUpdateRequest[] = [];
  let release: () => void = (): void => {
    throw new Error("Delayed response was not initialized");
  };
  const ready = new Promise<void>((resolve): void => {
    release = resolve;
  });
  await page.route("**/api/review", async (route): Promise<void> => {
    const body = route.request().postDataJSON() as ReviewUpdateRequest;
    writes.push(body);
    if (writes.length === 1) await ready;
    const image = required(harness.data.images[0]);
    if (body.enabled_profile_indexes)
      for (const profile of image.profiles)
        profile.enabled = body.enabled_profile_indexes.includes(profile.profile_index);
    await route.fulfill({ json: { ...harness.data, type: "patch" } });
  });
  const first = page.locator(".profile-availability").nth(0);
  const second = page.locator(".profile-availability").nth(1);
  await first.uncheck();
  await expect.poll(() => writes.length).toBe(1);
  await second.uncheck();
  await expect(first).not.toBeChecked();
  await expect(second).not.toBeChecked();
  release();
  await expect.poll(() => writes.length).toBe(2);
  expect(writes.map((body) => body.enabled_profile_indexes)).toEqual([[1], []]);
  await expect(first).not.toBeChecked();
  await expect(second).not.toBeChecked();
  expect(harness.errors).toEqual([]);
});

test("notes for two images survive shared navigation inside the autosave debounce", async ({ page }): Promise<void> => {
  const harness = await openReview(page);
  await page.clock.install();
  const notes = page.locator("#notes");
  await notes.fill("Unsaved first picture");
  harness.data.ui.current_image_id = 2;
  await sendState(page, structuredClone(harness.data));
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
  await notes.fill("Separate second picture");
  await page.clock.runFor(600);
  await expect
    .poll(() => harness.data.images.map((image) => image.notes))
    .toEqual(["Unsaved first picture", "Separate second picture", "Camera note"]);
  const notesWrites = harness.requests
    .filter((request) => request.path === "review")
    .map((request) => request.body as ReviewUpdateRequest);
  expect(notesWrites.map((body) => [body.image_id, body.notes])).toEqual([
    [1, "Unsaved first picture"],
    [2, "Separate second picture"],
  ]);
  expect(harness.errors).toEqual([]);
});

test("a client-count event does not hide unacknowledged retouch pixels", async ({ page }): Promise<void> => {
  const harness = await openReview(page);
  await page.clock.install();
  await page.locator("#retouch-exposure").evaluate((input: HTMLInputElement): void => {
    input.value = "1";
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await expect(page.locator("#main-image")).toHaveAttribute("style", /brightness/);
  await sendState(page, { type: "patch", version: harness.data.version, client_count: 4 });
  await expect(page.locator("#main-image")).toHaveAttribute("style", /brightness/);
  await expect(page.locator("#profile-state")).toContainText("retouch draft");
  expect(harness.requests.filter((request) => request.path === "review")).toHaveLength(0);
  await page.clock.runFor(1300);
  await expect.poll(() => required(harness.data.images[0]).retouch.adjustments.exposure).toBe(1);
  expect(harness.errors).toEqual([]);
});

test("a failed metadata save retains its draft and exposes an explicit retry", async ({ page }): Promise<void> => {
  const harness = await openReview(page);
  let attempts = 0;
  await page.route("**/api/review", async (route): Promise<void> => {
    attempts += 1;
    if (attempts === 1) {
      await route.fulfill({ status: 503, json: { error: "Review store temporarily unavailable" } });
      return;
    }
    const body = route.request().postDataJSON() as ReviewUpdateRequest;
    required(harness.data.images[0]).notes = body.notes;
    await route.fulfill({ json: { ...harness.data, type: "patch" } });
  });
  await page.locator("#notes").fill("A recoverable note");
  await expect(page.getByText(/Edits are still unsaved/)).toBeVisible();
  expect(attempts).toBe(1);
  await expect(page.locator("#notes")).toHaveValue("A recoverable note");
  await page.getByRole("button", { name: "Retry save" }).click();
  await expect.poll(() => attempts).toBe(2);
  await expect(page.getByText(/Edits are still unsaved/)).toHaveCount(0);
  expect(required(harness.data.images[0]).notes).toBe("A recoverable note");
});

for (const action of ["rating", "profile", "label", "next"] as const) {
  test(`${action} stays attached to the image where it was triggered while its draft save is blocked`, async ({
    page,
  }): Promise<void> => {
    const harness = await openReview(page);
    const writes: ReviewUpdateRequest[] = [];
    let release: () => void = (): void => {};
    const blocked = new Promise<void>((resolve): void => {
      release = resolve;
    });
    await page.route("**/api/review", async (route): Promise<void> => {
      const body = route.request().postDataJSON() as ReviewUpdateRequest;
      writes.push(body);
      if (writes.length === 1) await blocked;
      await route.fallback();
    });
    await page.locator("#notes").fill("The first image owns this draft");
    if (action === "rating") await page.locator('[data-rating="4"]').click();
    else if (action === "profile") await page.locator(".profile-card").nth(1).click();
    else if (action === "label") await page.locator('[data-label="red"]').click();
    else {
      await page.locator("#notes").blur();
      await page.keyboard.press("ArrowRight");
    }
    await expect.poll(() => writes.length).toBe(1);
    harness.data.ui.current_image_id = 2;
    await sendState(page, structuredClone(harness.data));
    await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
    release();
    await expect.poll(() => writes.length).toBeGreaterThanOrEqual(2);
    expect(writes[1]?.image_id).toBe(1);
    if (action === "rating") expect(writes[1]?.rating).toBe(4);
    if (action === "profile") expect(writes[1]?.selected_profile_index).toBe(1);
    if (action === "label") expect(writes[1]?.labels).toContain("red");
    if (action === "next") {
      await expect.poll(() => harness.requests.filter((request) => request.path === "ui").length).toBe(1);
      expect(harness.data.ui.current_image_id).toBe(2);
    }
    expect(harness.errors).toEqual([]);
  });
}

test("shared navigation transfers focused raw input ownership without normalizing its spelling", async ({
  page,
}): Promise<void> => {
  const harness = await openReview(page);
  const tags = page.locator("#tags");
  await tags.focus();
  harness.data.ui.current_image_id = 2;
  await sendState(page, structuredClone(harness.data));
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
  await tags.fill("007,   007");
  await expect.poll(() => required(harness.data.images[1]).tags).toEqual(["007", "007"]);
  await expect(tags).toHaveValue("007,   007");
  await sendState(page, structuredClone(harness.data));
  await expect(tags).toHaveValue("007,   007");
  expect(harness.errors).toEqual([]);
});

test("a retouch edited during pending profile selection uses the displayed profile baseline", async ({
  page,
}): Promise<void> => {
  const harness = await openReview(page);
  required(harness.data.profiles[0]).retouch_base.exposure = 4;
  required(harness.data.profiles[1]).retouch_base.exposure = -4;
  await sendState(page, structuredClone(harness.data));
  const writes: ReviewUpdateRequest[] = [];
  let release: () => void = (): void => {};
  const blocked = new Promise<void>((resolve): void => {
    release = resolve;
  });
  await page.route("**/api/review", async (route): Promise<void> => {
    writes.push(route.request().postDataJSON() as ReviewUpdateRequest);
    if (writes.length === 1) await blocked;
    await route.fallback();
  });
  await page.locator(".profile-card").nth(1).click();
  await expect.poll(() => writes.length).toBe(1);
  await expect(page.locator("#retouch-exposure")).toHaveValue("-4");
  await page.locator("#retouch-exposure").evaluate((input: HTMLInputElement): void => {
    input.value = "0";
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await page.locator("#retouch-exposure").press("Enter");
  release();
  await expect.poll(() => writes.length).toBe(2);
  expect(writes[1]?.retouch?.adjustments.exposure).toBe(4);
  expect(harness.errors).toEqual([]);
});

test("a failed assignment refreshes before explicit retry and preserves refreshed unrelated fields", async ({
  page,
}): Promise<void> => {
  const harness = await openReview(page);
  let attempts = 0;
  const writes: ReviewUpdateRequest[] = [];
  await page.route("**/api/review", async (route): Promise<void> => {
    attempts += 1;
    writes.push(route.request().postDataJSON() as ReviewUpdateRequest);
    if (attempts === 1) {
      await route.fulfill({ status: 503, json: { error: "Assignment response lost" } });
      return;
    }
    await route.fallback();
  });
  await page.locator('[data-label="red"]').click();
  await expect(page.getByRole("button", { name: "Refresh and retry" })).toBeVisible();
  await expect.poll(() => harness.requests.filter((request) => request.path === "state").length).toBe(2);
  expect(attempts).toBe(1);
  required(harness.data.images[0]).notes = "New server note before retry";
  await page.getByRole("button", { name: "Refresh and retry" }).click();
  await expect.poll(() => attempts).toBe(2);
  expect(writes[1]?.notes).toBe("New server note before retry");
  expect(writes[1]?.labels).toContain("red");
  await expect(page.getByRole("button", { name: "Refresh and retry" })).toHaveCount(0);
});

test("an ambiguous rating-and-advance can only be checked, not replayed", async ({ page }): Promise<void> => {
  const harness = await openReview(page);
  let attempts = 0;
  await page.route("**/api/review", async (route): Promise<void> => {
    attempts += 1;
    required(harness.data.images[0]).rating = 4;
    harness.data.ui.current_image_id = 2;
    await route.fulfill({ status: 503, json: { error: "Committed but acknowledgement lost" } });
  });
  await page.locator('[data-rating="4"]').click();
  await expect(page.getByRole("button", { name: "Check state" })).toBeVisible();
  await page.getByRole("button", { name: "Check state" }).click();
  await expect(page.getByRole("button", { name: "Check state" })).toHaveCount(0);
  expect(attempts).toBe(1);
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
});

test("a successful label change retains an unrelated failed availability intention", async ({
  page,
}): Promise<void> => {
  const harness = await openReview(page);
  const writes: ReviewUpdateRequest[] = [];
  await page.route("**/api/review", async (route): Promise<void> => {
    writes.push(route.request().postDataJSON() as ReviewUpdateRequest);
    if (writes.length === 1) {
      await route.fulfill({ status: 503, json: { error: "Profile response lost" } });
      return;
    }
    await route.fallback();
  });
  await page.locator(".profile-availability").first().uncheck();
  await expect(page.getByRole("button", { name: "Refresh and retry" })).toBeVisible();
  await page.locator('[data-label="red"]').click();
  await expect.poll(() => writes.length).toBe(2);
  await expect(page.getByRole("button", { name: "Refresh and retry" })).toBeVisible();
  await page.getByRole("button", { name: "Refresh and retry" }).click();
  await expect.poll(() => writes.length).toBe(3);
  expect(writes[2]?.enabled_profile_indexes).toEqual([1]);
  expect(writes[2]?.labels).toContain("red");
  await expect(page.getByRole("button", { name: "Refresh and retry" })).toHaveCount(0);
  expect(required(harness.data.images[0]).profiles[0]?.enabled).toBe(false);
});

test("an ambiguous acknowledgement resyncs before the next queued legacy body is compiled", async ({
  page,
}): Promise<void> => {
  const harness = await openReview(page);
  const writes: ReviewUpdateRequest[] = [];
  let release: () => void = (): void => {};
  const blocked = new Promise<void>((resolve): void => {
    release = resolve;
  });
  await page.route("**/api/review", async (route): Promise<void> => {
    const body = route.request().postDataJSON() as ReviewUpdateRequest;
    writes.push(body);
    if (writes.length === 1) {
      await blocked;
      required(harness.data.images[0]).labels = body.labels;
      required(harness.data.images[0]).label = body.label;
      await route.fulfill({ status: 503, json: { error: "Committed label response lost" } });
      return;
    }
    await route.fallback();
  });
  await page.locator('[data-label="red"]').click();
  await expect.poll(() => writes.length).toBe(1);
  await page.locator(".profile-availability").first().uncheck();
  release();
  await expect.poll(() => writes.length).toBe(2);
  expect(writes[1]?.labels).toContain("red");
  expect(writes[1]?.enabled_profile_indexes).toEqual([1]);
  expect(harness.requests.filter((request) => request.path === "state")).toHaveLength(2);
  await expect(page.getByRole("button", { name: "Refresh and retry" })).toBeVisible();
});
