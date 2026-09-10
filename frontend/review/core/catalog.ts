/** Keep picture identities, pending intentions, and ordering independent so one edit touches one reactive cell. */
import { batch, computed, signal, type ReadonlySignal, type Signal } from "@preact/signals";
import { projectReviewIntent, type ReviewIntent } from "../session/commands";
import type { RetouchObservation, ReviewImageObservation } from "./types";

/** An intention's queue identity allows acknowledgement to remove only its own optimistic projection. */
interface ImageCommand {
  readonly id: number;
  readonly intent: ReviewIntent;
}

/** Present pictures retain a canonical cell; removed pictures survive only while observed or locally dirty. */
interface ImageCell {
  readonly confirmed: Signal<ReviewImageObservation | null>;
  readonly commands: Signal<readonly ImageCommand[]>;
  readonly retouch: Signal<RetouchObservation | null>;
  readonly projected: ReadonlySignal<ReviewImageObservation | null>;
  observed: boolean;
}

/** Own bounded per-picture projections without rebuilding every image for a local slider or rating change. */
export class ReviewCatalog {
  readonly #cells = new Map<number, ImageCell>();
  readonly #commands = new Map<number, number>();
  readonly #order = signal<readonly number[]>([]);
  readonly #dirty = signal<ReadonlySet<number>>(new Set());
  readonly #membership = signal(0);
  #nextCommand = 0;

  readonly order: ReadonlySignal<readonly number[]> = this.#order;
  readonly dirtyIds: ReadonlySignal<ReadonlySet<number>> = this.#dirty;
  readonly images: ReadonlySignal<readonly ReviewImageObservation[]> = computed(() =>
    this.#order.value.flatMap((id): readonly ReviewImageObservation[] => {
      const image = this.#cells.get(id)?.projected.value;
      return image ? [image] : [];
    }),
  );
  readonly byId: ReadonlySignal<ReadonlyMap<number, ReviewImageObservation>> = computed(
    () => new Map(this.images.value.map((image) => [image.id, image])),
  );

  /** Reconcile server observations, retaining equal identities already normalized by the wire reconciler. */
  replace(images: readonly ReviewImageObservation[]): void {
    batch((): void => {
      const order = images.map((image) => image.id);
      const present = new Set(order);
      for (const image of images) this.#cell(image.id).confirmed.value = image;
      for (const [id, cell] of this.#cells) {
        if (present.has(id)) continue;
        cell.confirmed.value = null;
        this.#release(id, cell);
      }
      const previous = this.#order.peek();
      if (previous.length !== order.length || previous.some((id, index) => id !== order[index])) {
        this.#order.value = order;
        this.#membership.value += 1;
      }
    });
  }

  /** Subscribe directly to an existing image; uncached missing lookups follow later ingestion through membership. */
  image(id: number): ReadonlySignal<ReviewImageObservation | null> {
    const cell = this.#cells.get(id);
    return (
      cell?.projected ??
      computed((): ReviewImageObservation | null => {
        void this.#membership.value;
        return this.#cells.get(id)?.projected.value ?? null;
      })
    );
  }

  /** Read the server baseline for one command without materializing or scanning an aggregate snapshot. */
  confirmedImage(id: number): ReviewImageObservation | null {
    return this.#cells.get(id)?.confirmed.peek() ?? null;
  }

  /** Publish one optimistic intention without traversing the rest of the catalog. */
  begin(imageId: number, intent: ReviewIntent): number {
    const id = ++this.#nextCommand;
    const cell = this.#cell(imageId);
    this.#commands.set(id, imageId);
    cell.commands.value = [...cell.commands.peek(), { id, intent }];
    return id;
  }

  /** Acknowledge exactly one queued operation; newer intentions continue to project over confirmed data. */
  finish(id: number): void {
    const imageId = this.#commands.get(id);
    if (imageId === undefined) return;
    this.#commands.delete(id);
    const cell = this.#cells.get(imageId);
    if (!cell) return;
    cell.commands.value = cell.commands.peek().filter((command) => command.id !== id);
    this.#release(imageId, cell);
  }

  /** Keep draft retouch visible until its owner acknowledges the matching revision. */
  setRetouch(id: number, value: RetouchObservation | null): void {
    const cell = this.#cell(id);
    if (cell.retouch.peek() === value) return;
    batch((): void => {
      cell.retouch.value = value;
      const dirty = new Set(this.#dirty.peek());
      if (value) dirty.add(id);
      else dirty.delete(id);
      this.#dirty.value = dirty;
      this.#release(id, cell);
    });
  }

  /** Expose only a count for deterministic cache-bound tests, never the mutable backing collections. */
  get retainedImageCount(): number {
    return this.#cells.size;
  }

  /** Build one immutable observation from the latest server image and only this image's local ownership. */
  #cell(id: number): ImageCell {
    const existing = this.#cells.get(id);
    if (existing) return existing;
    const confirmed = signal<ReviewImageObservation | null>(null);
    const commands = signal<readonly ImageCommand[]>([]);
    const retouch = signal<RetouchObservation | null>(null);
    const cell: ImageCell = {
      confirmed,
      commands,
      retouch,
      observed: false,
      projected: computed(
        (): ReviewImageObservation | null => {
          let image = confirmed.value;
          const pending = commands.value;
          const draft = retouch.value;
          if (!image) return null;
          for (const command of pending) image = projectReviewIntent(image, command.intent);
          return draft ? { ...image, retouch: draft } : image;
        },
        {
          watched: (): void => {
            cell.observed = true;
          },
          unwatched: (): void => {
            cell.observed = false;
            this.#release(id, cell);
          },
        },
      ),
    };
    this.#cells.set(id, cell);
    return cell;
  }

  /** An observed removed cell must still receive re-ingestion; unobserved clean tombstones have no owner. */
  #release(id: number, cell: ImageCell): void {
    if (!cell.observed && !cell.confirmed.peek() && !cell.commands.peek().length && !cell.retouch.peek()) {
      if (this.#cells.get(id) === cell) this.#cells.delete(id);
    }
  }
}
