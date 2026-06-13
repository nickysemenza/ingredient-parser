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

export const scaleAmount = (amount: Measure, scale: number): Measure => ({
  ...amount,
  value: amount.value * scale,
  upper_value: amount.upper_value ? amount.upper_value * scale : undefined,
});

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
        const displayAmount = val.unit === "whole" ? { ...val, unit: "" } : val;
        return (
          <span
            className="mx-0.5 inline rounded-md border border-blue-200 bg-blue-100 px-1.5 py-0.5 font-semibold text-blue-800"
            key={`measure-${index}`}
          >
            {fmtAmount(displayAmount)}
          </span>
        );
      }
      default:
        return null;
    }
  });
};
