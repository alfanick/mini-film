/** Exercise the real Rust listener and embedded bundle independently from the fixture server and image engines. */
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "frontend/tests",
  testMatch: "**/*.integration.spec.ts",
  outputDir: "target/playwright-daemon-results",
  workers: 1,
  timeout: 120_000,
  forbidOnly: Boolean(process.env["CI"]),
  retries: 0,
  use: { browserName: "chromium", viewport: { width: 1600, height: 1000 }, trace: "retain-on-failure" },
});
