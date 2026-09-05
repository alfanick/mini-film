/**
 * Profile-catalog hierarchy algorithms preserve the sampler grouping and priority semantics independently of
 * component lifecycles.
 */
import type { SamplerEntry, SamplerJob } from "../core/types";
import { capitalize } from "./common";

/** Preserve source proportions while sampler thumbnails load. */
export function samplerMediaStyle(job: SamplerJob | null): { aspectRatio: string } | undefined {
  const width = Number(job?.source_width);
  const height = Number(job?.source_height);
  return Number.isFinite(width) && width > 0 && Number.isFinite(height) && height > 0
    ? { aspectRatio: `${width} / ${height}` }
    : undefined;
}

/** Intermediate catalog branch before redundant path segments are compacted for display. */
export interface SamplerTrieNode {
  part: string;
  entries: SamplerEntry[];
  children: Map<string, SamplerTrieNode>;
}

/** A visible catalog section with ancestor and descendant identities for controlled expansion. */
export interface SamplerSectionData {
  key: string;
  label: string;
  depth: number;
  ancestorKeys: string[];
  entries: SamplerEntry[];
  allEntries: SamplerEntry[];
  children: SamplerSectionData[];
}

/** The display tree and matching reverse lookup used to reveal newly enabled profiles. */
export interface SamplerHierarchy {
  sections: SamplerSectionData[];
  entrySections: Map<string, SamplerSectionData>;
}

/** Build display sections and an entry lookup together so expansion and request priority use identical grouping. */
export function buildSamplerHierarchy(entries: SamplerEntry[]): SamplerHierarchy {
  const root = samplerTrieNode("");
  for (const entry of entries) {
    const parts = Array.isArray(entry.parts) ? entry.parts.map((part) => String(part).trim()).filter(Boolean) : [];
    let node = root;
    for (const part of parts.length > 0 ? parts : ["Profiles"]) {
      if (!node.children.has(part)) node.children.set(part, samplerTrieNode(part));
      node = node.children.get(part)!;
    }
    node.entries.push(entry);
  }
  const entrySections = new Map<string, SamplerSectionData>();
  const sections = samplerTrieChildren(root).map(([part, node]) =>
    buildSamplerSection(node, [part], 0, [], entrySections),
  );
  return { sections, entrySections };
}

/** Create a catalog trie node; keeping children in a map preserves distinct preset path segments. */
export function samplerTrieNode(part: string): SamplerTrieNode {
  return { part, entries: [], children: new Map() };
}

/** Order catalog branches naturally so numbered film speeds and versions remain readable. */
export function samplerTrieChildren(node: SamplerTrieNode): [string, SamplerTrieNode][] {
  return Array.from(node.children.entries()).sort(([left], [right]) =>
    left.localeCompare(right, undefined, { numeric: true }),
  );
}

/** Flatten shallow branches while retaining explicit version and film-speed branches for useful comparisons. */
export function buildSamplerSection(
  node: SamplerTrieNode,
  prefix: string[],
  depth: number,
  ancestorKeys: string[],
  entrySections: Map<string, SamplerSectionData>,
): SamplerSectionData {
  const key = prefix.map(encodeURIComponent).join("/");
  const allEntries = collectSamplerTrieEntries(node);
  const flatten = (depth >= 1 || samplerSubtreeDepth(node) <= 2) && !samplerContainsForcedBranch(node);
  let entries: SamplerEntry[];
  let children: SamplerSectionData[];
  if (flatten) {
    entries = allEntries;
    children = [];
  } else {
    entries = [...node.entries];
    children = [];
    for (const [part, child] of samplerTrieChildren(node)) {
      if (child.children.size === 0 && child.entries.length > 0) {
        entries.push(...child.entries);
      } else {
        children.push(buildSamplerSection(child, [...prefix, part], depth + 1, [...ancestorKeys, key], entrySections));
      }
    }
  }
  entries = samplerSortEntries(entries);
  const section = {
    key,
    label: prefix.join(" "),
    depth,
    ancestorKeys,
    entries,
    allEntries,
    children,
  };
  for (const entry of entries) entrySections.set(entry.key, section);
  return section;
}

/** Collect a whole branch before sorting so collapsed sections retain accurate totals. */
export function collectSamplerTrieEntries(node: SamplerTrieNode): SamplerEntry[] {
  const entries = [...node.entries];
  for (const child of node.children.values()) entries.push(...collectSamplerTrieEntries(child));
  return samplerSortEntries(entries);
}

/** Apply the original locale-aware, numeric ordering to profile entries. */
export function samplerSortEntries(entries: SamplerEntry[]): SamplerEntry[] {
  return [...entries].sort((left, right) => left.name.localeCompare(right.name, undefined, { numeric: true }));
}

/** Measure remaining nesting to decide whether a sampler branch can be flattened. */
export function samplerSubtreeDepth(node: SamplerTrieNode): number {
  if (node.children.size === 0) return 0;
  return 1 + Math.max(...Array.from(node.children.values(), samplerSubtreeDepth));
}

/** Retain explicit version or ISO-speed grouping anywhere below a branch. */
export function samplerContainsForcedBranch(node: SamplerTrieNode): boolean {
  return Array.from(
    node.children,
    ([part, child]) => samplerIsVersionPart(part) || samplerIsFilmSpeedPart(part) || samplerContainsForcedBranch(child),
  ).some(Boolean);
}

/** Recognize version path segments that must stay independently expandable. */
export function samplerIsVersionPart(part: string): boolean {
  return /^v\d+$/i.test(part);
}

/** Recognize plausible film speeds without treating every numeric path segment as an ISO group. */
export function samplerIsFilmSpeedPart(part: string): boolean {
  if (!/^\d+$/.test(part)) return false;
  const speed = Number(part);
  return speed >= 25 && speed <= 12800;
}

/** Describe sampler preparation and rendering progress using the existing labels. */
export function samplerStatusText(job: SamplerJob): string {
  if (job.status === "preparing") return "Preparing neutral TIFF";
  if (job.status === "rendering") return `Rendering profiles${job.failed ? ` | ${job.failed} failed` : ""}`;
  if (job.status === "done") return job.failed ? `Complete | ${job.failed} failed` : "Complete";
  return capitalize(job.status);
}
