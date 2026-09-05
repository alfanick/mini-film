/** Panorama status formatting stays independent of the asynchronous project editor. */
import type { ReviewPanoramaProject } from "../core/types";
import { capitalize } from "./common";

/** Describe project progress with the established panorama labels. */
export function panoramaStatusText(project: ReviewPanoramaProject | null): string {
  if (!project) return "Select at least two sources";
  if (project.status === "ready") return "Previews ready";
  if (project.status === "complete") return project.output_file_name || "Panorama complete";
  if (project.status === "failed") return project.error || "Panorama failed";
  if (project.status === "interrupted") return project.error || "Panorama interrupted";
  if (project.status === "draft") return "Draft";
  const stage = String(project.progress_stage || project.status).replaceAll("-", " ");
  return capitalize(stage);
}
