/**
 * Compose the review workspace from state-driven Preact features.
 * The shell preserves embedded UI structure while hooks own requests, edits and browser capabilities.
 */
import { Fragment, type ComponentChildren } from "preact";
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { useReviewContext, useReviewModel } from "./core/context";
import { COLOR_LABELS, RATING_VALUES } from "./core/constants";
import {
  capitalize,
  currentImage,
  defaultRetouch,
  isCompressedImage,
  isDirectCompressedImage,
  isSoocProfile,
  plural,
  profileDisplayName,
  profileDisplayState,
  profilesAreImplicitOnly,
  selectedProfile,
} from "./core/selectors";
import type { ReviewStateData } from "./core/types";
import { Controls } from "./components/Controls";
import { BwFilterControls, ImageList, ProfileList } from "./components/Browsing";
import { formatImageExif, imageSourceInfoTitle } from "./components/formatting";
import { ShortcutsOverlay } from "./components/Shortcuts";
import { useReviewSession, type ReviewActions, type ReviewSession } from "./session/use-session";
import { useReviewEdits, type ReviewEdits } from "./session/use-edits";
import { useMediaQuery, usePanelSafeArea } from "./session/use-layout";
import { useReviewShortcuts } from "./session/use-shortcuts";
import { useOriginalShare } from "./session/use-original-share";
import { Viewer } from "./viewer/Viewer";
import { ToolsProvider, useActiveTools, ToolOverlayHost } from "./tools/context";
import { publishProgressPercent } from "./features/publish/helpers";

/** Keep live pipeline progress phrased like the original compact sidebar summary. */
function statusSummary(data: ReviewStateData | null, count: number, profileCount: number): string {
  if (!data) return "Connecting...";
  const codex = data.codex;
  let codexText = "";
  if (codex.enabled) {
    if (codex.processing > 0)
      codexText = `codex analyzing ${codex.processing}${codex.queued > 0 ? ` queued ${codex.queued}` : ""}`;
    else if (codex.queued > 0) codexText = `codex queued ${codex.queued}`;
    else if (codex.failed > 0) codexText = `codex failed ${codex.failed}`;
  }
  const job = data.publish_jobs[data.publish_jobs.length - 1];
  const publishText = !job
    ? ""
    : job.status === "running"
      ? `publishing ${job.album} ${publishProgressPercent(job)}% ${job.step || "publish"}` +
        (job.current ? ` | ${job.current}` : "")
      : job.status === "done"
        ? `published ${job.linked} files`
        : "publish failed";
  return (
    `${count}/${data.images.length} pictures | ${profileCount} ${plural(profileCount, "profile")} | ` +
    `${data.client_count} ${plural(data.client_count, "client")}${codexText ? ` | ${codexText}` : ""}` +
    (publishText ? ` | ${publishText}` : "")
  );
}

/** Mount the single review application with stable feature components and controlled forms. */
export function ReviewApp(): ComponentChildren {
  const session = useReviewSession();
  return (
    <ToolsProvider session={session}>
      <ReviewWorkspace session={session} />
    </ToolsProvider>
  );
}

/** Tool forms own a separate render boundary; the workspace reads only catalog and visible-shell state. */
function ReviewWorkspace({ session }: { session: ReviewSession }): ComponentChildren {
  const model = useReviewModel();
  const { state, update } = useReviewContext([
    "data",
    "currentId",
    "labelFilters",
    "cropEditing",
    "mobileDrawer",
    "informationOpen",
    "histogramOpen",
    "profileInfoProfileIndex",
    "commandInvocationOpen",
    "diffusionOpen",
    "samplerOpen",
    "panoramaOpen",
    "localRetouchDirty",
  ]);
  const editSession = useReviewEdits(session);
  const tools = useActiveTools();
  const image = currentImage(state);
  const selected = selectedProfile(image, state);
  const direct = isDirectCompressedImage(image);
  const compressed = isCompressedImage(image);
  const hideProfiles = direct || profilesAreImplicitOnly(state, image);
  const profileCount = profilesAreImplicitOnly(state, image) ? 0 : image?.profiles.length || 0;
  const configuredProfileCount = profilesAreImplicitOnly(state, null) ? 0 : state.data?.profiles.length || 0;
  const disabled = !image || direct || isSoocProfile(selected);
  const [feedback, setFeedback] = useState<{ text: string; sequence: number } | null>(null);
  /** Restart the viewer's transient feedback even when consecutive actions use the same caption. */
  const showFeedback = (text: string): void =>
    setFeedback((previous) => ({ text, sequence: (previous?.sequence || 0) + 1 }));
  const edits: ReviewEdits = {
    ...editSession,
    copy: (): void => {
      if (!disabled) {
        editSession.copy();
        showFeedback("copied sliders");
      }
    },
    paste: (): void => {
      if (!disabled && editSession.clipboard) {
        editSession.paste();
        showFeedback("pasted sliders");
      }
    },
  };
  const images = model.visibleImages.value;
  const mobile = useMediaQuery("(max-width: 600px), (max-width: 950px) and (max-height: 520px)");
  const rail = useMediaQuery("(min-width: 901px) and (min-height: 620px)");
  const tuckedRail = useMediaQuery("(min-width: 901px) and (min-height: 620px) and (max-width: 1499.98px)");
  const [shortcutsOpen, setShortcutsOpen] = useState<boolean>(false);
  const [railOpen, setRailOpen] = useState<boolean>(false);
  const [cropReady, setCropReady] = useState<boolean>(false);
  const workspaceRef = useRef<HTMLElement>(null);
  const appRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const tagsRef = useRef<HTMLInputElement>(null);
  const notesRef = useRef<HTMLInputElement>(null);
  const panelSafe = usePanelSafeArea(workspaceRef, panelRef, `${mobile}:${rail}:${state.mobileDrawer}:${cropReady}`);
  const original = useOriginalShare(image, showFeedback);
  const originalTitle = compressed
    ? `${original.label === "Open Photo" ? "Open" : "Save"} original ${image?.file_name || "photo"}`
    : undefined;

  // Actions capture identities synchronously; their session-owned queue then flushes that picture's draft.
  const actions = session.actions;
  const toggleProfile = useCallback<ReviewActions["toggleProfile"]>(
    (profile) => actions.toggleProfile(profile),
    [actions],
  );
  const soloProfile = useCallback<ReviewActions["toggleProfile"]>(
    (profile) => actions.toggleProfile(profile, true),
    [actions],
  );
  const onWheel = useReviewShortcuts({
    actions,
    edits,
    tools,
    shortcutsOpen,
    setShortcutsOpen,
    tagsRef,
    notesRef,
    mobile,
    appRef,
  });
  useEffect(() => {
    if ((!mobile && state.mobileDrawer) || (direct && state.mobileDrawer === "retouch")) update({ mobileDrawer: null });
  }, [mobile, direct, state.mobileDrawer, update]);
  useEffect(() => {
    update({ cropEditing: false });
  }, [image?.id, update]);
  useEffect((): void => setRailOpen(false), [tuckedRail]);

  const drawer = mobile ? state.mobileDrawer : null;
  const tagsCount = image?.tags.length || 0;
  const metadataTitle = `${tagsCount} ${plural(tagsCount, "tag")}${image?.notes ? ", notes present" : ""}`;
  const retouchActive = JSON.stringify(edits.retouch) !== JSON.stringify(defaultRetouch());
  const cropAdjusted =
    state.cropEditing || Boolean(edits.retouch.crop) || Math.abs(edits.retouch.rotation_degrees) > 0.001;
  const diffusionSettings = selected?.diffusion?.settings || selected?.diffusion_settings;
  const diffusionActive =
    !disabled && Boolean(diffusionSettings && (diffusionSettings.softness > 0 || diffusionSettings.highlight_glow > 0));
  const profile = state.data?.profiles.find((item) => item.index === selected?.profile_index) || null;
  const exif = formatImageExif(image);
  const display = profileDisplayState(image, selected, state.localRetouchDirty);
  const codexStatus = image?.codex.status;
  const codexText =
    codexStatus === "processing"
      ? "Codex analyzing"
      : codexStatus === "queued"
        ? "Codex queued"
        : codexStatus === "failed"
          ? "Codex failed"
          : "";
  const suffix =
    `${display.text}${selected?.url || direct ? "" : image?.preview_url ? " | camera preview" : ""}` +
    (codexText ? ` | ${codexText}` : "");
  const correctionText = [selected?.dcp_profile_filename ? "DCP" : "", selected?.lcp_profile_filename ? "LCP" : ""]
    .filter(Boolean)
    .join(" + ");
  const correctionTitle = [
    selected?.dcp_profile_filename ? `DCP: ${selected.dcp_profile_filename}` : "",
    selected?.lcp_profile_filename ? `LCP: ${selected.lcp_profile_filename}` : "",
  ]
    .filter(Boolean)
    .join("; ");
  const classes = [
    "app",
    cropReady ? "crop-mode" : "",
    direct ? "compressed-image" : "",
    isSoocProfile(selected) ? "sooc-profile-selected" : "",
    drawer ? `mobile-drawer-open mobile-drawer-${drawer}` : "",
  ]
    .filter(Boolean)
    .join(" ");
  const liveClass = ["live-dot", session.connected ? "connected" : "", session.keepalive.tick ? "keepalive-pulse" : ""]
    .filter(Boolean)
    .join(" ");
  const shortcutsBlocked =
    state.commandInvocationOpen ||
    state.profileInfoProfileIndex !== null ||
    state.diffusionOpen ||
    state.samplerOpen ||
    state.panoramaOpen ||
    tools.publishOpen ||
    shortcutsOpen;
  const profileList = (
    <div
      id="profiles"
      class={`profiles${railOpen ? " peek-open" : ""}`}
      hidden={hideProfiles}
      onPointerDown={(event): void => {
        if (!tuckedRail || event.pointerType === "mouse" || railOpen) return;
        setRailOpen(true);
        event.preventDefault();
        event.stopPropagation();
      }}
    >
      <ProfileList
        image={image}
        onSelect={actions.selectProfile}
        onToggleEnabled={toggleProfile}
        onSolo={soloProfile}
      />
    </div>
  );
  return (
    <>
      <div class={classes} ref={appRef}>
        <aside class="sidebar">
          <header class="sidebar-header">
            <div>
              <h1>Review</h1>
              <button
                id="app-version"
                class="app-version"
                type="button"
                onClick={(event): void => {
                  event.preventDefault();
                  tools.openCommandInvocation();
                }}
              >
                {`mini-film ${state.data?.version || ""}`.trim()}
              </button>
            </div>
            <div class="header-actions">
              <button
                id="shortcuts-help"
                class="help-button"
                type="button"
                aria-label="Keyboard shortcuts"
                onClick={(): void => setShortcutsOpen(true)}
              >
                ?
              </button>
              <button
                id="publish"
                class="publish-button"
                type="button"
                title="Publish"
                aria-label="Publish"
                onClick={(): void => tools.togglePublishWizard()}
              >
                Pub
              </button>
            </div>
          </header>
          <div class="filter">
            <label>
              <span>Rating</span>
              <select
                id="min-rating"
                value={String(state.data?.ui.min_rating || 0)}
                onChange={(event): void => {
                  void session.updateSharedUi({ min_rating: Number(event.currentTarget.value) }).catch(console.error);
                }}
              >
                {RATING_VALUES.map((rating: number): ComponentChildren => (
                  <option key={rating} value={String(rating)}>
                    {rating}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>Colour</span>
              <select
                id="filter-label"
                aria-label="Colour label filter"
                value={Array.from(state.labelFilters)[0] || ""}
                onChange={(event): void => {
                  const value = COLOR_LABELS.find((label): boolean => label === event.currentTarget.value);
                  void session.updateSharedUi({ labels: value ? [value] : [] }).catch(console.error);
                }}
              >
                <option value="">Any</option>
                {COLOR_LABELS.map((label): ComponentChildren => (
                  <option key={label} value={label}>
                    {capitalize(label)}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <div class="status">
            <span
              key={session.keepalive.tick}
              id="live-dot"
              class={liveClass}
              title={session.keepalive.title || (session.connected ? "Connected" : "Connecting...")}
            />
            <span id="status">
              {session.connectionError || statusSummary(state.data, images.length, configuredProfileCount)}
            </span>
          </div>
          {edits.errors.map((error) => (
            <div key={error.imageId} class="save-error" role="status" aria-live="polite">
              {`Picture ${error.imageId}: ${error.message}. Edits are still unsaved. `}
              <button
                type="button"
                onClick={(): void => {
                  void edits.retry(error.imageId).catch(() => undefined);
                }}
              >
                Retry save
              </button>
            </div>
          ))}
          {session.reviewFailures.map((failure): ComponentChildren => (
            <div key={failure.key} class="save-error" role="status" aria-live="polite">
              {`Picture ${failure.imageId}: ${failure.message}. Save could not be confirmed. `}
              <button
                type="button"
                onClick={(): void => {
                  void session.recover(failure);
                }}
              >
                {failure.retryable ? "Refresh and retry" : "Check state"}
              </button>
            </div>
          ))}
          <div id="image-list" class="image-list">
            <ImageList
              images={images}
              bursts={state.data?.bursts || []}
              currentId={state.currentId}
              onSelect={actions.selectImage}
              onToggleBurst={actions.toggleBurst}
            />
          </div>
          <section class="sidebar-tools" aria-label="Tools">
            <div class="sidebar-tools-title">Tools</div>
            <button
              id="crop-toggle"
              class={`sidebar-tool-button${cropAdjusted ? " active" : ""}`}
              type="button"
              disabled={disabled}
              title={cropAdjusted ? "Crop or rotation adjustment active" : "Crop/rotate"}
              onClick={(): void => {
                if (image?.crop_source_url || selected?.base_url || image?.preview_url || selected?.url)
                  update({ cropEditing: true });
              }}
            >
              Crop/rotate
            </button>
            <button
              id="diffusion"
              class={`sidebar-tool-button${diffusionActive ? " active" : ""}`}
              type="button"
              title={diffusionActive ? "Diffusion applied" : "Open diffusion tools"}
              aria-label={diffusionActive ? "Open diffusion tools, diffusion applied" : "Open diffusion tools"}
              disabled={!image || direct || !selected || isSoocProfile(selected)}
              onClick={(): void => tools.openDiffusion()}
            >
              Diffusion
            </button>
            <button
              id="sampler"
              class="sidebar-tool-button"
              type="button"
              title="Open profile sampler"
              aria-label="Open profile sampler"
              hidden={!state.data?.capabilities.sampler}
              disabled={!image}
              onClick={(): void => {
                void tools.openSampler().catch(console.error);
              }}
            >
              Sampler
            </button>
            <button
              id="panorama"
              class="sidebar-tool-button"
              type="button"
              title="Create panorama"
              aria-label="Create panorama"
              hidden={!state.data?.capabilities.panorama.available}
              onClick={(): void => tools.openPanoramaWizard()}
            >
              Panorama
            </button>
          </section>
        </aside>
        <main
          class="workspace"
          ref={workspaceRef}
          style={{ "--review-panel-safe": `${panelSafe}px` }}
          onWheel={onWheel}
          onPointerDown={(event): void => {
            if (tuckedRail && event.target instanceof Element && event.target.id === "main-image") setRailOpen(false);
          }}
        >
          <Viewer
            feedback={feedback}
            onCropReadyChange={setCropReady}
            shortcutsBlocked={shortcutsBlocked}
            image={image}
            selected={selected}
            retouch={edits.retouch}
            onRetouch={(value): void => {
              edits.setRetouch(value);
              void edits.flush().catch(console.error);
            }}
            onMove={actions.move}
            onRate={(rating): Promise<void> => actions.rate(rating, false)}
            cropActive={state.cropEditing}
            onCropActiveChange={(active): void => update({ cropEditing: active })}
            showHistogram={state.histogramOpen}
            showFocus={state.informationOpen}
          />
          <section class="panel" ref={panelRef}>
            <div class="meta">
              <div>
                <div class="image-title-line">
                  <div id="image-title" class="image-title" title={image ? imageSourceInfoTitle(image) : undefined}>
                    {image?.file_name}
                  </div>
                  <div id="image-exif" class="image-exif" aria-label="Camera settings">
                    {exif.map((part, index): ComponentChildren => (
                      <Fragment key={index}>
                        {index > 0 ? " · " : ""}
                        <span class={part.className} title={part.title}>
                          {part.text}
                        </span>
                      </Fragment>
                    ))}
                  </div>
                </div>
              </div>
              <div id="profile-state" class="profile-state">
                {image ? (
                  hideProfiles || !selected ? (
                    `${display.text}${codexText ? ` | ${codexText}` : ""}`
                  ) : (
                    <span class="profile-state-summary">
                      <button
                        type="button"
                        class="current-profile-link"
                        onClick={(): void => tools.openProfileInfo(selected)}
                      >
                        {profileDisplayName(selected)}
                      </button>
                      {correctionText ? (
                        <span title={correctionTitle} aria-label={`${correctionText} used: ${correctionTitle}`}>
                          {correctionText} used
                        </span>
                      ) : null}
                      {selected.bw_filter_eligible ? (
                        <BwFilterControls image={image} profile={selected} onChange={actions.setBwFilter} />
                      ) : null}
                      {`: ${suffix}`}
                    </span>
                  )
                ) : null}
              </div>
            </div>
            <div class="mobile-actions" aria-label="Review tools">
              {(["profiles", "retouch", "metadata"] as const).map((name): ComponentChildren => (
                <Fragment key={name}>
                  {name === "metadata" ? (
                    <button
                      id="mobile-save-original"
                      type="button"
                      hidden={!compressed}
                      disabled={!compressed || original.busy}
                      title={originalTitle}
                      aria-label={originalTitle}
                      onClick={(): void => {
                        void original.save().catch(console.error);
                      }}
                    >
                      {original.label}
                    </button>
                  ) : null}
                  <button
                    data-mobile-drawer={name}
                    type="button"
                    hidden={name === "profiles" ? hideProfiles : name === "retouch" && direct}
                    class={drawer === name ? "active" : undefined}
                    title={
                      name === "profiles"
                        ? `${profileCount} profile ${profileCount === 1 ? "render" : "renders"}`
                        : name === "retouch"
                          ? retouchActive
                            ? "Retouch adjustments are active"
                            : "Retouch"
                          : metadataTitle
                    }
                    aria-pressed={drawer === name}
                    onClick={(): void => update({ mobileDrawer: drawer === name ? null : name })}
                  >
                    {name === "profiles"
                      ? profileCount
                        ? `Profiles ${profileCount}`
                        : "Profiles"
                      : name === "retouch"
                        ? retouchActive
                          ? "Retouch *"
                          : "Retouch"
                        : image?.tags.length || image?.notes
                          ? "Meta *"
                          : "Meta"}
                  </button>
                </Fragment>
              ))}
              <button id="mobile-publish" type="button" onClick={(): void => tools.togglePublishWizard()}>
                Publish
              </button>
            </div>
            {!rail ? profileList : null}
            <Controls
              image={image}
              profile={profile}
              disabled={disabled}
              edits={edits}
              tagsRef={tagsRef}
              notesRef={notesRef}
              onRate={actions.rate}
              onLabel={actions.toggleLabel}
              onMove={actions.move}
            />
          </section>
          {rail ? profileList : null}
        </main>
        <ToolOverlayHost placement="inside" />
      </div>
      <ShortcutsOverlay open={shortcutsOpen} onClose={(): void => setShortcutsOpen(false)} />
      <ToolOverlayHost placement="outside" />
    </>
  );
}
