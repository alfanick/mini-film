/** Reactive publish views render controlled tool state; stable component identities preserve focus and open details. */
import { createContext } from "preact";
import { useContext } from "preact/hooks";
import type { ToolsController } from "../tools/use-tools";
import type { ComponentChildren } from "preact";

import type { ReviewPublishJob } from "../core/types";

/** Controlled publish values and derived job/selection state required by its form components. */
export interface PublishViewDependencies {
  publishOpen: ToolsController["publishOpen"];
  publishForm: ToolsController["publishForm"];
  publishSubmitting: ToolsController["publishSubmitting"];
  publishError: ToolsController["publishError"];
  publishJob: ToolsController["publishJob"];
  publishRerender: ToolsController["publishRerender"];
  publishStats: ToolsController["publishStats"];
  togglePublishWizard: ToolsController["togglePublishWizard"];
  setPublishField: ToolsController["setPublishField"];
  togglePublishLabel: ToolsController["togglePublishLabel"];
  submitPublish: ToolsController["submitPublish"];
  COLOR_LABELS: typeof import("../core/constants").COLOR_LABELS;
  RATING_VALUES: typeof import("../core/constants").RATING_VALUES;
  capitalize: typeof import("../core/selectors").capitalize;
  publishProgressPercent: typeof import("../tools/publish-helpers").publishProgressPercent;
  reviewUrl: typeof import("../core/api").reviewUrl;
}

export const PublishViewContext = createContext<PublishViewDependencies | null>(null);

/** Read the current dialog dependencies from its provider instead of retaining initial factory closures. */
function usePublishView(): PublishViewDependencies {
  const value = useContext(PublishViewContext);
  if (!value) throw new Error("Publish views require their tool provider");
  return value;
}

/** Render publish overlay from current state and typed callbacks. */
export function PublishOverlay(): ComponentChildren {
  const {
    publishOpen,
    publishRerender,
    publishStats,
    togglePublishWizard,
    publishSubmitting,
    publishError,
    publishJob,
    submitPublish,
  } = usePublishView();
  return (
    <div
      id={"publish-overlay"}
      class={"publish-overlay"}
      role={"dialog"}
      aria-modal={"true"}
      aria-labelledby={"publish-title"}
      hidden={!publishOpen}
      onClick={(event) => {
        if (event.target === event.currentTarget) togglePublishWizard(false);
      }}
    >
      <form
        id={"publish-form"}
        class={"publish-card"}
        onSubmit={(event) => {
          event.preventDefault();
          void submitPublish();
        }}
      >
        <header class={"publish-header"}>
          <div>
            <h2 id={"publish-title"}>{"Publish"}</h2>
            <p id={"publish-mode"}>
              {publishRerender
                ? "Changed output or grain settings will rerender selected pictures from the original RAW files."
                : "Settings match daemon defaults, so publish will hardlink reviewed outputs when possible."}
            </p>
            <p id={"publish-count"} class={"publish-count"}>
              {`${publishStats.pictures} ${publishStats.pictures === 1 ? "picture" : "pictures"} selected, ` +
                `${publishStats.outputs} ${publishStats.outputs === 1 ? "output" : "outputs"} will be exported.`}
            </p>
          </div>
          <button id={"publish-cancel"} type={"button"} onClick={() => togglePublishWizard(false)}>
            {"Cancel"}
          </button>
        </header>
        <div class={"publish-grid"}>
          <PublishSelectionSection />
          <PublishOutputSection />
          <PublishGallerySection />
        </div>
        <footer class={"publish-footer"}>
          <div id={"publish-status"} class={"publish-status"}>
            {publishError ||
              (publishJob ? (
                <PublishStatus job={publishJob} />
              ) : publishRerender ? (
                "Changed output settings will rerender from original RAWs."
              ) : (
                "Default settings will link existing reviewed outputs."
              ))}
          </div>
          <button
            id={"publish-submit"}
            type={"submit"}
            disabled={publishSubmitting || publishJob?.status === "running"}
          >
            {"Start publish job"}
          </button>
        </footer>
      </form>
    </div>
  );
}

/** Render publish selection section from current state and typed callbacks. */
export function PublishSelectionSection(): ComponentChildren {
  const { COLOR_LABELS, RATING_VALUES, capitalize, publishForm, setPublishField, togglePublishLabel } =
    usePublishView();
  return (
    <section class={"publish-section"}>
      <h3>{"Selection"}</h3>
      <label>
        <span>{"Output folder"}</span>
        <input
          id={"publish-album"}
          value={publishForm.album}
          onInput={(event) => setPublishField("album", event.currentTarget.value)}
          type={"text"}
          autocomplete={"off"}
        />
      </label>
      <label>
        <span>{"Rating >="}</span>
        <select
          id={"publish-min-rating"}
          value={publishForm.minRating}
          onChange={(event) => setPublishField("minRating", event.currentTarget.value)}
        >
          {RATING_VALUES.map((rating) => (
            <option key={rating} value={String(rating)}>
              {String(rating)}
            </option>
          ))}
        </select>
      </label>
      <div class={"publish-labels"}>
        <span>{"Colour labels"}</span>
        {COLOR_LABELS.map((label) => (
          <label key={label} data-publish-label={label} title={capitalize(label)}>
            <input
              type={"checkbox"}
              name={"publish-label"}
              value={label}
              checked={publishForm.labels.includes(label)}
              onChange={(event) => togglePublishLabel(label, event.currentTarget.checked)}
            />{" "}
            <span>{capitalize(label)}</span>
          </label>
        ))}
      </div>
      <label>
        <span>{"Tags"}</span>
        <input
          id={"publish-tags"}
          value={publishForm.tags}
          onInput={(event) => setPublishField("tags", event.currentTarget.value)}
          type={"text"}
          autocomplete={"off"}
          placeholder={"optional comma-separated tags"}
        />
      </label>
      <label>
        <input
          id={"publish-main-profile-only"}
          checked={publishForm.mainProfileOnly}
          onChange={(event) => setPublishField("mainProfileOnly", event.currentTarget.checked)}
          type={"checkbox"}
        />
        {" First/main profile only (exclude SOOC)"}
      </label>
    </section>
  );
}

/** Render publish output section from current state and typed callbacks. */
export function PublishOutputSection(): ComponentChildren {
  const { publishForm, setPublishField } = usePublishView();
  return (
    <section class={"publish-section"}>
      <h3>{"Output"}</h3>
      <label>
        <span>{"Format"}</span>
        <select
          id={"publish-output-format"}
          value={publishForm.outputFormat}
          onChange={(event) => setPublishField("outputFormat", event.currentTarget.value)}
        >
          <option value={"jpg"}>{"JPG"}</option>
          <option value={"tiff"}>{"TIFF"}</option>
        </select>
      </label>
      <label>
        <span>{"Grain engine"}</span>
        <select
          id={"publish-grain-engine"}
          value={publishForm.grainEngine}
          onChange={(event) => setPublishField("grainEngine", event.currentTarget.value)}
        >
          <option value={"legacy"}>{"Legacy"}</option>
          <option value={"rfgrfast"}>{"RFGR fast"}</option>
          <option value={"rfgr"}>{"RFGR"}</option>
        </select>
      </label>
      <label>
        <span>{"Grain reference MPix"}</span>
        <input
          id={"publish-normalize-grain-mpix"}
          disabled={!publishForm.normalizeGrain}
          value={publishForm.normalizeGrainMpix}
          onInput={(event) => setPublishField("normalizeGrainMpix", event.currentTarget.value)}
          type={"number"}
          min={"5e-324"}
          step={"any"}
          required={true}
        />
      </label>
      <label>
        <span>{"Size"}</span>
        <select
          id={"publish-size-mode"}
          value={publishForm.sizeMode}
          onChange={(event) => setPublishField("sizeMode", event.currentTarget.value)}
        >
          <option value={"original"}>{"Original size"}</option>
          <option value={"long-edge"}>{"Long edge"}</option>
          <option value={"bounds"}>{"Max width/height"}</option>
          <option value={"geometry"}>{"Convert geometry"}</option>
        </select>
      </label>
      <div class={"publish-sizes"}>
        <input
          id={"publish-long-edge"}
          hidden={publishForm.sizeMode !== "long-edge"}
          value={publishForm.longEdge}
          onInput={(event) => setPublishField("longEdge", event.currentTarget.value)}
          type={"number"}
          min={"1"}
          placeholder={"long edge px"}
        />
        <input
          id={"publish-max-width"}
          hidden={publishForm.sizeMode !== "bounds"}
          value={publishForm.maxWidth}
          onInput={(event) => setPublishField("maxWidth", event.currentTarget.value)}
          type={"number"}
          min={"1"}
          placeholder={"max width"}
        />
        <input
          id={"publish-max-height"}
          hidden={publishForm.sizeMode !== "bounds"}
          value={publishForm.maxHeight}
          onInput={(event) => setPublishField("maxHeight", event.currentTarget.value)}
          type={"number"}
          min={"1"}
          placeholder={"max height"}
        />
        <input
          id={"publish-resize"}
          hidden={publishForm.sizeMode !== "geometry"}
          value={publishForm.resize}
          onInput={(event) => setPublishField("resize", event.currentTarget.value)}
          type={"text"}
          placeholder={"e.g. 3840x3840>"}
        />
      </div>
      <label>
        <span>{"JPG quality"}</span>
        <input
          id={"publish-jpg-quality"}
          value={publishForm.jpgQuality}
          onInput={(event) => setPublishField("jpgQuality", event.currentTarget.value)}
          type={"number"}
          min={"1"}
          max={"100"}
        />
      </label>
      <label>
        <span>{"Subsampling"}</span>
        <select
          id={"publish-jpeg-subsampling"}
          value={publishForm.jpegSubsampling}
          onChange={(event) => setPublishField("jpegSubsampling", event.currentTarget.value)}
        >
          <option value={"s444"}>{"4:4:4"}</option>
          <option value={"s422"}>{"4:2:2"}</option>
          <option value={"s420"}>{"4:2:0"}</option>
        </select>
      </label>
      <div class={"publish-checks"}>
        <label>
          <input
            id={"publish-normalize-grain"}
            checked={publishForm.normalizeGrain}
            onChange={(event) => setPublishField("normalizeGrain", event.currentTarget.checked)}
            type={"checkbox"}
          />
          {" Normalize grain"}
        </label>
        <label>
          <input
            id={"publish-progressive"}
            checked={publishForm.progressive}
            onChange={(event) => setPublishField("progressive", event.currentTarget.checked)}
            type={"checkbox"}
          />
          {" Progressive JPG"}
        </label>
        <label>
          <input
            id={"publish-strip-metadata"}
            checked={publishForm.stripMetadata}
            onChange={(event) => setPublishField("stripMetadata", event.currentTarget.checked)}
            type={"checkbox"}
          />
          {" Strip metadata"}
        </label>
      </div>
    </section>
  );
}

/** Render publish gallery section from current state and typed callbacks. */
export function PublishGallerySection(): ComponentChildren {
  const { publishForm, setPublishField } = usePublishView();
  return (
    <section class={"publish-section publish-section-wide"}>
      <h3>{"Gallery"}</h3>
      <label>
        <span>{"Template"}</span>
        <select
          id={"publish-gallery"}
          value={publishForm.gallery}
          onChange={(event) => setPublishField("gallery", event.currentTarget.value)}
        >
          {[
            ["none", "No gallery"],
            ["modern", "Modern"],
            ["soft", "Soft"],
            ["compact", "Compact"],
            ["hero", "Hero"],
            ["phone", "Phone"],
            ["all", "All templates"],
          ].map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>{"Columns"}</span>
        <input
          id={"publish-gallery-columns"}
          value={publishForm.galleryColumns}
          onInput={(event) => setPublishField("galleryColumns", event.currentTarget.value)}
          type={"number"}
          min={"1"}
          max={"12"}
        />
      </label>
      <label>
        <span>{"Thumbnail edge"}</span>
        <input
          id={"publish-gallery-thumbnail-long-edge"}
          value={publishForm.galleryThumbnailLongEdge}
          onInput={(event) => setPublishField("galleryThumbnailLongEdge", event.currentTarget.value)}
          type={"number"}
          min={"1"}
        />
      </label>
    </section>
  );
}

/** Render publish status from current state and typed callbacks. */
export function PublishStatus({ job }: { job: ReviewPublishJob }): ComponentChildren {
  const { publishProgressPercent, reviewUrl } = usePublishView();
  if (job.status === "running") {
    const percent = publishProgressPercent(job);
    return (
      <div>
        <div>
          {`Publishing ${job.album}: ${percent}% ${job.step || "publish"}${job.current ? ` | ${job.current}` : ""}`}
        </div>
        <div class={"publish-progress"}>
          <span style={{ width: `${percent}%` }} />
        </div>
        <div class={"publish-progress-counts"}>
          {`${job.processed || 0}/${job.total || 0} outputs | linked ${job.linked || 0} | ` +
            `skipped ${job.skipped || 0} | galleries ${job.galleries || 0}`}
        </div>
      </div>
    );
  }
  if (job.status === "done") {
    const links = Array.isArray(job.gallery_urls) ? job.gallery_urls : [];
    const galleryLinks = links.map((link) => (link.startsWith("/") ? link : `/${link}`));
    return (
      <div>
        {`Published ${job.linked} files to ${job.album}; skipped ${job.skipped}; galleries ${job.galleries}.`}
        {galleryLinks.length ? (
          <div class={"publish-galleries"}>
            <div>{"Gallery links:"}</div>
            <div class={"publish-gallery-list"}>
              {galleryLinks.map((link, index) => (
                <div key={link} class={"publish-gallery-row"}>
                  <a href={link} target={"_blank"} rel={"noopener noreferrer"} class={"publish-gallery-link"}>
                    {galleryLinks.length > 1 ? `Gallery ${index + 1}` : "Open gallery"}
                  </a>
                  <a href={reviewUrl(`api/publish/${job.id}/gallery.zip`)} download={""} class={"publish-gallery-link"}>
                    {"Download gallery"}
                  </a>
                </div>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    );
  }
  return `Publish failed: ${job.error || "unknown error"}`;
}
