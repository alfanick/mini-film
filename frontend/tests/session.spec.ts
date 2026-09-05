/**
 * Lock the original metadata and navigation wire behavior independently of UI
 * rendering so the hook migration cannot silently rewrite photographers' data.
 */
import { required } from "./required";
import { expect, test } from "@playwright/test";
import { parseTags } from "../review/session/use-edits";
import {
  carriedProfileIndex,
  profileBwFilters,
  reviewRequestBody,
  toggleEnabledProfile,
} from "../review/session/review-requests";
import type { ReviewUpdateRequest } from "../review/core/types";
import { reviewFixture } from "./fixtures";

// Tag separators are normalized, but duplicate or zero-prefixed strings remain user-owned.
test("tag parsing preserves duplicate tags, order, and numeric-looking strings", (): void => {
  expect(parseTags(" 12, 12\t007, blue-sky 42 ")).toEqual(["12", "12", "007", "blue-sky", "42"]);
  expect(parseTags(" , \n\t")).toEqual([]);
});

// JSON omission determines whether the server updates availability or publish selection.
test("review payloads preserve metadata, explicit values, and omitted optional fields", (): void => {
  const image = required(reviewFixture().images[0]);
  const body = reviewRequestBody(image, { rating: 0, labels: [], notes: "", tags: ["007", "007"] });
  const serialized = JSON.parse(JSON.stringify(body)) as ReviewUpdateRequest;
  expect(serialized).toMatchObject({
    image_id: 1,
    rating: 0,
    label: "none",
    labels: [],
    notes: "",
    tags: ["007", "007"],
    retouch: image.retouch,
    publish_profile_indexes: [0, 1],
    profile_bw_filters: [],
    advance_after_update: false,
  });
  expect(Object.keys(serialized)).not.toContain("selected_profile_index");
  expect(Object.keys(serialized)).not.toContain("enabled_profile_indexes");
  const availability = JSON.parse(
    JSON.stringify(
      reviewRequestBody(image, {
        enabled_profile_indexes: [],
        selected_profile_index: 1,
        advance_after_update: true,
      }),
    ),
  ) as ReviewUpdateRequest;
  expect(availability.enabled_profile_indexes).toEqual([]);
  expect(availability.selected_profile_index).toBe(1);
  expect(availability.advance_after_update).toBe(true);
  expect(Object.keys(availability)).not.toContain("publish_profile_indexes");
});

// Camera renditions remain available separately from creative-profile enablement.
test("profile enablement excludes SOOC and filters retain the original normalization", (): void => {
  const image = required(reviewFixture().images[0]);
  image.profiles.push({ ...required(image.profiles[0]), profile_index: 1000000000, profile_stem: "sooc" });
  expect(toggleEnabledProfile(image, 0)).toEqual([1]);
  required(image.profiles[1]).enabled = false;
  expect(toggleEnabledProfile(image, 1)).toEqual([0, 1]);
  image.profile_bw_filters = [
    { profile_index: 1, filter: "yellow" },
    { profile_index: 0, filter: "green" },
    { profile_index: 1, filter: "red" },
    { profile_index: 99, filter: "orange" },
    { profile_index: 1000000000, filter: "none" },
  ];
  expect(profileBwFilters(image)).toEqual([
    { profile_index: 0, filter: "green" },
    { profile_index: 1, filter: "red" },
  ]);
});

// Navigation and rating advance use published-profile preference, not mere profile presence.
test("profile carry keeps published looks and falls back to the first selected publish variant", (): void => {
  const image = required(reviewFixture().images[0]);
  image.selected_profile_index = 1;
  image.publish_profile_indexes = [0];
  expect(carriedProfileIndex(image, 1)).toBe(0);
  expect(carriedProfileIndex(image, 99)).toBe(0);
  expect(carriedProfileIndex(image, undefined)).toBeUndefined();
  image.publish_profile_indexes = [1, 0];
  expect(carriedProfileIndex(image, 1)).toBeUndefined();
  expect(carriedProfileIndex(image, 0)).toBe(0);
  image.publish_profile_indexes = [];
  expect(carriedProfileIndex(image, 0)).toBeUndefined();
  expect(carriedProfileIndex(image, 99)).toBeUndefined();
});
