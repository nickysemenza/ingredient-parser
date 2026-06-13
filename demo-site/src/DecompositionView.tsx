import type { WDecomposition, WField } from "./wasm";

// Each grammar field gets an underline + legend dot color. `amount` matches the
// rich-text measure highlight (blue), `name` the existing name underline
// (accent/emerald), `modifier` a distinct amber.
const FIELD_STYLES: Record<
  WField,
  { underline: string; dot: string; label: string }
> = {
  amount: { underline: "border-blue-400", dot: "bg-blue-400", label: "amount" },
  name: { underline: "border-accent-400", dot: "bg-accent-400", label: "name" },
  modifier: {
    underline: "border-amber-400",
    dot: "bg-amber-400",
    label: "modifier",
  },
};

const FIELD_ORDER: WField[] = ["amount", "name", "modifier"];

/**
 * Diagnostic-style view of how the grammar carved the line: the (normalized)
 * source in monospace with a colored underline under each amount/name/modifier
 * span, plus a legend. Mirrors `parse-ingredient --explain`. Renders nothing
 * until there's input.
 */
export function DecompositionView({
  decomp,
}: {
  decomp: WDecomposition | undefined;
}) {
  if (!decomp) return null;

  const present = FIELD_ORDER.filter((f) =>
    decomp.segments.some((s) => s.field === f)
  );

  return (
    <div className="mb-5">
      <div className="mb-2 text-sm font-medium text-zinc-500">
        How the grammar carved it
      </div>
      <div className="overflow-x-auto rounded-lg border border-zinc-200 bg-zinc-50 px-4 py-3">
        <div className="font-mono text-base whitespace-pre text-zinc-900">
          {decomp.segments.map((seg, i) =>
            seg.field ? (
              <span
                key={i}
                className={`border-b-2 pb-0.5 ${FIELD_STYLES[seg.field].underline}`}
              >
                {seg.text}
              </span>
            ) : (
              <span key={i} className="text-zinc-400">
                {seg.text}
              </span>
            )
          )}
        </div>
      </div>
      {present.length > 0 ? (
        <div className="mt-2 flex flex-wrap gap-3 text-xs text-zinc-500">
          {present.map((f) => (
            <span key={f} className="inline-flex items-center gap-1.5">
              <span
                className={`h-2 w-2 rounded-full ${FIELD_STYLES[f].dot}`}
              />
              {FIELD_STYLES[f].label}
            </span>
          ))}
        </div>
      ) : (
        <p className="mt-2 text-xs text-zinc-400">
          No grammar decomposition — handled by a recognizer or name-only
          fallback.
        </p>
      )}
    </div>
  );
}
