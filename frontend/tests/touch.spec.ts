/** Exercise trusted touch taps on both mobile browser engines, including coarse-pointer layout and modal controls. */
import { expect, test } from "@playwright/test";
import { openReview } from "./harness";

test("native touch context opens and closes mobile controls without inventing photo actions", async ({ page }) => {
  const harness = await openReview(page);
  expect(
    await page.evaluate(() => ({
      coarse: matchMedia("(pointer: coarse)").matches,
      hover: matchMedia("(hover: hover)").matches,
    })),
  ).toEqual({ coarse: true, hover: false });
  await page.evaluate(() => {
    document.addEventListener(
      "pointerdown",
      (event: PointerEvent): void => {
        document.documentElement.dataset["trustedTouch"] = String(event.isTrusted && event.pointerType === "touch");
      },
      { once: true, capture: true },
    );
  });
  await page.locator("#mobile-publish").tap();
  await expect(page.locator("html")).toHaveAttribute("data-trusted-touch", "true");
  const dialog = page.getByRole("dialog", { name: "Publish", exact: true });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Cancel", exact: true }).tap();
  await expect(dialog).toBeHidden();
  await page.locator('[data-mobile-drawer="metadata"]').tap();
  await expect(page.locator("#notes")).toBeVisible();
  await page.locator('[data-mobile-drawer="metadata"]').tap();
  await page.locator("#main-image").tap();
  await expect(page.locator("#zoom-full")).toBeHidden();
  await expect(page.locator("#zoom-loupe")).toBeHidden();
  await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
  expect(harness.requests.filter((request) => request.name === "review")).toHaveLength(0);
  expect(harness.errors).toEqual([]);
});
