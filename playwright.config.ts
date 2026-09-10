// Exercise the production bundle in both browser engines used by the review UI.
// Keep traces and screenshots under Cargo's ignored build-output directory.
import { defineConfig } from "@playwright/test";
import { pureTestPatterns } from "./frontend/tests/configuration";

const desktopIgnored = [
  ...pureTestPatterns,
  "**/touch.spec.ts",
  "**/touch-gestures.spec.ts",
  "**/*.integration.spec.ts",
];
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
    {
      name: "chromium-debug",
      testIgnore: desktopIgnored,
      snapshotPathTemplate: "{testDir}/../../target/review-visual/chromium/{arg}{ext}",
      use: { browserName: "chromium" },
    },
    {
      name: "webkit-debug",
      testIgnore: desktopIgnored,
      snapshotPathTemplate: "{testDir}/../../target/review-visual/webkit/{arg}{ext}",
      use: { browserName: "webkit" },
    },
    ...(legacy
      ? []
      : [
          {
            name: "chromium-release",
            testIgnore: desktopIgnored,
            snapshotPathTemplate: "{testDir}/../../target/review-visual/chromium/{arg}{ext}",
            use: { browserName: "chromium" as const, baseURL: "http://127.0.0.1:4178/nested/review-release/" },
          },
          {
            name: "webkit-release",
            testIgnore: desktopIgnored,
            snapshotPathTemplate: "{testDir}/../../target/review-visual/webkit/{arg}{ext}",
            use: { browserName: "webkit" as const, baseURL: "http://127.0.0.1:4178/nested/review-release/" },
          },
        ]),
    ...(["chromium", "webkit"] as const).flatMap((browserName) =>
      (legacy ? ["debug"] : ["debug", "release"]).map((profile) => ({
        name: `${browserName}-touch-${profile}`,
        testMatch:
          browserName === "chromium" ? ["**/touch.spec.ts", "**/touch-gestures.spec.ts"] : ["**/touch.spec.ts"],
        use: {
          browserName,
          viewport: { width: 390, height: 844 },
          hasTouch: true,
          isMobile: true,
          baseURL: `http://127.0.0.1:4178/nested/review${profile === "release" ? "-release" : ""}/`,
        },
      })),
    ),
  ],
  webServer: {
    command: legacy
      ? "node frontend/tests/server.mts"
      : "npm run build:review && npm run build:review -- --profile release --out-dir target/review-release" +
        " && node frontend/tests/server.mts",
    url: "http://127.0.0.1:4178/nested/review/",
    reuseExistingServer: false,
    timeout: 240_000,
  },
});
