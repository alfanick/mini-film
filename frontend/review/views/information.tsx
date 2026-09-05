/**
 * Reactive information views render controlled tool state; stable component identities preserve focus and open
 * details.
 */
import { createContext } from "preact";
import { useContext } from "preact/hooks";
import type { ToolsController } from "../tools/use-tools";
import type { ReviewState, ReviewProfileMetadata, ReviewExif } from "../core/types";

import type { ComponentChildren } from "preact";

/** Current profile/command state plus detail loading and display helpers for the information dialogs. */
export interface InformationViewDependencies {
  closeCommandInvocation: ToolsController["closeCommandInvocation"];
  closeProfileInfo: ToolsController["closeProfileInfo"];
  commandInvocationCopyText: typeof import("../tools/information-helpers").commandInvocationCopyText;
  commandInvocationDisplayValue: typeof import("../tools/information-helpers").commandInvocationDisplayValue;
  commandInvocationLines: typeof import("../tools/information-helpers").commandInvocationLines;
  findImage: ToolsController["findImage"];
  loadProfilePp3: ToolsController["loadProfilePp3"];
  profileByIndex: ToolsController["profileByIndex"];
  profileDisplayName: typeof import("../core/selectors").profileDisplayName;
  profilePp3DownloadName: typeof import("../tools/information-helpers").profilePp3DownloadName;
  profilePp3Key: typeof import("../tools/information-helpers").profilePp3Key;
  profilePp3Url: typeof import("../tools/information-helpers").profilePp3Url;
  renderAdjustments: typeof import("../tools/information-helpers").renderAdjustments;
  renderGrain: typeof import("../tools/information-helpers").renderGrain;
  renderPp3Adjustments: typeof import("../tools/information-helpers").renderPp3Adjustments;
  renderSharpening: typeof import("../tools/information-helpers").renderSharpening;
  reviewUrl: typeof import("../core/api").reviewUrl;
  state: ReviewState;
  versionedUrl: typeof import("../core/selectors").versionedUrl;
}

export const InformationViewContext = createContext<InformationViewDependencies | null>(null);

/** Read the current dialog dependencies from its provider instead of retaining initial factory closures. */
function useInformationView(): InformationViewDependencies {
  const value = useContext(InformationViewContext);
  if (!value) throw new Error("Information views require their tool provider");
  return value;
}

/** Render profile info overlay from current state and typed callbacks. */
export function ProfileInfoOverlay(): ComponentChildren {
  const {
    closeProfileInfo,
    findImage,
    loadProfilePp3,
    profileByIndex,
    profileDisplayName,
    profilePp3DownloadName,
    profilePp3Key,
    profilePp3Url,
    renderAdjustments,
    renderGrain,
    renderPp3Adjustments,
    renderSharpening,
    reviewUrl,
    state,
    versionedUrl,
  } = useInformationView();
  const profile = profileByIndex(state.profileInfoProfileIndex);
  if (!profile) return null;
  const metadata: Partial<ReviewProfileMetadata> = profile.metadata || {};
  const image = findImage(state.currentId);
  const exif: Partial<ReviewExif> = image?.exif || {};
  const haldImage = metadata.has_hald ? `api/profile/${profile.index}/hald` : null;
  const pp3Url = image ? profilePp3Url(image, profile) : null;
  const pp3Key = image ? profilePp3Key(image, profile) : null;
  const pp3State = state.profileInfoPp3.key === pp3Key ? state.profileInfoPp3 : null;
  const pp3Text = pp3State?.error || pp3State?.text || "Loading...";
  return (
    <section class={"profile-info-card"} role={"dialog"} aria-modal={"true"}>
      <header class={"profile-info-header"}>
        <div>
          <h3>{"Profile info"}</h3>
          <p>{profileDisplayName(profile)}</p>
        </div>
        <button
          class={"profile-info-close"}
          type={"button"}
          aria-label={"Close profile info"}
          onClick={(event) => {
            event.preventDefault();
            closeProfileInfo();
          }}
        >
          {"×"}
        </button>
      </header>
      <div class={"profile-info-grid"}>
        {ProfileInfoRow("Profile", metadata.profile_name || "—")}
        {ProfileInfoRow("Profile UUID", metadata.profile_uuid || "—")}
        {ProfileInfoRow("Look", metadata.look_name || "—")}
        {ProfileInfoRow("Look UUID", metadata.look_uuid || "—")}
        {ProfileInfoRow("Source profile", metadata.source_profile_name || "—")}
        {ProfileInfoRow("Source UUID", metadata.source_profile_uuid || "—")}
        {ProfileInfoRow("Active D-Lighting", exif.active_d_lighting || "—")}
        {ProfileInfoRow("Source adjustments", renderAdjustments(metadata.source_adjustments || {}), true)}
        {ProfileInfoRow("Emulation adjustments", renderAdjustments(metadata.emulation_adjustments || {}), true)}
        {ProfileInfoRow("Source sharpening", renderSharpening(metadata.source_sharpening || {}), true)}
        {ProfileInfoRow("Emulation sharpening", renderSharpening(metadata.emulation_sharpening || {}), true)}
        {ProfileInfoRow("PP3 adjustments", renderPp3Adjustments(metadata.pp3_adjustments || []), true)}
        {ProfileInfoRow("Has Camera Raw settings", metadata.has_camera_raw_settings ? "Yes" : "No")}
        {ProfileInfoRow("Has HALD LUT", metadata.has_hald ? "Yes" : "No")}
        {ProfileInfoRow("Has PP3", metadata.has_pp3 ? "Yes" : "No")}
        {ProfileInfoRow("Grain", renderGrain(metadata.grain))}
        {ProfileInfoRow("PP3 file", metadata.pp3_name || "—")}
      </div>
      {haldImage ? (
        <div class={"profile-info-hald"}>
          <img src={versionedUrl(haldImage, state.data?.version || "")} alt={"HALD LUT table"} loading={"lazy"} />
        </div>
      ) : null}
      {pp3Url && image ? (
        <details
          key={pp3Key}
          class={"profile-info-details"}
          onToggle={(event) => {
            if (event.currentTarget.open && image) void loadProfilePp3(image, profile);
          }}
        >
          <summary>{"Complete PP3"}</summary>
          <div class={"profile-info-pp3-actions"}>
            <a
              href={reviewUrl(pp3Url)}
              download={profilePp3DownloadName(image, profile)}
              class={"profile-info-pp3-download"}
            >
              {"Download PP3"}
            </a>
          </div>
          <pre class={`profile-info-pp3 ${pp3State?.error ? "profile-info-pp3-error" : ""}`}>{pp3Text}</pre>
        </details>
      ) : null}
      <details class={"profile-info-details"}>
        <summary>{"Advanced metadata"}</summary>
        <pre class={"profile-info-json"}>{JSON.stringify(metadata, null, 2)}</pre>
      </details>
    </section>
  );
}

/** Render profile info row from current state and typed callbacks. */
export function ProfileInfoRow(label: string, value: ComponentChildren, multiline: boolean = false): ComponentChildren {
  if (value === null || value === undefined || value === "") {
    value = "—";
  }
  return (
    <div class={`profile-info-row ${multiline ? "profile-info-row-multiline" : ""}`}>
      <span class={"profile-info-label"}>{label}</span>
      <span class={"profile-info-value"}>
        {typeof value === "string" ? value : <code class={"profile-info-pre"}>{value}</code>}
      </span>
    </div>
  );
}

/** Render command invocation overlay from current state and typed callbacks. */
export function CommandInvocationOverlay(): ComponentChildren {
  const {
    closeCommandInvocation,
    commandInvocationCopyText,
    commandInvocationDisplayValue,
    commandInvocationLines,
    state,
  } = useInformationView();
  const invocation = state.data?.invocation || "Invocation unavailable.";
  const lines = commandInvocationLines(invocation);
  return (
    <section class={"command-invocation-card"} role={"dialog"} aria-modal={"true"}>
      <header class={"command-invocation-header"}>
        <div>
          <h3>{"Command invocation"}</h3>
          <p>{"This review session was launched with:"}</p>
        </div>
        <button
          class={"command-invocation-close"}
          type={"button"}
          aria-label={"Close command invocation"}
          onClick={(event) => {
            event.preventDefault();
            closeCommandInvocation();
          }}
        >
          {"×"}
        </button>
      </header>
      <div class={"command-invocation-code"} title={invocation}>
        {lines.length === 0
          ? invocation
          : lines.map((line, index, arr) => (
              <div
                class={`command-invocation-line${index === 0 ? "" : " command-invocation-line-indented"}`}
                aria-label={
                  line.type === "single"
                    ? line.value
                    : line.type === "binary-subcommand"
                      ? `${line.value} ${line.subcommand}`
                      : `${line.name} ${line.value}`
                }
              >
                <span class={"command-invocation-line-content"}>
                  {line.type === "binary-subcommand"
                    ? [
                        <span class={"command-invocation-binary"}>{line.value}</span>,
                        <span class={"command-invocation-arg"}>{line.subcommand}</span>,
                      ]
                    : line.type === "pair"
                      ? [
                          <span class={"command-invocation-arg"}>{line.name}</span>,
                          <span class={"command-invocation-value"}>
                            {commandInvocationDisplayValue(
                              line.value,
                              line.name === "--profile" || line.name === "-p" || line.name === "--profile-name",
                            )}
                          </span>,
                        ]
                      : [
                          <span class={line.binary ? "command-invocation-binary" : "command-invocation-arg"}>
                            {line.value}
                          </span>,
                        ]}
                </span>
                {index < arr.length - 1 ? <span class={"command-invocation-continuation"}>{" \\"}</span> : null}
              </div>
            ))}
      </div>
      <div class={"command-invocation-actions"}>
        <button
          type={"button"}
          onClick={() => {
            const copyText = commandInvocationCopyText(state.data?.invocation ?? "");
            if (!copyText) return;
            void navigator.clipboard?.writeText(copyText).catch((error) => {
              console.error(error);
            });
          }}
        >
          {"Copy"}
        </button>
        <button type={"button"} onClick={() => closeCommandInvocation()}>
          {"Close"}
        </button>
      </div>
    </section>
  );
}
