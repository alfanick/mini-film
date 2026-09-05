/**
 * Render controlled review metadata and retouch inputs with accessible keyboard behavior.
 * Slider values combine profile baselines and camera white balance while saved values remain deltas.
 */
import type { ComponentChildren, RefObject } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { COLOR_LABELS, RATING_VALUES } from "../core/constants";
import { capitalize, clamp, defaultRetouch, imageLabels, labelLetter, normalizedRetouch } from "../core/selectors";
import type {
  BasicRetouchAdjustments as RetouchAdjustments,
  ReviewImage,
  ReviewProfile,
  ReviewLabel,
} from "../core/types";
import type { ReviewEdits } from "../session/use-edits";
import { retouchFromVisibleControls } from "../session/retouch-controls";

type Adjustment = keyof RetouchAdjustments;
interface SliderDefinition {
  key: Adjustment;
  label: string;
  limit: number;
  step: number;
}
const SLIDERS: SliderDefinition[] = [
  { key: "clarity", label: "Clarity", limit: 100, step: 1 },
  { key: "highlights", label: "Highlights", limit: 100, step: 1 },
  { key: "whites", label: "Whites", limit: 100, step: 1 },
  { key: "temperature", label: "Temperature", limit: 2500, step: 50 },
  { key: "exposure", label: "Exposure", limit: 4, step: 0.05 },
  { key: "contrast", label: "Contrast", limit: 100, step: 1 },
  { key: "shadows", label: "Shadows", limit: 100, step: 1 },
  { key: "blacks", label: "Blacks", limit: 100, step: 1 },
  { key: "offset", label: "Tint", limit: 100, step: 1 },
];

interface ControlsProps {
  image: ReviewImage | null;
  profile: ReviewProfile | null;
  disabled: boolean;
  edits: ReviewEdits;
  tagsRef: RefObject<HTMLInputElement>;
  notesRef: RefObject<HTMLInputElement>;
  onRate: (rating: number) => Promise<void>;
  onLabel: (label: ReviewLabel) => Promise<void>;
  onMove: (delta: number) => Promise<void>;
}

/** Derive a numeric camera baseline without treating absent EXIF as a measured zero. */
function cameraBaseline(image: ReviewImage | null, key: "temperature" | "offset"): number {
  const raw = key === "temperature" ? image?.exif.white_balance_temperature : image?.exif.white_balance_offset;
  if (raw === null || raw === undefined) return 0;
  const value = Number(raw);
  return Number.isFinite(value) && (key !== "temperature" || value > 0) ? Math.round(value) : 0;
}

/** Format readouts identically to the review UI, including absolute camera temperature. */
function sliderReadout(key: Adjustment, value: number, baseline: number): string {
  if (key === "temperature") {
    const rounded = Math.round(value);
    return `${baseline ? rounded : `${rounded > 0 ? "+" : ""}${rounded}`}K`;
  }
  const text = value.toFixed(key === "exposure" ? 2 : 0);
  return `${Number(text) > 0 ? "+" : ""}${text}`;
}

/** Keep a slider's focus-time value for reversible Escape edits and percentage keyboard nudges. */
function RetouchSlider({
  definition,
  image,
  profile,
  edits,
  disabled,
}: {
  definition: SliderDefinition;
  image: ReviewImage | null;
  profile: ReviewProfile | null;
  edits: ReviewEdits;
  disabled: boolean;
}): ComponentChildren {
  const { key, label, limit, step } = definition;
  const camera = key === "temperature" || key === "offset";
  const baseline = camera
    ? cameraBaseline(image, key)
    : normalizedRetouch({ adjustments: profile?.retouch_base }).adjustments[key];
  const min = (camera ? baseline : 0) - limit;
  const max = (camera ? baseline : 0) + limit;
  const value = clamp(baseline + edits.retouch.adjustments[key], min, max);
  const [active, setActive] = useState<boolean>(false);
  const original = useRef<number>(value);
  const inputRef = useRef<HTMLInputElement>(null);
  const id = `retouch-${key}`;

  /** Convert the displayed absolute value back to the server's per-picture delta. */
  const change = (next: number, save = true): void => {
    edits.setRetouch(
      retouchFromVisibleControls(
        {
          ...edits.retouch,
          adjustments: { ...edits.retouch.adjustments, [key]: next - baseline },
        },
        profile,
        image,
      ),
      save,
    );
  };
  useEffect((): (() => void) | undefined => {
    if (!active) return;
    /** A photo tap clears slider keyboard ownership even when that photo suppresses native focus changes. */
    function clearOutside(event: PointerEvent): void {
      if (!(event.target instanceof Element) || event.target.closest(".retouch label")) return;
      setActive(false);
      inputRef.current?.blur();
    }
    document.addEventListener("pointerdown", clearOutside);
    return (): void => document.removeEventListener("pointerdown", clearOutside);
  }, [active]);
  return (
    <label data-retouch-adjustment="true" class={active ? "retouch-slider-active" : undefined}>
      <span
        class={disabled ? "retouch-adjustment-label-disabled" : undefined}
        title={
          key === "temperature" && image?.exif.white_balance_mode
            ? `White balance: ${image.exif.white_balance_mode}`
            : "Double-click to reset"
        }
        onDblClick={(event): void => {
          event.preventDefault();
          if (!disabled) change(baseline);
        }}
      >
        {label}
      </span>
      <input
        id={id}
        ref={inputRef}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onFocus={(): void => {
          original.current = value;
          setActive(true);
        }}
        onBlur={(): void => setActive(false)}
        onClick={(event): void => {
          original.current = Number(event.currentTarget.value);
          setActive(true);
        }}
        onInput={(event): void => change(Number(event.currentTarget.value))}
        onKeyDown={(event): void => {
          if (
            !event.ctrlKey &&
            !event.altKey &&
            !event.metaKey &&
            ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
          ) {
            event.preventDefault();
            event.stopPropagation();
            const direction = ["ArrowLeft", "ArrowDown"].includes(event.key) ? -1 : 1;
            const next = clamp(value + (max - min) * (event.shiftKey ? 0.01 : 0.1) * direction, min, max);
            change(Number(clamp(Math.round((next - min) / step) * step + min, min, max).toFixed(6)));
          } else if (event.key === "Escape" || event.key === "Enter") {
            event.preventDefault();
            event.stopPropagation();
            if (event.key === "Escape") change(original.current, false);
            else void edits.flush(true).catch(console.error);
            event.currentTarget.blur();
          }
        }}
      />
      <output id={`${id}-value`}>{sliderReadout(key, value, baseline)}</output>
    </label>
  );
}

/** Compose metadata and tonal controls without DOM registries or imperative value synchronization. */
export function Controls({
  image,
  profile,
  disabled,
  edits,
  tagsRef,
  notesRef,
  onRate,
  onLabel,
  onMove,
}: ControlsProps): ComponentChildren {
  const labels = imageLabels(image);
  return (
    <div class="controls">
      <div class="rating" role="group" aria-label="Rating">
        {RATING_VALUES.map((rating: number): ComponentChildren => (
          <button
            key={rating}
            data-rating={rating}
            type="button"
            class={(image?.rating || 0) === rating ? "active" : undefined}
            onClick={(): void => {
              void onRate(rating).catch(console.error);
            }}
          >
            {rating}
          </button>
        ))}
      </div>
      <div class="labels edit-labels" role="group" aria-label="Label">
        {COLOR_LABELS.map((label): ComponentChildren => (
          <button
            key={label}
            data-label={label}
            type="button"
            title={capitalize(label)}
            aria-label={`${capitalize(label)} label`}
            class={labels.includes(label) ? "active" : undefined}
            onClick={(): void => {
              void onLabel(label).catch(console.error);
            }}
          >
            {labelLetter(label)}
          </button>
        ))}
      </div>
      <label class="tags">
        <span>Tags</span>
        <input
          ref={tagsRef}
          id="tags"
          type="text"
          inputMode="numeric"
          autocomplete="off"
          placeholder="12, 42, 108"
          value={edits.tags}
          onFocus={(): void => edits.focusMetadata("tags")}
          onInput={(event): void => edits.setTags(event.currentTarget.value)}
          onBlur={(): void => {
            edits.focusMetadata(null);
            void edits.flush().catch(console.error);
          }}
          onKeyDown={(event): void => {
            if (event.key !== "Enter" && event.key !== "Escape") return;
            event.preventDefault();
            event.stopPropagation();
            const advance = event.key === "Enter";
            event.currentTarget.blur();
            void edits
              .flush()
              .then(async (): Promise<void> => {
                if (advance) await onMove(1);
              })
              .catch(console.error);
          }}
        />
      </label>
      <label class="notes">
        <span>Notes</span>
        <input
          ref={notesRef}
          id="notes"
          type="text"
          autocomplete="off"
          placeholder="optional note"
          value={edits.notes}
          onFocus={(): void => edits.focusMetadata("notes")}
          onInput={(event): void => edits.setNotes(event.currentTarget.value)}
          onBlur={(): void => {
            edits.focusMetadata(null);
            void edits.flush().catch(console.error);
          }}
          onKeyDown={(event): void => {
            if (event.key !== "Enter" && event.key !== "Escape") return;
            event.preventDefault();
            event.stopPropagation();
            event.currentTarget.blur();
            void edits.flush().catch(console.error);
          }}
        />
      </label>
      <section
        class="retouch"
        aria-label="Retouch"
        onFocusIn={(): void => edits.focusRetouch(true)}
        onFocusOut={(event): void => {
          if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget))
            edits.focusRetouch(false);
        }}
      >
        <div class="retouch-header">
          <span>Retouch</span>
          <div class="retouch-header-actions">
            <button id="retouch-copy" type="button" disabled={disabled} onClick={edits.copy}>
              Copy
            </button>
            <button id="retouch-paste" type="button" disabled={disabled || !edits.clipboard} onClick={edits.paste}>
              Paste
            </button>
            <button
              id="retouch-reset"
              type="button"
              disabled={disabled}
              onClick={(): void => edits.setRetouch(defaultRetouch())}
            >
              Reset
            </button>
          </div>
        </div>
        {SLIDERS.map((definition: SliderDefinition): ComponentChildren => (
          <RetouchSlider
            key={definition.key}
            definition={definition}
            image={image}
            profile={profile}
            edits={edits}
            disabled={disabled}
          />
        ))}
      </section>
    </div>
  );
}
