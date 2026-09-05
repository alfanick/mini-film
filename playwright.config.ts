// Exercise the production bundle in both browser engines used by the review UI.
// Keep traces and screenshots under Cargo's ignored build-output directory.
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "frontend/tests",
  outputDir: "target/playwright-results",
  fullyParallel: true,
  workers: process.env.CI ? 2 : undefined,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
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
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  webServer: {
    command:
      process.env.REVIEW_LEGACY === "1"
        ? "node frontend/tests/server.mjs"
        : "npm run build:review && node frontend/tests/server.mjs",
    url: "http://127.0.0.1:4178/nested/review/",
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
