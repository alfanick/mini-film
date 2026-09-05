/** Warm adjacent media through invisible declarative images instead of manually constructed DOM nodes.
 * Idle scheduling protects the active picture; a bounded URL history retains the original cache behavior. */
import type { JSX } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { ReviewImage, ReviewProfileRender, ReviewState } from "../core/types";
import { useReviewContext } from "../core/context";
import { COMPRESSED_REVIEW_PREVIEW_LONG_EDGE } from "../core/constants";
import {
  filteredImages,
  isCompressedImage,
  isDirectCompressedImage,
  selectedProfile,
  versionedUrl,
} from "../core/selectors";

interface ViewerPreloadsProps {
  image: ReviewImage | null;
  selected: ReviewProfileRender | null;
  longEdge: number;
  sourceUrl: string;
}

/** Choose forward-only compressed previews or the immediate neighbors in a RAW/profile review. */
export function nearbyPreloadUrls(state: ReviewState, imageId: number, longEdge: number): string[] {
  const catalog = state.data?.images || [];
  const compressedOnly = catalog.length > 0 && catalog.every(isCompressedImage);
  const images = filteredImages(state);
  const index = images.findIndex((image) => image.id === imageId);
  if (index < 0) return [];
  const candidates = compressedOnly ? images.slice(index + 1, index + 4) : [images[index - 1], images[index + 1]];
  const fullMedia = compressedOnly && longEdge > COMPRESSED_REVIEW_PREVIEW_LONG_EDGE;
  const urls = new Set<string>();
  for (const image of candidates) {
    if (!image) continue;
    const selected = selectedProfile(image, state);
    if (selected?.url) urls.add(versionedUrl(selected.url, selected.updated_at));
    else if (isDirectCompressedImage(image) && fullMedia && image.full_url)
      urls.add(versionedUrl(image.full_url, image.preview_updated_at || image.updated_at));
    else if (image.preview_url) urls.add(versionedUrl(image.preview_url, image.preview_updated_at));
  }
  return Array.from(urls);
}

/** Mount low-priority media when the browser is idle and prepare the uncropped camera source immediately. */
export function ViewerPreloads({ image, selected, longEdge, sourceUrl }: ViewerPreloadsProps): JSX.Element {
  const { state } = useReviewContext();
  const [preloaded, setPreloaded] = useState<string[]>([]);
  const urlsKey = JSON.stringify(image ? nearbyPreloadUrls(state, image.id, longEdge) : []);
  const cropUrl = image?.crop_source_url || selected?.base_url || image?.preview_url || selected?.url;
  const cropUpdated = image?.crop_source_url
    ? image.crop_source_updated_at || image.preview_updated_at
    : selected?.base_url
      ? selected.updated_at
      : image?.preview_url
        ? image.preview_updated_at
        : selected?.updated_at;
  const cropSource = cropUrl ? versionedUrl(cropUrl, cropUpdated) : null;
  useEffect((): (() => void) => {
    const urls: string[] = JSON.parse(urlsKey) as string[];
    /** Retain decoded neighbors without allowing a long review to grow the hidden subtree indefinitely. */
    function preload(): void {
      if (urls.length === 0) return;
      setPreloaded((previous) => {
        const next = Array.from(new Set([...previous, ...urls]));
        return next.length > 96 ? next.slice(-64) : next;
      });
    }
    if (typeof window.requestIdleCallback === "function") {
      const request = window.requestIdleCallback(preload, { timeout: 1200 });
      return (): void => window.cancelIdleCallback(request);
    }
    const timer = window.setTimeout(preload, 350);
    return (): void => window.clearTimeout(timer);
  }, [urlsKey]);
  const urls = new Set(preloaded);
  if (cropSource && cropSource !== sourceUrl) urls.add(cropSource);
  return (
    <div hidden aria-hidden="true" style={{ display: "none" }}>
      {Array.from(urls).map((url) => (
        <img key={url} src={url} alt="" decoding="async" loading="eager" fetchpriority="low" />
      ))}
    </div>
  );
}
