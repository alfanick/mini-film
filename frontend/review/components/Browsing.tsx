/** Reactive picture browsing and profile selection components.
 * Stable component identities keep focus, scrolling, and image loads intact as live review state changes. */
import type { ComponentChildren } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ReviewImage, ReviewProfileRender, ReviewBurst, BwFilter } from "../core/types";
import { useReviewContext } from "../core/context";
import { BW_FILTERS, BW_FILTER_LABELS, BW_FILTER_NAMES } from "../core/constants";
import {
  imageLabels,
  isSoocProfile,
  labelLetter,
  normalizeBwFilter,
  plural,
  profileDisplayName,
  profileDisplayState,
  selectedProfile,
  versionedUrl,
  profilesAreImplicitOnly,
} from "../core/selectors";
import {
  burstCaptureDeltaDisplay,
  isPortraitRenderProfile,
  isolateBurstActivation,
  profileDownloadName,
  profileDownloadTitle,
  renderProgressSummary,
  sidebarCameraModel,
  sidebarFileStem,
  imageCaptureDisplay,
} from "./formatting";

/** Typed callbacks keep review mutations in the owning session. */
export interface ImageListProps {
  images: ReviewImage[];
  bursts: ReviewBurst[];
  currentId: number | null;
  onSelect: (image: ReviewImage) => Promise<unknown>;
  onToggleBurst: (id: string, expanded: boolean) => Promise<unknown>;
}

/** Typed callbacks keep review mutations in the owning session. */
export interface VisibleBurst extends ReviewBurst {
  members: ReviewImage[];
  total: number;
}

/** Typed callbacks keep review mutations in the owning session. */
export interface BurstGroupProps {
  burst: VisibleBurst;
  currentId: ImageListProps["currentId"];
  onSelect: ImageListProps["onSelect"];
  onToggleBurst: ImageListProps["onToggleBurst"];
}

/** Typed callbacks keep review mutations in the owning session. */
export interface ImageRowProps {
  image: ReviewImage;
  currentId: ImageListProps["currentId"];
  onSelect: ImageListProps["onSelect"];
  className?: string;
  burstCount?: string | null;
  isolateActivation?: boolean;
}

/** Typed callbacks keep review mutations in the owning session. */
export interface ProfileListProps {
  image: ReviewImage | null;
  onSelect: (profile: ReviewProfileRender) => Promise<unknown>;
  onToggleEnabled: (profile: ReviewProfileRender) => Promise<unknown>;
  onSolo: (profile: ReviewProfileRender) => Promise<unknown>;
}

/** The filter UI is reusable in the profile status line without changing profile-card layout. */
export interface BwFilterControlsProps {
  image: ReviewImage;
  profile: ReviewProfileRender;
  onChange?: (profile: ReviewProfileRender, filter: BwFilter) => Promise<void>;
}

/** Render the filtered photo sequence and preserve camera-defined burst membership. */
export function ImageList({ images, bursts, currentId, onSelect, onToggleBurst }: ImageListProps): ComponentChildren {
  const displayImages = useMemo((): ReviewImage[] => {
    let day: string | null = null;
    return images.map((image): ReviewImage => {
      const display = imageCaptureDisplay(image, day);
      day = display.day;
      return { ...image, capture_time: image.capture_time ?? display.text };
    });
  }, [images]);
  const imageById = new Map(displayImages.map((image) => [String(image.id), image]));
  const burstByImageId = new Map<string, VisibleBurst>();

  for (const burst of Array.isArray(bursts) ? bursts : []) {
    if (burst?.id === undefined || burst?.id === null || !Array.isArray(burst.image_ids)) continue;
    const memberIds = Array.from(new Set(burst.image_ids.map(String)));
    const members = memberIds
      .filter((imageId) => !burstByImageId.has(imageId))
      .map((imageId) => imageById.get(imageId))
      .filter((image): image is ReviewImage => Boolean(image));
    if (members.length < 2) continue;
    const visibleBurst = {
      ...burst,
      members,
      total: memberIds.length,
    };
    for (const member of members) burstByImageId.set(String(member.id), visibleBurst);
  }

  const renderedBursts = new Set<string>();
  return displayImages.flatMap((image) => {
    const burst = burstByImageId.get(String(image.id));
    if (!burst) {
      return [<ImageRow key={`image:${image.id}`} image={image} currentId={currentId} onSelect={onSelect} />];
    }

    const burstKey = String(burst.id);
    if (renderedBursts.has(burstKey)) return [];
    renderedBursts.add(burstKey);
    return [
      <BurstGroup
        key={`burst:${burstKey}`}
        burst={burst}
        currentId={currentId}
        onSelect={onSelect}
        onToggleBurst={onToggleBurst}
      />,
    ];
  });
}

/** Keep each burst expandable without changing the independent review state of its members. */
export function BurstGroup({ burst, currentId, onSelect, onToggleBurst }: BurstGroupProps): ComponentChildren {
  const currentMember = burst.members.find((image) => image.id === currentId);
  const displayed = currentMember || burst.members[0];
  const visibleCount = burst.members.length;
  const count = `${visibleCount}/${burst.total}`;
  const expanded = Boolean(burst.expanded);
  const expansionLabel = `${expanded ? "Collapse" : "Expand"} burst, ${visibleCount} of ${burst.total} ${plural(
    burst.total,
    "picture",
  )} visible`;

  return (
    <section
      class={`burst-group${currentMember ? " contains-active" : ""}${expanded ? " expanded" : ""}`}
      role={"group"}
      aria-label={`Burst, ${visibleCount} of ${burst.total} ${plural(burst.total, "picture")} visible`}
    >
      <div class={"burst-header"}>
        <ImageRow
          image={displayed}
          currentId={expanded ? null : currentId}
          onSelect={onSelect}
          className={"burst-summary"}
          burstCount={count}
          isolateActivation={true}
        />
        <button
          type={"button"}
          class={"burst-toggle"}
          title={expansionLabel}
          aria-label={expansionLabel}
          aria-expanded={expanded ? "true" : "false"}
          onKeyDown={isolateBurstActivation}
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            onToggleBurst(burst.id, !expanded).catch((error) => console.error(error));
          }}
        >
          <span class={"burst-chevron-icon"} aria-hidden={"true"}>
            {"\u203a"}
          </span>
        </button>
      </div>
      {expanded ? (
        <div class={"burst-members"}>
          {burst.members.map((image, index) => {
            const relativeCaptureTime = index > 0 ? burstCaptureDeltaDisplay(burst.members[0], image) : "";
            return (
              <ImageRow
                key={`burst:${burst.id}:image:${image.id}`}
                image={relativeCaptureTime ? { ...image, capture_time: relativeCaptureTime } : image}
                currentId={currentId}
                onSelect={onSelect}
                className={"burst-member"}
              />
            );
          })}
        </div>
      ) : null}
    </section>
  );
}

/** Display one picture and scroll the active row into view through a ref effect. */
export function ImageRow({
  image,
  currentId,
  onSelect,
  className = "",
  burstCount = null,
  isolateActivation = false,
}: ImageRowProps): ComponentChildren {
  const { state } = useReviewContext();
  const rowRef = useRef<HTMLButtonElement>(null);
  const isActive = image.id === currentId;
  useEffect((): (() => void) | undefined => {
    if (!isActive) return;
    const frame = requestAnimationFrame((): void =>
      rowRef.current?.scrollIntoView({ block: "nearest", inline: "nearest" }),
    );
    return (): void => cancelAnimationFrame(frame);
  }, [isActive, currentId]);
  const progress = renderProgressSummary(
    image,
    state.localRetouchDirty && image.id === state.currentId,
    profilesAreImplicitOnly(state, image),
  );
  const labels = imageLabels(image);
  const thumbnailUrl = image.thumbnail_url || image.preview_url;
  const cameraModel = sidebarCameraModel(image.exif?.camera_model);
  return (
    <button
      ref={rowRef}
      type={"button"}
      class={`image-row${className ? ` ${className}` : ""}${isActive ? " active" : ""}`}
      aria-current={isActive ? "true" : undefined}
      onKeyDown={isolateActivation ? isolateBurstActivation : undefined}
      onClick={(): void => {
        void onSelect(image).catch((error: unknown): void => console.error(error));
      }}
    >
      <img
        class={"image-row-thumb"}
        alt={""}
        src={thumbnailUrl ? versionedUrl(thumbnailUrl, image.preview_updated_at || image.updated_at) : undefined}
        loading={"lazy"}
        decoding={"async"}
        fetchpriority={"low"}
      />
      <div class={"image-row-title"} title={image.relative_path || image.file_name}>
        <span class={"image-row-title-text"}>
          <span class={"image-row-file-name"}>{sidebarFileStem(image.file_name)}</span>
          {cameraModel ? (
            <span class={"image-row-camera-model"} title={image.exif.camera_model || undefined}>
              {cameraModel}
            </span>
          ) : null}
        </span>
        {burstCount ? (
          <span class={"burst-count"} title={`${burstCount} burst pictures visible`}>
            {burstCount}
          </span>
        ) : null}
      </div>
      {image.capture_time ? (
        <span class={"image-row-capture-time"} title={image.capture_time}>
          {image.capture_time}
        </span>
      ) : null}
      <div class={"image-row-meta"}>
        <span class={"image-row-rating"}>
          {image.rating}
          {labels.length > 0 ? <LabelBadges labels={labels} /> : null}
        </span>
        <span class={"image-row-progress"} title={progress.title}>
          {progress.text}
        </span>
      </div>
      <span class={`image-row-indicator ${progress.state}`} title={progress.title} aria-label={progress.title} />
    </button>
  );
}

/** Offer the existing monochrome filters through the parent review action. */
export function BwFilterControls({ profile, onChange }: BwFilterControlsProps): ComponentChildren {
  const active = normalizeBwFilter(profile.bw_filter);
  return (
    <span class={"bw-filter-controls"} role={"group"} aria-label={"Black-and-white filter"}>
      {BW_FILTERS.map((filter) => (
        <button
          key={filter}
          type={"button"}
          class={normalizeBwFilter(filter) === active ? "active" : ""}
          title={`${BW_FILTER_NAMES.get(filter)} black-and-white filter`}
          aria-label={filter === "none" ? "No black-and-white filter" : `${filter} black-and-white filter`}
          aria-pressed={normalizeBwFilter(filter) === active ? "true" : "false"}
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            if (normalizeBwFilter(filter) === active) return;
            if (onChange) void onChange(profile, filter).catch((error: unknown): void => console.error(error));
          }}
        >
          {BW_FILTER_LABELS.get(filter)}
        </button>
      ))}
    </span>
  );
}

/** Render selected profile variants with reactive availability and touch double-tap tracking. */
export function ProfileList({ image, onSelect, onToggleEnabled, onSolo }: ProfileListProps): ComponentChildren {
  const { state } = useReviewContext();
  const profileDoubleTap = useRef<{ profileIndex: number | null; at: number }>({ profileIndex: null, at: 0 });
  const [portraitProfiles, setPortraitProfiles] = useState<Record<string, boolean>>({});
  if (!image) return null;
  const previewProfile = selectedProfile(image, state);
  const profiles = image.profiles || [];
  const canSolo = profiles.length > 1;
  return profiles.map((profile) => {
    const displayName = profileDisplayName(profile);
    const downloadTitle = profileDownloadTitle(profile, displayName);
    const cardUrl = profile.url || image.preview_url;
    const available = isSoocProfile(profile) || profile.enabled !== false;
    const display = profileDisplayState(image, profile, state.localRetouchDirty && image.id === state.currentId);
    const portraitKey = `${image.id}:${profile.profile_index}:${profile.updated_at}`;
    const isPortrait = portraitProfiles[portraitKey] ?? isPortraitRenderProfile(profile);
    const sourceStatus = profile.url ? display.text : `${display.text} | preview`;
    const classes = [
      "profile-card",
      profile.profile_index === previewProfile?.profile_index ? "active" : "",
      profile.url ? "" : "pending",
      isPortrait ? "portrait" : "",
      display.state,
      available ? "availability-enabled" : "availability-disabled",
    ]
      .filter(Boolean)
      .join(" ");
    return (
      <div key={profile.profile_index} class={"profile-entry"}>
        <button
          type={"button"}
          class={classes}
          onClick={(): void => {
            void onSelect(profile).catch((error: unknown): void => console.error(error));
          }}
          onDblClick={(event) => {
            if (!canSolo) return;
            event.preventDefault();
            onSolo(profile).catch((error) => console.error(error));
          }}
          onPointerUp={(event) => {
            if (!canSolo || event.pointerType === "mouse") return;
            const now = Date.now();
            const sameProfile = profileDoubleTap.current.profileIndex === profile.profile_index;
            const isDoubleTap = sameProfile && now - profileDoubleTap.current.at < 450;
            profileDoubleTap.current.profileIndex = profile.profile_index;
            profileDoubleTap.current.at = now;
            if (!isDoubleTap) return;
            event.preventDefault();
            onSolo(profile).catch((error) => console.error(error));
          }}
        >
          <input
            type={"checkbox"}
            class={"profile-availability"}
            checked={available}
            disabled={isSoocProfile(profile)}
            title={
              isSoocProfile(profile)
                ? "SOOC remains available"
                : available
                  ? "Available for this picture"
                  : "Disabled for this picture"
            }
            aria-label={`Enable ${displayName}`}
            onClick={(event) => event.stopPropagation()}
            onChange={(event) => {
              event.stopPropagation();
              onToggleEnabled(profile).catch((error) => console.error(error));
            }}
          />
          {cardUrl ? (
            <img
              src={versionedUrl(cardUrl, profile.url ? profile.updated_at : image.preview_updated_at)}
              alt={displayName}
              loading={profile.profile_index === previewProfile?.profile_index ? "eager" : "lazy"}
              decoding={"async"}
              fetchpriority={profile.profile_index === previewProfile?.profile_index ? "high" : "low"}
              onLoad={(event) => {
                if (isPortraitRenderProfile(profile)) return;
                const portrait = event.currentTarget.naturalHeight > event.currentTarget.naturalWidth;
                setPortraitProfiles((previous) =>
                  previous[portraitKey] === portrait ? previous : { ...previous, [portraitKey]: portrait },
                );
              }}
            />
          ) : null}
          <div class={"profile-name"}>{displayName}</div>
          <div class={"profile-status"} title={display.title}>
            {`${sourceStatus} | ${available ? "available" : "off"}`}
          </div>
        </button>
        {profile.url ? (
          <a
            class={"profile-download"}
            href={versionedUrl(profile.url, profile.updated_at)}
            download={profileDownloadName(image, profile)}
            title={downloadTitle}
            aria-label={downloadTitle}
            onClick={(event) => event.stopPropagation()}
          >
            {"DL"}
          </a>
        ) : null}
      </div>
    );
  });
}

/** Render compact color labels consistently across metadata and picture rows. */
export function LabelBadges({ labels }: { labels: string[] }): ComponentChildren {
  return (
    <span class={"label-badges"} title={labels.join(", ")} aria-label={labels.join(", ")}>
      {labels.map((label) => (
        <span key={label} class={"label-badge"} data-label={label}>
          {labelLetter(label)}
        </span>
      ))}
    </span>
  );
}
