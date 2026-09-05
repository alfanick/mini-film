// Serve the actual embedded shell and bundle without starting image-processing
// services. Browser tests supply deterministic HTTP state and SSE messages.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const legacy = process.env.REVIEW_LEGACY === "1";
const assets = resolve(root, legacy ? "target/review-baseline" : "assets/review");
const bundle = resolve(root, legacy ? "target/review-baseline/app.js" : "target/review-frontend/review/app.js");
const releaseBundle = resolve(root, "target/review-release/review/app.js");
const image = [
  '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800">',
  '<rect width="1200" height="800" fill="#50657c"/>',
  '<path d="M0 650L360 240L700 650L930 370L1200 650V800H0" fill="#b8b39b"/>',
  '<circle cx="960" cy="180" r="75" fill="#e9d39b"/>',
  "</svg>",
].join("");

// Resolve only allowlisted assets so the fixture server cannot expose the checkout.
const server = createServer(async (request, response) => {
  const url = new URL(request.url, "http://localhost");
  const release = url.pathname.startsWith("/nested/review-release/");
  const path = url.pathname.replace(/^\/nested\/review(?:-release)?\//, "");
  let file;
  let type;
  if (path === "" || path === "index.html") {
    file = resolve(assets, "index.html");
    type = "text/html";
  } else if (path === "assets/app.js") {
    file = release ? releaseBundle : bundle;
    type = "application/javascript";
  } else if (path === "assets/styles.css") {
    file = resolve(assets, "styles.css");
    type = "text/css";
  } else if (legacy && /^assets\/vendor\/[a-z.]+$/.test(path)) {
    file = resolve(assets, path.slice("assets/".length));
    type = "application/javascript";
  } else if (path.startsWith("fixture/")) {
    response.writeHead(200, { "Content-Type": "image/svg+xml" });
    response.end(image);
    return;
  } else {
    response.writeHead(404);
    response.end("not found");
    return;
  }
  try {
    const contents = await readFile(file);
    response.writeHead(200, { "Content-Type": type });
    response.end(contents);
  } catch (error) {
    response.writeHead(500);
    response.end(error.message);
  }
});
server.listen(4178, "127.0.0.1");
