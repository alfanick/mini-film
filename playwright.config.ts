// Exercise the production bundle in both browser engines used by the review UI.
// Keep traces and screenshots under Cargo's ignored build-output directory.
import { defineConfig } from "@playwright/test";

const pureTests = ["**/geometry.spec.ts", "**/reconcile.spec.ts", "**/session.spec.ts", "**/*.unit.spec.ts"];
const legacy = process.env["REVIEW_LEGACY"] === "1";

export default defineConfig({
  testDir: "frontend/tests",
  outputDir: "target/playwright-results",
  fullyParallel: true,
  workers: process.env["CI"] ? 2 : 4,
  forbidOnly: Boolean(process.env["CI"]),
  retries: process.env["CI"] ? 1 : 0,
  snapshotPathTemplate: "{testDir}/../../target/review-visual/{projectName}/{arg}{ext}",
  updateSnapshots: "none",
  expect: { toHaveScreenshot: { animations: "disabled", scale: "css", threshold: 0.02, maxDiffPixels: 80 } },
  reporter: "list",
  use: {
    baseURL: "http://127.0.0.1:4178/nested/review/",
    viewport: { width: 1600, height: 1000 },
    colorScheme: "dark",
    locale: "en-US",
    timezoneId: "UTC",
    trace: "retain-on-failure",
  },
  projects: [
    { name: "pure", testMatch: pureTests },
    {
      name: "chromium-debug",
      testIgnore: pureTests,
      snapshotPathTemplate: "{testDir}/../../target/review-visual/chromium/{arg}{ext}",
      use: { browserName: "chromium" },
    },
    {
      name: "webkit-debug",
      testIgnore: pureTests,
      snapshotPathTemplate: "{testDir}/../../target/review-visual/webkit/{arg}{ext}",
      use: { browserName: "webkit" },
    },
    ...(legacy
      ? []
      : [
          {
            name: "chromium-release",
            testIgnore: pureTests,
            snapshotPathTemplate: "{testDir}/../../target/review-visual/chromium/{arg}{ext}",
            use: { browserName: "chromium" as const, baseURL: "http://127.0.0.1:4178/nested/review-release/" },
          },
          {
            name: "webkit-release",
            testIgnore: pureTests,
            snapshotPathTemplate: "{testDir}/../../target/review-visual/webkit/{arg}{ext}",
            use: { browserName: "webkit" as const, baseURL: "http://127.0.0.1:4178/nested/review-release/" },
          },
        ]),
  ],
  webServer: {
    command: legacy
      ? "node frontend/tests/server.mjs"
      : "npm run build:review && npm run build:review -- --profile release --out-dir target/review-release" +
        " && node frontend/tests/server.mjs",
    url: "http://127.0.0.1:4178/nested/review/",
    reuseExistingServer: false,
    timeout: 240_000,
  },
});
