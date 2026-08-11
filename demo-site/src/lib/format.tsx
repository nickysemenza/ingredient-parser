import { RichItem, wasm, Measure } from "../wasm";

// WASM is guaranteed loaded before this tree mounts (see main.tsx), so every
// call here is synchronous and never guards on the module being ready.
export const fmtAmount = (a: Measure): string => {
  try {
    return wasm.format_amount(a);
  } catch {
    return `${a.value} ${a.unit}`.trim();
  }
};

// wasm.parse_rich_text throws (a raw string) on unparseable input; falling
// back to a single Text chunk keeps a render-path call from unmounting the
// page (there's no error boundary above these sections).
export const safeParseRichText = (text: string, names: string[]): RichItem[] => {
  try {
    return wasm.parse_rich_text(text, names);
  } catch {
    return [{ kind: "Text", value: text }];
  }
};

// Which kinds grow with the recipe now lives in Rust, where the kind table is
// defined. This file used to hand-copy that table, which is the drift the
// wasm boundary exists to prevent — a non-scalable amount (a pan dimension, an
// oven temperature) must survive a resize untouched.
export const scaleAmount = (amount: Measure, scale: number): Measure =>
  wasm.scale_amount(amount, scale);

export const formatRichText = (text: RichItem[]) => {
  return text.map((t, index) => {
    switch (t.kind) {
      case "Text":
        return t.value;
      case "Ing":
        return (
          <span
            className="mx-0.5 inline rounded-md border border-accent-200 bg-accent-100 px-1.5 py-0.5 font-semibold text-accent-800"
            key={`ing-${index}`}
          >
            {t.value}
          </span>
        );
      case "Measure": {
        const val = t.value[t.value.length - 1];
        if (!val) {
          return null;
        }
        return (
          <span
            className="mx-0.5 inline rounded-md border border-blue-200 bg-blue-100 px-1.5 py-0.5 font-semibold text-blue-800"
            key={`measure-${index}`}
          >
            {/* No `whole` special case: Measure's Display already renders a
                bare count with no unit suffix. Blanking the unit here actually
                made it worse — an empty unit parses as Other(""), which
                Display renders with a trailing space. */}
            {fmtAmount(val)}
          </span>
        );
      }
      default:
        return null;
    }
  });
};
