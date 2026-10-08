// Recipe pieces: instructions with their measures and ingredients marked, and
// a scale control.
import { Minus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import type { RichChunk } from "../api";
import { cn } from "../lib/cn";
import { SCALE_PRESETS } from "../lib/examples";
import { Tip } from "./ui";

const formatValue = (value: number) => (Number.isInteger(value) ? String(value) : String(+value.toFixed(3)));

export function RichText({ chunks, className }: { chunks: RichChunk[]; className?: string }) {
  return (
    <span className={cn("leading-relaxed", className)}>
      {chunks.map((chunk, i) =>
        chunk.kind === "text" ? (
          <span key={i}>{chunk.text}</span>
        ) : chunk.kind === "ingredient" ? (
          <span key={i} data-kind="ingredient" className="rounded-sm bg-accent/10 px-0.5 font-medium text-name">
            {chunk.text}
          </span>
        ) : (
          <Tip
            key={i}
            label={chunk.amounts
              .map((m) => `${formatValue(m.value)}${m.upper_value !== null ? `–${formatValue(m.upper_value)}` : ""} ${m.unit}`)
              .join(" · ")}
          >
            <span data-kind="measure" className="rounded-sm bg-amount/12 px-0.5 font-medium text-amount tabular-nums">
              {chunk.text}
            </span>
          </Tip>
        ),
      )}
    </span>
  );
}

const label = (factor: number) => (factor === 0.5 ? "½×" : `${formatValue(factor)}×`);

/** Choose how much to make: presets plus a free value. */
export function ScaleControl({
  value,
  onChange,
  disabled,
}: {
  value: number;
  onChange: (value: number) => void;
  disabled?: boolean;
}) {
  const [text, setText] = useState(formatValue(value));
  useEffect(() => setText(formatValue(value)), [value]);
  const commit = (raw: string) => {
    const next = Number(raw);
    if (Number.isFinite(next) && next >= 0.01 && next <= 100) onChange(next);
    else setText(formatValue(value));
  };
  const step = (delta: number) => onChange(Math.max(0.25, Math.min(100, Math.round((value + delta) * 4) / 4)));
  return (
    <div role="group" aria-label="Scale" className="inline-flex items-center gap-2">
      <div className="inline-flex rounded-[8px] border border-line bg-control p-0.5">
        {SCALE_PRESETS.map((preset) => (
          <button
            key={preset}
            type="button"
            disabled={disabled}
            aria-pressed={value === preset}
            onClick={() => onChange(preset)}
            className={cn(
              "h-6 rounded-[6px] px-2.5 text-[12px] font-medium text-muted tabular-nums transition-colors hover:text-fg disabled:opacity-50",
              value === preset && "border border-line bg-evidence text-fg shadow-sm",
            )}
          >
            {label(preset)}
          </button>
        ))}
      </div>
      <div className="inline-flex h-7 items-center rounded-control border border-line bg-evidence">
        <button type="button" aria-label="Scale down" disabled={disabled} onClick={() => step(-0.25)} className="flex h-full w-6 items-center justify-center text-muted hover:text-fg">
          <Minus className="size-3" />
        </button>
        <input
          aria-label="Scale factor"
          inputMode="decimal"
          disabled={disabled}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={(e) => commit(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && commit(e.currentTarget.value)}
          className="w-10 bg-transparent text-center text-[12px] tabular-nums outline-none"
        />
        <button type="button" aria-label="Scale up" disabled={disabled} onClick={() => step(0.25)} className="flex h-full w-6 items-center justify-center text-muted hover:text-fg">
          <Plus className="size-3" />
        </button>
      </div>
    </div>
  );
}
