/** Run pure contracts, models, and geometry without building a bundle or starting a listening server. */
import { defineConfig } from "@playwright/test";
import { pureTestPatterns } from "./frontend/tests/configuration";

export default defineConfig({
  testDir: "frontend/tests",
  testMatch: pureTestPatterns,
  outputDir: "target/playwright-pure-results",
  fullyParallel: true,
  workers: process.env["CI"] ? 2 : 4,
  forbidOnly: Boolean(process.env["CI"]),
  retries: 0,
  reporter: "list",
});
