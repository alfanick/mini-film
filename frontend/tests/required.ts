/** Assert fixture membership explicitly so tests fail clearly rather than bypassing indexed-access safety. */

/** Retrieve an expected fixture item, rejecting missing data before any behavioral assertion uses it. */
export function required<T>(value: T): NonNullable<T> {
  if (value === undefined || value === null) throw new Error("Expected fixture item is missing");
  return value;
}
