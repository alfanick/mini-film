/**
 * Mount the review Preact application once; Cargo bundles this entry and its imports into embedded app.js.
 * The daemon's HTML needs no runtime module loader, package manager or external asset server.
 */
import { render } from "preact";
import { ReviewProvider } from "./core/context";
import { ReviewApp } from "./ReviewApp";
import { ErrorBoundary } from "./components/ErrorBoundary";

const root = document.getElementById("review-root");
if (!root) throw new Error("Review application mount is missing");
render(
  <ReviewProvider>
    <ErrorBoundary>
      <ReviewApp />
    </ErrorBoundary>
  </ReviewProvider>,
  root,
);
