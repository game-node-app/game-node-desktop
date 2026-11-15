/** Joins truthy class names, keeping call sites free of template-string noise. */
export function cx(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(' ');
}

/**
 * Merges a base set of Mantine `classNames` with per-call overrides. A class
 * supplied twice is concatenated so the caller keeps the primitive's defaults
 * unless its own utility intentionally wins by cascade order.
 */
export function mergeClassNames<T extends Partial<Record<string, string>>>(
  base: T,
  overrides?: T,
): T {
  if (!overrides) return base;
  const merged: Partial<Record<string, string>> = { ...base };
  for (const [key, value] of Object.entries(overrides)) {
    merged[key] = cx(merged[key], value);
  }
  return merged as T;
}
