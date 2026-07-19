/**
 * Convert a snake_case key to a human-readable label.
 * "hallucinated_imports" → "Hallucinated Imports"
 */
export function formatLabel(key) {
  return key.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}
