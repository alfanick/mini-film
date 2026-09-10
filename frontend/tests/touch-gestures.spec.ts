/** Chromium CDP exercises trusted continuous and multi-contact touch through the browser's native input stack. */
import { expect, test, type CDPSession, type Locator } from "@playwright/test";
import { openReview } from "./harness";
import { required } from "./required";

/** Browser coordinates and stable contact identities model fingers across native touch frames. */
interface Contact {
  id: number;
  x: number;
  y: number;
}

/** Native dispatch preserves capture, cancellation, pointer type, and trusted-event behavior. */
async function touch(
  session: CDPSession,
  type: "touchStart" | "touchMove" | "touchEnd" | "touchCancel",
  contacts: readonly Contact[],
): Promise<void> {
  await session.send("Input.dispatchTouchEvent", { type, touchPoints: contacts.map((contact) => ({ ...contact })) });
}

/** Require visible photo/crop geometry before choosing contact locations. */
async function center(locator: Locator): Promise<{ x: number; y: number; width: number; height: number }> {
  const rect = await locator.boundingBox();
  if (rect === null) throw new Error("Touch gestures require visible media geometry");
  return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2, width: rect.width, height: rect.height };
}

test("native swipes navigate or rate on only their dominant axis", async ({ page }) => {
  const harness = await openReview(page);
  const session = await page.context().newCDPSession(page);
  const photo = await center(page.locator("#main-image"));
  await touch(session, "touchStart", [{ id: 1, x: photo.x + 50, y: photo.y }]);
  await touch(session, "touchMove", [{ id: 1, x: photo.x - 50, y: photo.y + 4 }]);
  await touch(session, "touchEnd", []);
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
  await touch(session, "touchStart", [{ id: 2, x: photo.x, y: photo.y + 45 }]);
  await touch(session, "touchMove", [{ id: 2, x: photo.x + 4, y: photo.y - 45 }]);
  await touch(session, "touchEnd", []);
  await expect.poll(() => required(harness.data.images[1]).rating).toBe(3);
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
  const confirmedWrites = harness.requests.filter((request) => request.name === "review").length;
  await touch(session, "touchStart", [{ id: 3, x: photo.x + 45, y: photo.y + 45 }]);
  await touch(session, "touchMove", [{ id: 3, x: photo.x - 45, y: photo.y - 45 }]);
  await touch(session, "touchEnd", []);
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
  expect(harness.requests.filter((request) => request.name === "review")).toHaveLength(confirmedWrites);
  expect(harness.errors).toEqual([]);
  await session.detach();
});

test("native hold opens the loupe and cancellation never becomes a swipe or rating", async ({ page }) => {
  const harness = await openReview(page);
  const session = await page.context().newCDPSession(page);
  const photo = await center(page.locator("#main-image"));
  await touch(session, "touchStart", [{ id: 1, x: photo.x, y: photo.y }]);
  await expect(page.locator("#zoom-loupe")).toBeVisible();
  await touch(session, "touchMove", [{ id: 1, x: photo.x - 100, y: photo.y }]);
  await touch(session, "touchCancel", []);
  await expect(page.locator("#zoom-loupe")).toBeHidden();
  await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
  await touch(session, "touchStart", [{ id: 2, x: photo.x + 40, y: photo.y }]);
  await touch(session, "touchMove", [{ id: 2, x: photo.x - 40, y: photo.y }]);
  await touch(session, "touchCancel", []);
  await expect(page.locator("#zoom-loupe")).toBeHidden();
  expect(harness.requests.filter((request) => request.name === "review")).toHaveLength(0);
  expect(harness.errors).toEqual([]);
  await session.detach();
});

test("native two-finger crop scales and rotates locally, keeps bounds, then commits once", async ({ page }) => {
  // Tablet touch layout exposes the existing sidebar crop command without inventing a phone-only control.
  await page.setViewportSize({ width: 1024, height: 900 });
  const harness = await openReview(page);
  await page.locator("#crop-toggle").tap();
  await expect(page.locator("#crop-ratio")).toBeEnabled();
  await page.locator("#crop-ratio").selectOption("1:1");
  const crop = await center(page.locator("#crop-box"));
  const session = await page.context().newCDPSession(page);
  await touch(session, "touchStart", [
    { id: 1, x: crop.x - 45, y: crop.y },
    { id: 2, x: crop.x + 45, y: crop.y },
  ]);
  await touch(session, "touchMove", [
    { id: 1, x: crop.x - 28, y: crop.y - 14 },
    { id: 2, x: crop.x + 28, y: crop.y + 14 },
  ]);
  await touch(session, "touchEnd", []);
  await expect.poll(async () => Number(await page.locator("#crop-rotation").inputValue())).toBeGreaterThan(15);
  expect((await center(page.locator("#crop-box"))).width).toBeLessThan(crop.width);
  expect(harness.requests.filter((request) => request.name === "review")).toHaveLength(0);
  await page.locator("#crop-ok").tap();
  await expect.poll(() => harness.requests.filter((request) => request.name === "review").length).toBe(1);
  const result = required(harness.data.images[0]).retouch;
  expect(result.rotation_degrees).toBeGreaterThan(15);
  if (!result.crop) throw new Error("Native crop approval must persist a rectangle");
  expect(result.crop.x).toBeGreaterThanOrEqual(0);
  expect(result.crop.y).toBeGreaterThanOrEqual(0);
  expect(result.crop.x + result.crop.width).toBeLessThanOrEqual(1);
  expect(result.crop.y + result.crop.height).toBeLessThanOrEqual(1);
  await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
  expect(harness.errors).toEqual([]);
  await session.detach();
});
