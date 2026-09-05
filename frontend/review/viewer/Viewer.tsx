/** Present the current picture with reactive crop, autofocus, histogram, and zoom layers.
 * Preact owns visible markup and styles; refs are reserved for canvas sampling, measurements, and pointer capture. */
import type { JSX } from "preact";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Dimensions, RetouchSettings, ReviewImage, ReviewProfileRender } from "../core/types";
import { useReviewContext } from "../core/context";
import {
  currentImage,
  defaultRetouch,
  isDirectCompressedImage,
  isSoocProfile,
  mainImageSource,
  profileDisplayState,
  versionedUrl,
} from "../core/selectors";
import { TOUCH_SWIPE_MIN_PX, TOUCH_SWIPE_RATIO, ZOOM_LONG_PRESS_MS, ZOOM_MOVE_CANCEL_PX } from "../core/constants";
import { CropEditor } from "./CropEditor";
import { Histogram } from "./Histogram";
import { ViewerPreloads } from "./Preloads";
import { clamp, cssUrl, focusRegionPolygons, fullZoomOffset, zoomLoupePosition } from "./geometry";

export interface ViewerProps {
  image: ReviewImage | null;
  selected: ReviewProfileRender | null;
  retouch: RetouchSettings;
  onRetouch: (retouch: RetouchSettings) => void | Promise<void>;
  onMove: (delta: number) => Promise<void>;
  onRate: (rating: number) => Promise<void>;
  cropActive: boolean;
  onCropActiveChange: (active: boolean) => void;
  onCropReadyChange?: (ready: boolean) => void;
  showHistogram: boolean;
  showFocus: boolean;
  shortcutsBlocked?: boolean;
  feedback?: ViewerFeedback | null;
}

/** A sequence distinguishes repeated identical feedback from an unchanged application snapshot. */
export interface ViewerFeedback {
  text: string;
  sequence: number;
}

interface Rect extends Dimensions {
  left: number;
  top: number;
}
interface Layout {
  viewer: Rect;
  image: Rect;
  full: Rect;
  loupe: Dimensions;
  available: Dimensions;
  longEdge: number;
}
interface ZoomPoint {
  clientX: number;
  clientY: number;
  pointerType: string;
}
interface Zoom {
  kind: "loupe" | "full";
  point: ZoomPoint;
}
interface PointerSession extends ZoomPoint {
  pointerId: number;
  startX: number;
  startY: number;
  timer: number | null;
  zoomed: boolean;
}
interface LoadedSource extends Dimensions {
  url: string;
}

const EMPTY_RECT: Rect = { left: 0, top: 0, width: 0, height: 0 };

/** Read the minimum bounding-box fields needed to position declarative viewer layers. */
function rectOf(element: Element | null): Rect {
  const rect = element?.getBoundingClientRect();
  return rect ? { left: rect.left, top: rect.top, width: rect.width, height: rect.height } : EMPTY_RECT;
}

/** Approximate pending tonal edits using the existing fast browser preview filter. */
function draftFilter(retouch: RetouchSettings, active: boolean, sooc: boolean): string {
  const adjustments = sooc ? defaultRetouch().adjustments : retouch.adjustments;
  const changed = Object.values(adjustments).some((value) => value !== 0) || retouch.crop || retouch.rotation_degrees;
  if (!active || !changed) return "";
  const {
    exposure,
    highlights,
    shadows,
    whites,
    blacks,
    temperature,
    offset,
    clarity,
    contrast: globalContrast,
  } = adjustments;
  const brightness = clamp(
    1 + exposure * 0.13 + whites * 0.002 - blacks * 0.0015 + shadows * 0.0015 - highlights * 0.0008,
    0.45,
    1.85,
  );
  const contrast = clamp(1 + globalContrast * 0.004 + clarity * 0.002 + (highlights - shadows) * 0.0008, 0.55, 1.65);
  const saturation = clamp(
    1 + clarity * 0.0015 + Math.abs(temperature) * 0.000015 + Math.abs(offset) * 0.0006,
    0.7,
    1.3,
  );
  const sepia = clamp(Math.max(0, temperature) / 2500, 0, 1) * 0.12;
  const hue = clamp(-temperature / 2500, -1, 1) * 5 + clamp(offset / 100, -1, 1) * 4;
  return [
    `brightness(${brightness.toFixed(3)})`,
    `contrast(${contrast.toFixed(3)})`,
    `saturate(${saturation.toFixed(3)})`,
    `sepia(${sepia.toFixed(3)})`,
    `hue-rotate(${hue.toFixed(3)}deg)`,
  ].join(" ");
}

/** Keep image browsing and editing responsive without granting DOM ownership to controllers. */
export function Viewer({
  image,
  selected,
  retouch,
  onRetouch,
  onMove,
  onRate,
  cropActive,
  onCropActiveChange,
  onCropReadyChange,
  showHistogram,
  showFocus,
  shortcutsBlocked = false,
  feedback: feedbackRequest,
}: ViewerProps): JSX.Element {
  const { state, getState } = useReviewContext();
  const viewerRef = useRef<HTMLElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);
  const fullRef = useRef<HTMLDivElement>(null);
  const loupeRef = useRef<HTMLDivElement>(null);
  const pointer = useRef<PointerSession | null>(null);
  const [zoom, setZoom] = useState<Zoom | null>(null);
  const [feedback, setFeedback] = useState<ViewerFeedback>({ text: "", sequence: 0 });
  const [revision, setRevision] = useState<number>(0);
  const [natural, setNatural] = useState<Dimensions>({ width: 0, height: 0 });
  const [loadedSource, setLoadedSource] = useState<string>("");
  const [fullSource, setFullSource] = useState<LoadedSource | null>(null);
  const [cropReady, setCropReady] = useState<boolean>(false);
  const [layout, setLayout] = useState<Layout>({
    viewer: EMPTY_RECT,
    image: EMPTY_RECT,
    full: EMPTY_RECT,
    loupe: { width: 180, height: 180 },
    available: { width: 1, height: 1 },
    longEdge: Math.max(window.innerWidth, window.innerHeight),
  });
  const source = mainImageSource(image, selected, layout.longEdge);
  const sourceUrl = source.url ? versionedUrl(source.url, source.updatedAt) : "";
  const pending = state.localRetouchDirty || Boolean(selected && selected.status !== "done");
  const focusPending =
    state.localRetouchDirty || Boolean(image?.preview_retouch_pending) || Boolean(selected?.retouch_pending);
  const display = profileDisplayState(image, selected, state.localRetouchDirty);
  const gridPending =
    state.localRetouchDirty || display.state === "retouch-queued" || display.state === "retouch-processing";
  const filter = draftFilter(retouch, pending || cropActive, isSoocProfile(selected));
  const polygons = useMemo(
    () =>
      image && showFocus && !cropActive && !focusPending && loadedSource === sourceUrl && natural.width > 0
        ? focusRegionPolygons(image, retouch)
        : [],
    [image, showFocus, cropActive, focusPending, loadedSource, sourceUrl, natural.width, retouch],
  );
  const fullMediaUrl =
    zoom && isDirectCompressedImage(image) && image?.full_url
      ? versionedUrl(image.full_url, image.preview_updated_at || image.updated_at)
      : null;
  const zoomSource: LoadedSource =
    fullSource && fullSource.url === fullMediaUrl
      ? fullSource
      : { url: sourceUrl, width: natural.width || layout.image.width, height: natural.height || layout.image.height };

  /** Read responsive geometry after layout, then let JSX position overlays from that snapshot. */
  const measure = useCallback((): void => {
    const viewer = viewerRef.current;
    if (!viewer) return;
    const padding = getComputedStyle(viewer);
    const next: Layout = {
      viewer: rectOf(viewer),
      image: rectOf(imageRef.current),
      full: rectOf(fullRef.current),
      loupe: { width: loupeRef.current?.offsetWidth || 180, height: loupeRef.current?.offsetHeight || 180 },
      available: {
        width: Math.max(1, viewer.clientWidth - parseFloat(padding.paddingLeft) - parseFloat(padding.paddingRight)),
        height: Math.max(1, viewer.clientHeight - parseFloat(padding.paddingTop) - parseFloat(padding.paddingBottom)),
      },
      longEdge: Math.max(window.innerWidth, window.innerHeight),
    };
    setLayout((previous) => (JSON.stringify(previous) === JSON.stringify(next) ? previous : next));
  }, []);

  useLayoutEffect((): (() => void) => {
    measure();
    let frame = 0;
    /** Batch resize notifications into a new frame so state-driven layout cannot reenter observer delivery. */
    function scheduleMeasure(): void {
      if (frame) return;
      frame = window.requestAnimationFrame((): void => {
        frame = 0;
        measure();
      });
    }
    const observer = new ResizeObserver(scheduleMeasure);
    if (viewerRef.current) observer.observe(viewerRef.current);
    if (imageRef.current) observer.observe(imageRef.current);
    window.addEventListener("resize", scheduleMeasure);
    return (): void => {
      observer.disconnect();
      window.removeEventListener("resize", scheduleMeasure);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [measure]);
  useLayoutEffect(measure, [measure, sourceUrl, revision, cropActive, zoom?.kind]);
  useEffect((): void => {
    onCropReadyChange?.(cropActive && cropReady);
  }, [onCropReadyChange, cropActive, cropReady]);

  /** Clear a held pointer and all zoom layers when media or editing context changes. */
  const stopZoom = useCallback((): void => {
    if (pointer.current?.timer !== null && pointer.current?.timer !== undefined)
      window.clearTimeout(pointer.current.timer);
    pointer.current = null;
    setZoom(null);
    setFullSource(null);
  }, []);

  useEffect((): void => {
    stopZoom();
  }, [image?.id, cropActive, stopZoom]);
  useEffect((): void => {
    // A rerender/profile refresh replaces media in full zoom, but dismisses transient pointer magnification.
    if (pointer.current?.timer !== null && pointer.current?.timer !== undefined)
      window.clearTimeout(pointer.current.timer);
    pointer.current = null;
    setZoom((previous) => (previous?.kind === "full" ? previous : null));
    setFullSource(null);
  }, [sourceUrl]);
  useEffect((): (() => void) => stopZoom, [stopZoom]);
  /** Refresh the transient message even if two consecutive actions produce the same rating or text. */
  const showFeedback = useCallback((text: string): void => {
    setFeedback((previous) => ({ text, sequence: previous.sequence + 1 }));
  }, []);
  useEffect((): void => {
    if (feedbackRequest) showFeedback(feedbackRequest.text);
  }, [feedbackRequest, showFeedback]);
  useEffect((): (() => void) | undefined => {
    if (!feedback.text) return;
    const timer = window.setTimeout((): void => setFeedback((previous) => ({ ...previous, text: "" })), 850);
    return (): void => window.clearTimeout(timer);
  }, [feedback]);
  useEffect((): (() => void) | undefined => {
    if (!zoom || shortcutsBlocked) return;
    /** Consume Escape only while a viewer zoom layer owns that action. */
    function escape(event: KeyboardEvent): void {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      stopZoom();
    }
    window.addEventListener("keydown", escape, true);
    return (): void => window.removeEventListener("keydown", escape, true);
  }, [zoom, stopZoom, shortcutsBlocked]);

  /** Track decoded dimensions separately from state so canvas and overlays redraw on load. */
  function imageLoaded(event: JSX.TargetedEvent<HTMLImageElement>): void {
    if (zoom?.kind === "loupe") stopZoom();
    setLoadedSource(event.currentTarget.getAttribute("src") || "");
    setNatural({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight });
    setRevision((value) => value + 1);
    measure();
  }

  /** Start a primary-button hold without interfering with crop tools or other controls. */
  function pointerDown(event: JSX.TargetedPointerEvent<HTMLElement>): void {
    if (
      cropActive ||
      zoom?.kind === "full" ||
      !image ||
      !sourceUrl ||
      (event.pointerType !== "touch" && event.button !== 0)
    )
      return;
    if (
      event.target instanceof Element &&
      event.target.closest(".crop-overlay, .crop-tools, .retouch-grid, .gesture-feedback, .zoom-loupe")
    )
      return;
    stopZoom();
    const session: PointerSession = {
      pointerId: event.pointerId,
      pointerType: event.pointerType,
      startX: event.clientX,
      startY: event.clientY,
      clientX: event.clientX,
      clientY: event.clientY,
      timer: null,
      zoomed: false,
    };
    session.timer = window.setTimeout((): void => {
      if (pointer.current !== session) return;
      session.zoomed = true;
      session.timer = null;
      setZoom({
        kind: "loupe",
        point: { clientX: session.clientX, clientY: session.clientY, pointerType: session.pointerType },
      });
    }, ZOOM_LONG_PRESS_MS);
    pointer.current = session;
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      /* Canceled browser gestures can reject capture. */
    }
    event.preventDefault();
  }

  /** Follow zoom pointers and cancel a hold once movement becomes a swipe. */
  function pointerMove(event: JSX.TargetedPointerEvent<HTMLElement>): void {
    if (zoom?.kind === "full" && event.pointerType === "mouse") {
      setZoom({
        kind: "full",
        point: { clientX: event.clientX, clientY: event.clientY, pointerType: event.pointerType },
      });
      return;
    }
    const session = pointer.current;
    if (!session || session.pointerId !== event.pointerId) return;
    if (session.pointerType === "touch") event.preventDefault();
    session.clientX = event.clientX;
    session.clientY = event.clientY;
    if (session.zoomed) {
      event.preventDefault();
      setZoom({
        kind: "loupe",
        point: { clientX: event.clientX, clientY: event.clientY, pointerType: event.pointerType },
      });
    } else if (
      Math.hypot(event.clientX - session.startX, event.clientY - session.startY) > ZOOM_MOVE_CANCEL_PX &&
      session.timer !== null
    ) {
      window.clearTimeout(session.timer);
      session.timer = null;
    }
  }

  /** Turn a released touch into navigation or rating, while completed zoom holds only close their loupe. */
  async function pointerUp(event: JSX.TargetedPointerEvent<HTMLElement>): Promise<void> {
    const session = pointer.current;
    if (!session || session.pointerId !== event.pointerId) return;
    const dx = event.clientX - session.startX;
    const dy = event.clientY - session.startY;
    if (session.zoomed) event.preventDefault();
    stopZoom();
    if (session.zoomed || session.pointerType !== "touch") return;
    if (Math.abs(dx) >= TOUCH_SWIPE_MIN_PX && Math.abs(dx) / Math.max(1, Math.abs(dy)) >= TOUCH_SWIPE_RATIO) {
      await onMove(dx > 0 ? -1 : 1);
      showFeedback(String(currentImage(getState())?.rating || 0));
    } else if (Math.abs(dy) >= TOUCH_SWIPE_MIN_PX && Math.abs(dy) / Math.max(1, Math.abs(dx)) >= TOUCH_SWIPE_RATIO) {
      const rating = clamp((image?.rating || 0) + (dy < 0 ? 1 : -1), 0, 5);
      await onRate(rating);
      showFeedback(String(rating));
    }
  }

  /** Cancel only the pointer-owned interaction, leaving unrelated desktop full zoom intact. */
  function pointerCanceled(event: JSX.TargetedPointerEvent<HTMLElement>): void {
    if (pointer.current?.pointerId === event.pointerId) stopZoom();
  }

  /** Toggle desktop full-frame zoom while keeping touch double taps reserved for gesture navigation. */
  function doubleClick(event: JSX.TargetedMouseEvent<HTMLElement>): void {
    if (
      cropActive ||
      !sourceUrl ||
      event.button !== 0 ||
      !window.matchMedia("(hover: hover) and (pointer: fine)").matches
    )
      return;
    if (
      event.target instanceof Element &&
      event.target.closest(".crop-overlay, .crop-tools, .retouch-grid, .gesture-feedback, .zoom-loupe")
    )
      return;
    event.preventDefault();
    if (zoom?.kind === "full") stopZoom();
    else {
      stopZoom();
      setZoom({ kind: "full", point: { clientX: event.clientX, clientY: event.clientY, pointerType: "mouse" } });
    }
  }

  /** Close the crop editor before dispatching its single saved retouch operation. */
  function applyCrop(next: RetouchSettings): void {
    onCropActiveChange(false);
    void Promise.resolve(onRetouch(next)).catch((error: unknown): void => console.error(error));
  }

  /** Suppress native dragging and context actions only on the image gesture surface. */
  function preventNativeAction(event: JSX.TargetedEvent<HTMLElement>): void {
    if (event.target instanceof Element && !event.target.closest(".crop-overlay, .crop-tools, .retouch-grid"))
      event.preventDefault();
  }

  const frameStyle: JSX.CSSProperties = {
    left: layout.image.left - layout.viewer.left,
    top: layout.image.top - layout.viewer.top,
    width: layout.image.width,
    height: layout.image.height,
  };
  let zoomStyle: JSX.CSSProperties = {};
  if (zoom && zoomSource.width > 0 && zoomSource.height > 0) {
    const { clientX, clientY, pointerType } = zoom.point;
    const relativeX = clamp((clientX - layout.image.left) / Math.max(1, layout.image.width), 0, 1);
    const relativeY = clamp((clientY - layout.image.top) / Math.max(1, layout.image.height), 0, 1);
    zoomStyle = { backgroundImage: `url("${cssUrl(zoomSource.url)}")`, filter };
    if (zoom.kind === "loupe") {
      const position = zoomLoupePosition(
        clientX,
        clientY,
        layout.viewer,
        layout.loupe.width,
        layout.loupe.height,
        pointerType,
      );
      zoomStyle = {
        ...zoomStyle,
        ...position,
        backgroundSize: `${zoomSource.width}px ${zoomSource.height}px`,
        backgroundPosition: [
          `${layout.loupe.width / 2 - relativeX * zoomSource.width}px`,
          `${layout.loupe.height / 2 - relativeY * zoomSource.height}px`,
        ].join(" "),
      };
    } else {
      const scale = Math.max(
        1,
        (layout.image.width * 2) / zoomSource.width,
        (layout.image.height * 2) / zoomSource.height,
        layout.full.width / zoomSource.width,
        layout.full.height / zoomSource.height,
      );
      const width = zoomSource.width * scale;
      const height = zoomSource.height * scale;
      const x = fullZoomOffset(
        clamp(clientX - layout.full.left, 0, layout.full.width),
        relativeX,
        width,
        layout.full.width,
      );
      const y = fullZoomOffset(
        clamp(clientY - layout.full.top, 0, layout.full.height),
        relativeY,
        height,
        layout.full.height,
      );
      zoomStyle = { ...zoomStyle, backgroundSize: `${width}px ${height}px`, backgroundPosition: `${x}px ${y}px` };
    }
  }
  const viewerClass = [
    "viewer",
    sourceUrl && "has-image",
    filter && "draft-retouch",
    zoom?.kind === "loupe" && "zooming",
    zoom?.kind === "full" && "zoom-full-active",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <section
      ref={viewerRef}
      class={viewerClass}
      onPointerDown={pointerDown}
      onPointerMove={pointerMove}
      onPointerUp={(event): void => {
        void pointerUp(event).catch((error: unknown): void => console.error(error));
      }}
      onPointerCancel={pointerCanceled}
      onDblClick={doubleClick}
      onContextMenu={preventNativeAction}
      onDragStart={preventNativeAction}
      onTouchStart={preventNativeAction}
      onTouchMove={preventNativeAction}
    >
      <div id="empty" class="empty" hidden={Boolean(sourceUrl)}>
        Waiting for pictures
      </div>
      <img
        id="main-image"
        ref={imageRef}
        src={sourceUrl || undefined}
        alt={image?.file_name || ""}
        draggable={false}
        decoding="async"
        fetchpriority="high"
        onLoad={imageLoaded}
        style={{
          visibility: cropActive && cropReady ? "hidden" : "visible",
          filter,
          transform: filter ? `rotate(${retouch.rotation_degrees}deg)` : undefined,
        }}
      />
      <svg
        id="focus-overlay"
        class="focus-overlay"
        hidden={polygons.length === 0 ? true : undefined}
        viewBox="0 0 1000 1000"
        preserveAspectRatio="none"
        aria-label="Camera focus points"
        style={frameStyle}
      >
        {polygons.map((polygon, index) => (
          <polygon
            key={index}
            class={polygon.primary ? "focus-region focus-region-primary" : "focus-region"}
            points={polygon.points
              .map((point) => `${(point.x * 1000).toFixed(3)},${(point.y * 1000).toFixed(3)}`)
              .join(" ")}
          />
        ))}
      </svg>
      {cropActive && image ? (
        <CropEditor
          key={image.id}
          image={image}
          selected={selected}
          retouch={retouch}
          available={layout.available}
          shortcutsBlocked={shortcutsBlocked}
          onReadyChange={setCropReady}
          onApply={applyCrop}
          onCancel={(): void => onCropActiveChange(false)}
        />
      ) : null}
      <ViewerPreloads image={image} selected={selected} longEdge={layout.longEdge} sourceUrl={sourceUrl} />
      <div
        id="gesture-feedback"
        class="gesture-feedback"
        hidden={!feedback.text}
        style={{
          left: layout.image.width > 0 ? layout.image.left - layout.viewer.left + layout.image.width / 2 : "50%",
          top: layout.image.height > 0 ? layout.image.top - layout.viewer.top + layout.image.height / 2 : "50%",
        }}
      >
        {feedback.text}
      </div>
      <div
        id="zoom-full"
        ref={fullRef}
        class="zoom-full"
        hidden={zoom?.kind !== "full"}
        aria-hidden="true"
        style={zoom?.kind === "full" ? zoomStyle : undefined}
      />
      <div
        id="zoom-loupe"
        ref={loupeRef}
        class="zoom-loupe"
        hidden={zoom?.kind !== "loupe"}
        style={zoom?.kind === "loupe" ? zoomStyle : undefined}
      />
      {fullMediaUrl ? (
        <img
          key={fullMediaUrl}
          src={fullMediaUrl}
          alt=""
          aria-hidden="true"
          hidden
          style={{ display: "none" }}
          onLoad={(event): void => {
            setFullSource({
              url: fullMediaUrl,
              width: event.currentTarget.naturalWidth,
              height: event.currentTarget.naturalHeight,
            });
          }}
        />
      ) : null}
      <Histogram
        imageRef={imageRef}
        sourceKey={sourceUrl}
        filter={filter}
        visible={showHistogram}
        revision={revision + layout.image.width + layout.image.height}
      />
      <div
        id="retouch-grid"
        class="retouch-grid"
        hidden={cropActive || !gridPending || Math.abs(retouch.rotation_degrees) <= 0.001}
        style={frameStyle}
      />
    </section>
  );
}
