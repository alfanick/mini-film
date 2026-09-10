/** Exercise picture-local reactivity and cache ownership without a DOM, server, or wall-clock timing assumptions. */
import { expect, test } from "@playwright/test";
import { effect } from "@preact/signals";
import { ReviewCatalog } from "../review/core/catalog";
import { defaultRetouch } from "../review/core/selectors";
import { reviewFixture } from "./fixtures";
import { required } from "./required";

for (const size of [1000, 10000]) {
  test(`a local edit only notifies its picture in a ${size}-image catalog`, (): void => {
    const catalog = new ReviewCatalog();
    const source = required(reviewFixture().images[0]);
    catalog.replace(Array.from({ length: size }, (_, index) => ({ ...source, id: index + 1 })));
    let changedReads = 0;
    let otherReads = 0;
    let orderReads = 0;
    const stopChanged = effect((): void => {
      void catalog.image(1).value;
      changedReads += 1;
    });
    const stopOther = effect((): void => {
      void catalog.image(size).value;
      otherReads += 1;
    });
    const stopOrder = effect((): void => {
      void catalog.order.value;
      orderReads += 1;
    });
    try {
      const ticket = catalog.begin(1, { kind: "fields", fields: { rating: 5 } });
      expect(required(catalog.image(1).peek()).rating).toBe(5);
      expect([changedReads, otherReads, orderReads]).toEqual([2, 1, 1]);
      catalog.finish(ticket);
      catalog.setRetouch(1, { ...defaultRetouch(), rotation_degrees: 12 });
      expect(required(catalog.image(1).peek()).retouch.rotation_degrees).toBe(12);
      expect([changedReads, otherReads, orderReads]).toEqual([4, 1, 1]);
      expect(catalog.retainedImageCount).toBe(size);
    } finally {
      stopChanged();
      stopOther();
      stopOrder();
    }
  });
}

test("removed subscriptions retain identity only while observed and receive later ingestion", (): void => {
  const catalog = new ReviewCatalog();
  const image = required(reviewFixture().images[0]);
  catalog.replace([image]);
  const observation = catalog.image(image.id);
  const values: (number | null)[] = [];
  const stop = effect((): void => {
    values.push(observation.value?.rating ?? null);
  });
  catalog.replace([]);
  expect(observation.peek()).toBeNull();
  expect(catalog.retainedImageCount).toBe(1);
  catalog.replace([{ ...image, rating: 5 }]);
  expect(observation.peek()?.rating).toBe(5);
  expect(values).toEqual([image.rating, null, 5]);
  catalog.replace([]);
  stop();
  expect(catalog.retainedImageCount).toBe(0);
  for (let id = 1; id <= 10000; id += 1) expect(catalog.image(id).peek()).toBeNull();
  expect(catalog.retainedImageCount).toBe(0);
});

test("missing lookups observe new membership without an indefinitely retained cache entry", (): void => {
  const catalog = new ReviewCatalog();
  const image = required(reviewFixture().images[0]);
  const observation = catalog.image(image.id);
  const values: (number | null)[] = [];
  const stop = effect((): void => {
    values.push(observation.value?.id ?? null);
  });
  try {
    expect(catalog.retainedImageCount).toBe(0);
    catalog.replace([image]);
    expect(values).toEqual([null, image.id]);
    catalog.replace([]);
    expect(values).toEqual([null, image.id, null]);
  } finally {
    stop();
  }
  expect(catalog.retainedImageCount).toBe(0);
});
