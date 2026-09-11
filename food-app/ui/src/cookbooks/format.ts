import type {
  Chapter,
  ChunkReport,
  Eta,
  Flag,
  Item,
  Measure,
  Span,
} from "../bridge";

/** Money with enough digits to see a fraction of a cent. */
export function formatCost(usd: number | null | undefined): string {
  if (usd === null || usd === undefined || !Number.isFinite(usd)) return "—";
  if (usd === 0) return "$0";
  if (usd < 0.0001) return "<$0.0001";
  return `$${usd < 1 ? usd.toFixed(4) : usd.toFixed(2)}`;
}
export function formatCostRange(low: number, high: number): string {
  return low === high
    ? formatCost(low)
    : `${formatCost(low)}–${formatCost(high)}`;
}
/** Milliseconds as the coarsest unit that still carries information. */
export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms) || ms < 0)
    return "—";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
  const minutes = Math.floor(ms / 60000);
  const seconds = Math.round((ms % 60000) / 1000);
  return seconds ? `${minutes} m ${seconds} s` : `${minutes} m`;
}
export function formatDurationRange(low: number, high: number): string {
  return low === high
    ? formatDuration(low)
    : `${formatDuration(low)}–${formatDuration(high)}`;
}
/** What remains of an extraction, as the backend's own low–high window. */
export function formatEta(eta: Eta): string {
  const low = Math.min(eta.remaining_low_ms, eta.remaining_high_ms);
  const high = Math.max(eta.remaining_low_ms, eta.remaining_high_ms);
  if (high <= 0) return "finishing";
  return `~${formatDurationRange(low, high)} left`;
}
export function formatPercent(
  value: number | null | undefined,
  digits = 0,
): string {
  return value === null || value === undefined || !Number.isFinite(value)
    ? "—"
    : `${(value * 100).toFixed(digits)}%`;
}
export function formatCount(value: number): string {
  return value.toLocaleString("en-US");
}
export function formatDate(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime())
    ? iso
    : date.toLocaleString(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
      });
}
/** Where an item came from, for the provenance line. */
export function formatSpan(span: Span): string {
  const page = span.page ? ` · page ${span.page}` : "";
  return `${span.doc_path}${page} · lines ${span.start}–${span.end}`;
}
export function describeFlag(flag: Flag): string {
  switch (flag.flag) {
    case "low_amount_parse_rate":
      return `low amount parse rate ${formatPercent(flag.rate)} over ${flag.lines} lines`;
    case "ingredient_like_ignored":
      return `${flag.count} ingredient-like lines ignored`;
    case "recipe_without_steps":
      return `recipe without steps: ${flag.title}`;
    case "missing_nav_title":
      return `missing contents title “${flag.title}” (line ${flag.line})`;
    case "phantom_title":
      return `phantom title “${flag.title}”`;
    case "truncated":
      return "truncated response";
    case "caption_as_title":
      return `caption read as a title (line ${flag.line})`;
    case "unassigned_lines":
      return `${flag.count} unassigned lines`;
  }
}
export interface ItemCounts {
  ingredients: number;
  steps: number;
  photos: number;
  refs: number;
}
export function itemCounts(item: Item): ItemCounts {
  if (item.kind === "recipe") {
    const sections = item.sections;
    return {
      ingredients: sections.reduce((n, s) => n + s.ingredients.length, 0),
      steps: sections.reduce((n, s) => n + s.steps.length, 0),
      photos: item.photos.length,
      refs:
        sections.reduce(
          (n, s) =>
            n +
            s.ingredients.filter((line) => line.ref).length +
            s.steps.reduce((m, step) => m + step.refs.length, 0),
          0,
        ) + item.notes.reduce((n, note) => n + note.refs.length, 0),
    };
  }
  return {
    ingredients: 0,
    steps: item.kind === "technique" ? item.steps.length : 0,
    photos: item.photos.length,
    refs: 0,
  };
}
export type TreeRow =
  | { kind: "chapter"; key: string; title: string; items: number }
  | {
      kind: "item";
      key: string;
      item: Item;
      counts: ItemCounts;
      chapter: string;
    };
/**
 * Chapters and their items in reading order, as one list for rendering. A
 * filter keeps items whose title or unique name matches, and drops chapters
 * that keep nothing.
 */
export function flattenChapters(chapters: Chapter[], filter = ""): TreeRow[] {
  const needle = filter.trim().toLowerCase();
  const rows: TreeRow[] = [];
  for (const [index, chapter] of chapters.entries()) {
    const title = chapter.title ?? "Front matter";
    const items = chapter.items.filter(
      (item) =>
        !needle ||
        item.title.toLowerCase().includes(needle) ||
        item.name.toLowerCase().includes(needle),
    );
    if (!items.length) continue;
    rows.push({
      kind: "chapter",
      key: chapter.id || `ch${index}`,
      title,
      items: items.length,
    });
    for (const item of items)
      rows.push({
        kind: "item",
        key: item.id,
        item,
        counts: itemCounts(item),
        chapter: title,
      });
  }
  return rows;
}
export function allItems(chapters: Chapter[]): Item[] {
  return chapters.flatMap((chapter) => chapter.items);
}
export function findItem(chapters: Chapter[], id: string): Item | undefined {
  return allItems(chapters).find((item) => item.id === id);
}
/** Failed chunks first, then flagged ones, then the rest in id order. */
export function orderChunks(chunks: ChunkReport[]): ChunkReport[] {
  const rank = (chunk: ChunkReport) =>
    chunk.status === "failed" ? 0 : chunk.flags.length ? 1 : 2;
  return [...chunks].sort(
    (a, b) => rank(a) - rank(b) || a.id.localeCompare(b.id),
  );
}

const number = (value: number) =>
  Number.isInteger(value) ? String(value) : String(Number(value.toFixed(3)));
/** One parsed amount, with its range when the line carried one. */
export function formatMeasure(measure: Measure): string {
  const value =
    measure.upper_value === null || measure.upper_value === undefined
      ? number(measure.value)
      : `${number(measure.value)}–${number(measure.upper_value)}`;
  return measure.unit ? `${value} ${measure.unit}` : value;
}
export function formatAmounts(amounts: Measure[]): string {
  return amounts.map(formatMeasure).join(" / ");
}
