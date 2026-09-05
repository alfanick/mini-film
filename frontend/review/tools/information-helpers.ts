/**
 * Pure information-dialog formatting preserves photographer-visible profile metadata, PP3 downloads, and command
 * quoting.
 */
import type {
  ReviewImage,
  ReviewProfile,
  ReviewProfileMetadata,
  ReviewProfileAdjustments,
  ReviewProfileSharpening,
  ReviewProfilePp3Section,
} from "../core/types";
import { safeDownloadPart, profileDisplayName } from "./common";

/** Format known profile adjustments without inventing unsupported metadata fields. */
export function renderAdjustments(adjustments: Partial<ReviewProfileAdjustments>): string {
  const values = [
    ["exposure", adjustments.exposure],
    ["contrast", adjustments.contrast],
    ["highlights", adjustments.highlights],
    ["shadows", adjustments.shadows],
    ["whites", adjustments.whites],
    ["blacks", adjustments.blacks],
    ["saturation", adjustments.saturation],
    ["vibrance", adjustments.vibrance],
    ["clarity", adjustments.clarity],
  ].map(([key, value]) => `${key}: ${formatNumberField(value, 2)}`);
  return values.join("\n");
}

/** Describe enabled grain only when its source parameters are meaningful. */
export function renderGrain(grain: ReviewProfileMetadata["grain"] | undefined): string {
  if (!grain || !grain.amount) {
    return "off";
  }
  const size = Number(grain.size);
  const frequency = Number(grain.frequency);
  if (!Number.isFinite(size) || !Number.isFinite(frequency)) {
    return "off";
  }
  return `amount=${grain.amount}, size=${size}, frequency=${frequency}`;
}

/** Display the profile sharpening parameters as recorded by the engine. */
export function renderSharpening(sharpening: Partial<ReviewProfileSharpening>): string {
  const values = [
    ["present", sharpening.present],
    ["amount", sharpening.amount],
    ["radius", sharpening.radius],
    ["detail", sharpening.detail],
    ["masking", sharpening.masking],
  ].map(([key, value]) => `${key}: ${formatNumberField(value, 2)}`);
  return values.join("\n");
}

/** Format profile sections and entries while omitting empty section bodies. */
export function renderPp3Adjustments(sections?: ReviewProfilePp3Section[]): string {
  if (!Array.isArray(sections) || sections.length === 0) {
    return "—";
  }
  return sections
    .map((section) => {
      const source = section.source ? `${section.source} ` : "";
      const entries = Array.isArray(section.entries)
        ? section.entries
            .filter((entry) => entry?.key && entry?.value)
            .map((entry) => `${entry.key}=${entry.value}`)
            .join(", ")
        : "";
      return entries ? `${source}[${section.section}]\n${entries}` : "";
    })
    .filter(Boolean)
    .join("\n\n");
}

/** Format numeric metadata compactly and retain meaningful nonnumeric values. */
export function formatNumberField(value: string | number | boolean | null | undefined, digits = 2): string {
  const number = Number(value);
  if (Number.isFinite(number)) {
    return number.toLocaleString("en-US", {
      maximumFractionDigits: digits,
      useGrouping: false,
    });
  }
  return String(value ?? "—");
}

/** Identify the generated PP3 for the selected source and configured profile. */
export function profilePp3Url(image: ReviewImage, profile: ReviewProfile): string {
  return `api/profile/${profile.index}/pp3/${image.id}`;
}

/** Invalidate the PP3 cache when its source image changes. */
export function profilePp3Key(image: ReviewImage, profile: ReviewProfile): string {
  return `${image.id}:${profile.index}:${image.updated_at || ""}`;
}

/** Combine sanitized source and profile stems into a stable PP3 download name. */
export function profilePp3DownloadName(image: ReviewImage, profile: ReviewProfile): string {
  const rawName = image.file_name || image.relative_path || "mini-film";
  const baseName = rawName.replace(/\.[^.]*$/, "");
  const profileName = profile.stem || profile.selector || profileDisplayName(profile);
  return `${safeDownloadPart(baseName)}--${safeDownloadPart(profileName)}.pp3`;
}

/** A shell-token grouping that preserves the recorded binary, subcommand, and option order. */
export type CommandInvocationLine =
  | { type: "binary-subcommand"; value: string; subcommand: string }
  | { type: "single"; value: string; binary?: boolean; name?: string }
  | { type: "pair"; name: string; value: string };

/** Group command tokens into binary, subcommand, and option rows without losing argument order. */
export function commandInvocationLines(invocation: string): CommandInvocationLine[] {
  const tokens = commandInvocationTokens(invocation);
  const lines: CommandInvocationLine[] = [];
  for (let index = 0; index < tokens.length;) {
    const token = tokens[index];
    if (index === 0) {
      if (token !== "" && (tokens[index + 1] === "app" || tokens[index + 1] === "daemon")) {
        lines.push({
          type: "binary-subcommand",
          value: token,
          subcommand: tokens[index + 1],
        });
        index += 2;
        continue;
      }
      lines.push({
        type: "single",
        value: token,
        binary: true,
        name: "binary",
      });
      index += 1;
      continue;
    }

    if (token.startsWith("--") && tokens[index + 1] && !tokens[index + 1].startsWith("--")) {
      let nextIndex = index + 1;
      while (nextIndex < tokens.length && !tokens[nextIndex].startsWith("--")) {
        nextIndex += 1;
      }

      const rawValue = tokens.slice(index + 1, nextIndex);
      lines.push({
        type: "pair",
        name: token,
        value: rawValue.join(" "),
      });
      index = nextIndex;
      continue;
    }

    lines.push({
      type: "single",
      value: token,
    });
    index += 1;
  }
  return lines;
}

/** Tokenize the recorded shell command while preserving quoted paths and escaped characters. */
export function commandInvocationTokens(invocation: string): string[] {
  const tokens = [];
  let current = "";
  let quote: string | null = null;
  let escaped = false;

  for (let index = 0; index < invocation.length; index++) {
    const char = invocation[index];
    const next = invocation[index + 1];

    if (escaped) {
      current += char;
      escaped = false;
      continue;
    }

    if (char === "\\") {
      if (quote === "'") {
        current += char;
      } else if (next !== undefined) {
        current += next;
        index += 1;
      } else {
        current += "\\";
      }
      continue;
    }

    if (quote) {
      if (char === quote) {
        quote = null;
        continue;
      }
      if (quote === '"' && char === "\\") {
        if (next === "\\" || next === '"' || next === "$" || next === "`") {
          current += next;
          index += 1;
          continue;
        }
      }
      current += char;
      continue;
    }

    if (char === "'" || char === '"') {
      quote = char;
      continue;
    }
    if (/\s/.test(char)) {
      if (current.length > 0) {
        tokens.push(current);
        current = "";
      }
      continue;
    }

    current += char;
  }

  if (current.length > 0) tokens.push(current);
  return tokens;
}

/** Quote values for readable command rows without changing the command data. */
export function commandInvocationDisplayValue(value: string, forceQuote = false): string {
  const str = String(value);
  if (!forceQuote && (/^[-+]?\d+(?:\.\d+)?$/.test(str) || !/[\\s"]/.test(str))) {
    return str;
  }
  const escaped = str.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return `"${escaped}"`;
}

/** Build a shell-safe invocation for the dialog clipboard action. */
export function commandInvocationCopyText(invocation: string): string {
  const tokens = commandInvocationTokens(invocation);
  return tokens.map(commandInvocationShellEscape).join(" ");
}

/** Quote a single argument, including embedded apostrophes, for shell pasting. */
export function commandInvocationShellEscape(value: string): string {
  const raw = String(value);
  if (raw.length === 0) return "''";
  if (/^[A-Za-z0-9._/+=:@,-]+$/.test(raw)) return raw;
  if (!raw.includes("'")) {
    return `'${raw}'`;
  }
  return `'${raw.replace(/'/g, "'\"'\"'")}'`;
}

/** Accept either configured or rendered profile identities used by the review API. */
export function profileRenderIndex(
  profile: { index?: number; profile_index?: number } | null | undefined,
): number | null {
  if (profile?.index !== undefined && Number.isFinite(Number(profile.index))) return Number(profile.index);
  if (profile?.profile_index !== undefined && Number.isFinite(Number(profile.profile_index)))
    return Number(profile.profile_index);
  return null;
}
