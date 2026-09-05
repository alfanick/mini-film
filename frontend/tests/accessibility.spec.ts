/** Verify real native focus, keyboard editing, and automated accessibility across both embedded browser engines. */
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { openReview } from "./harness";

/** Assert focus remains in the active native modal, including after an attempted background focus change. */
async function expectModalFocus(dialog: Locator): Promise<void> {
  await expect.poll(() => dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true);
}

/** Run the same WCAG rules against the real page and retain actionable selectors in failure output. */
async function expectAccessible(page: Page): Promise<void> {
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    expect(
      result.violations.map((violation) => ({
        scheme: colorScheme,
        rule: violation.id,
        nodes: violation.nodes.map((node) => ({ target: node.target, summary: node.failureSummary })),
      })),
    ).toEqual([]);
  }
}

const dialogs: readonly { name: string; opener: string }[] = [
  { name: "Publish", opener: "#publish" },
  { name: "Shortcuts", opener: "#shortcuts-help" },
  { name: "Profile info", opener: ".current-profile-link" },
  { name: "Command invocation", opener: "#app-version" },
  { name: "Sampler", opener: "#sampler" },
  { name: "Diffusion", opener: "#diffusion" },
  { name: "Panorama", opener: "#panorama" },
];

for (const { name, opener } of dialogs) {
  test(`${name} is named, traps focus, suspends background shortcuts, and restores its invoker`, async ({ page }) => {
    const harness = await openReview(page);
    const trigger = page.locator(opener);
    await trigger.focus();
    await trigger.press("Enter");
    const dialog = page.getByRole("dialog", { name, exact: true });
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveJSProperty("open", true);
    await expectModalFocus(dialog);
    const reviewCount = harness.requests.filter((request) => request.path === "review").length;
    await page.keyboard.press("5");
    await page.keyboard.press("PageDown");
    await page.keyboard.press("h");
    await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
    expect(harness.requests.filter((request) => request.path === "review")).toHaveLength(reviewCount);
    await page.locator("#min-rating").evaluate((element: HTMLSelectElement) => element.focus());
    await expectModalFocus(dialog);
    for (let step = 0; step < 18; step += 1) {
      await page.keyboard.press(step % 2 === 0 ? "Shift+Tab" : "Tab");
      await expectModalFocus(dialog);
    }
    await expectAccessible(page);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
    expect(harness.errors).toEqual([]);
  });
}

test("native profile activation and checkbox Space keep their independent meanings", async ({ page }) => {
  const harness = await openReview(page);
  const card = page.locator(".profile-card").nth(1);
  const checkbox = page.locator(".profile-availability").nth(1);
  await expect(page.locator("button input")).toHaveCount(0);
  await checkbox.focus();
  await checkbox.press("Space");
  await expect(checkbox).not.toBeChecked();
  await expect(page.locator(".profile-card").first()).toHaveAttribute("aria-pressed", "true");
  await expect
    .poll(() => harness.requests.find((request) => request.path === "review")?.body)
    .toMatchObject({
      image_id: 1,
      enabled_profile_indexes: [0],
      advance_after_update: false,
    });
  await card.focus();
  await card.press("Enter");
  await expect(card).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
  expect(harness.errors).toEqual([]);
});

test("the photo exposes keyboard zoom without rating, profile toggling, or navigation", async ({ page }) => {
  const harness = await openReview(page);
  const image = page.getByRole("button", { name: "Zoom frame-1.NEF", exact: true });
  await image.focus();
  await image.press("Enter");
  await expect(page.locator("#zoom-full")).toBeVisible();
  await expect(image).toHaveAttribute("aria-pressed", "true");
  await image.press("Space");
  await expect(page.locator("#zoom-full")).toBeHidden();
  await image.press("Space");
  await expect(page.locator("#zoom-full")).toBeVisible();
  await image.press("Escape");
  await expect(page.locator("#zoom-full")).toBeHidden();
  await expect(image).toBeFocused();
  expect(harness.requests.filter((request) => request.path === "review")).toHaveLength(0);
  await expectAccessible(page);
  expect(harness.errors).toEqual([]);
});

test("crop arrows move displayed pixels and resize locked corners without changing the photo", async ({ page }) => {
  const harness = await openReview(page);
  await page.locator("#crop-toggle").click();
  await expect(page.locator("#crop-ratio")).toBeEnabled();
  await page.locator("#crop-ratio").selectOption("1:1");
  const body = page.getByRole("group", { name: "Move crop selection", exact: true });
  const initial = await body.boundingBox();
  if (!initial) throw new Error("The crop frame must have displayed geometry");
  await body.focus();
  await body.press("ArrowRight");
  await expect.poll(async () => (await body.boundingBox())?.x).toBeCloseTo(initial.x + 1, 1);
  await body.press("Shift+ArrowRight");
  await expect.poll(async () => (await body.boundingBox())?.x).toBeCloseTo(initial.x + 11, 1);
  const corner = page.getByRole("button", { name: "Resize crop from bottom right", exact: true });
  await corner.focus();
  await corner.press("Shift+ArrowLeft");
  await expect.poll(async () => (await body.boundingBox())?.width).toBeCloseTo(initial.width - 10, 1);
  const resized = await body.boundingBox();
  if (!resized) throw new Error("Keyboard edits must retain the crop frame");
  expect(resized.width / resized.height).toBeCloseTo(1, 2);
  await corner.press("ArrowRight");
  await expect.poll(async () => (await body.boundingBox())?.width).toBeCloseTo(initial.width - 9, 1);
  expect(harness.requests.filter((request) => request.path === "review")).toHaveLength(0);
  await expectAccessible(page);
  await page.locator("#crop-ok").click();
  await expect.poll(() => harness.data.images[0]?.retouch.crop).not.toBeNull();
  const crop = harness.data.images[0]?.retouch.crop;
  if (!crop) throw new Error("Approving a keyboard crop must save its normalized geometry");
  expect((crop.width * 1200) / (crop.height * 800)).toBeCloseTo(1, 4);
  expect(crop.x).toBeGreaterThanOrEqual(0);
  expect(crop.y).toBeGreaterThanOrEqual(0);
  expect(crop.x + crop.width).toBeLessThanOrEqual(1);
  expect(crop.y + crop.height).toBeLessThanOrEqual(1);
  await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
  expect(harness.errors).toEqual([]);
});

test("publish dimensions have persistent names and job updates have live announcements", async ({ page }) => {
  await openReview(page);
  await page.locator("#publish").click();
  await page.locator("#publish-size-mode").selectOption("long-edge");
  await expect(page.getByRole("spinbutton", { name: "Long edge in pixels", exact: true })).toBeVisible();
  await page.locator("#publish-size-mode").selectOption("bounds");
  await expect(page.getByRole("spinbutton", { name: "Maximum width in pixels", exact: true })).toBeVisible();
  await expect(page.getByRole("spinbutton", { name: "Maximum height in pixels", exact: true })).toBeVisible();
  await page.locator("#publish-size-mode").selectOption("geometry");
  await expect(page.getByRole("textbox", { name: "ImageMagick resize geometry", exact: true })).toBeVisible();
  await expect(page.locator("#publish-status")).toHaveAttribute("role", "status");
  await expect(page.locator("#publish-status")).toHaveAttribute("aria-live", "polite");
});

test("mobile dialogs restore focus after both their close button and backdrop", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const harness = await openReview(page);
  const trigger = page.locator("#mobile-publish");
  await trigger.focus();
  await trigger.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Publish", exact: true });
  await expect(dialog).toBeVisible();
  await expectModalFocus(dialog);
  await expectAccessible(page);
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(trigger).toBeFocused();
  await trigger.press("Enter");
  await expect(dialog).toBeVisible();
  await dialog.click({ position: { x: 2, y: 2 } });
  await expect(dialog).toBeHidden();
  await expect(trigger).toBeFocused();
  expect(harness.errors).toEqual([]);
});

test("rating and color labels announce their selected toggle states", async ({ page }) => {
  const harness = await openReview(page);
  await expect(page.getByRole("button", { name: "Rate 2 stars", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: "Rate 3 stars", exact: true })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  const red = page.getByRole("button", { name: "Red label", exact: true });
  await expect(red).toHaveAttribute("aria-pressed", "false");
  await red.focus();
  await red.press("Enter");
  await expect(red).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
  await red.press("Space");
  await expect(red).toHaveAttribute("aria-pressed", "false");
  const yellow = page.getByRole("button", { name: "Yellow label", exact: true });
  await yellow.focus();
  await yellow.press("Enter");
  await expect(yellow).toHaveAttribute("aria-pressed", "true");
  await expectAccessible(page);
  expect(harness.errors).toEqual([]);
});
