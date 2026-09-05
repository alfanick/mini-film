/**
 * Preserve mobile original-photo sharing as a browser-capability hook.
 * Cached files support the second-tap gesture required by some mobile share sheets.
 */
import { useRef, useState } from "preact/hooks";
import { reviewUrl } from "../core/api";
import type { ReviewImage } from "../core/types";

interface ShareState {
  busyId: number | null;
  retryId: number | null;
  openId: number | null;
}
interface OriginalShare {
  label: string;
  busy: boolean;
  save: () => Promise<void>;
}
interface CachedOriginal {
  imageId: number;
  file: File | null;
  promise: Promise<File> | null;
}

/** Offer file sharing when supported and an explicit open-photo fallback otherwise. */
export function useOriginalShare(image: ReviewImage | null, onFeedback: (text: string) => void): OriginalShare {
  const [state, setState] = useState<ShareState>({ busyId: null, retryId: null, openId: null });
  const cache = useRef<CachedOriginal | null>(null);

  /** Reuse both decoded files and in-flight fetches when rapid activations request the same original. */
  const originalFile = async (current: ReviewImage): Promise<File> => {
    if (cache.current?.imageId === current.id) {
      if (cache.current.file) return cache.current.file;
      if (cache.current.promise) return cache.current.promise;
    }
    const entry: CachedOriginal = { imageId: current.id, file: null, promise: null };
    cache.current = entry;
    entry.promise = (async (): Promise<File> => {
      const response = await fetch(reviewUrl(`original/${current.id}`), { cache: "no-store" });
      if (!response.ok) throw new Error(`original ${response.status}`);
      const contentType = (response.headers.get("content-type")?.split(";", 1)[0] ?? "").trim().toLowerCase();
      if (!["image/jpeg", "image/heic", "image/heif"].includes(contentType))
        throw new Error(`unexpected original content type: ${contentType || "missing"}`);
      return new File(
        [await response.blob()],
        current.file_name || (contentType === "image/jpeg" ? "photo.jpg" : "photo.heic"),
        { type: contentType },
      );
    })();
    try {
      const file = await entry.promise;
      if (cache.current === entry) entry.file = file;
      return file;
    } finally {
      if (cache.current === entry) entry.promise = null;
    }
  };

  /** Fetch only compressed originals and retain a ready file for browser activation retries. */
  const save = async (): Promise<void> => {
    if (!image || image.source_type !== "compressed") return;
    const url = reviewUrl(`original/${image.id}`);
    if (
      state.openId === image.id ||
      typeof File !== "function" ||
      typeof navigator.share !== "function" ||
      typeof navigator.canShare !== "function"
    ) {
      setState((previous: ShareState): ShareState => ({ ...previous, openId: null }));
      window.open(url, "_blank", "noopener");
      return;
    }
    setState((previous: ShareState): ShareState => ({ ...previous, busyId: image.id, retryId: null }));
    try {
      const file = await originalFile(image);
      if (!navigator.canShare({ files: [file] })) {
        setState((previous: ShareState): ShareState => ({ ...previous, openId: image.id }));
        onFeedback("open photo");
        return;
      }
      await navigator.share({ files: [file] });
      setState((previous: ShareState): ShareState => ({ ...previous, openId: null }));
    } catch (error: unknown) {
      const name = typeof error === "object" && error !== null && "name" in error ? String(error.name) : "";
      if (name === "AbortError") return;
      if (name === "NotAllowedError" && cache.current?.imageId === image.id && cache.current.file) {
        setState((previous: ShareState): ShareState => ({ ...previous, retryId: image.id }));
        onFeedback("photo ready");
      } else {
        console.error(error);
        setState((previous: ShareState): ShareState => ({ ...previous, openId: image.id }));
        onFeedback("open photo");
      }
    } finally {
      setState((previous: ShareState): ShareState =>
        previous.busyId === image.id ? { ...previous, busyId: null } : previous,
      );
    }
  };
  const id = image?.id;
  return {
    label:
      state.busyId === id
        ? "Preparing"
        : state.openId === id
          ? "Open Photo"
          : state.retryId === id
            ? "Save Again"
            : "Save Photo",
    busy: state.busyId === id,
    save,
  };
}
