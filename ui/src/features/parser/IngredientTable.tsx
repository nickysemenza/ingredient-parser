import { ArrowDown, ArrowUp } from "lucide-react";
import { useMemo, useState } from "react";
import type { IngredientRow } from "../../api";
import { Amounts, ConfidenceBadge, SourceText } from "../../components/ingredient";
import { VirtualList } from "../../components/layout";
import { cn } from "../../lib/cn";

const COLUMNS = [
  { key: "line", label: "#", sort: (r: IngredientRow) => r.lineNumber },
  { key: "input", label: "Input", sort: (r: IngredientRow) => r.input },
  { key: "name", label: "Name", sort: (r: IngredientRow) => r.ingredient.name },
  { key: "amounts", label: "Amounts", sort: (r: IngredientRow) => r.amounts.join(" ") },
  { key: "modifier", label: "Modifier", sort: (r: IngredientRow) => r.ingredient.modifier ?? "" },
  { key: "confidence", label: "Confidence", sort: (r: IngredientRow) => ["low", "medium", "high"].indexOf(r.confidence) },
] as const;
type Key = (typeof COLUMNS)[number]["key"];

const GRID = "grid grid-cols-[2.25rem_minmax(10rem,2.2fr)_minmax(6rem,1.2fr)_minmax(6rem,1fr)_minmax(5rem,1fr)_6.5rem] gap-3";

/** Parsed lines, sortable, one selected. `selected` is a line number. */
export function IngredientTable({
  rows,
  selected,
  onSelect,
  label = "Parsed ingredients",
  stale,
}: {
  rows: IngredientRow[];
  selected: number | null;
  onSelect: (lineNumber: number) => void;
  label?: string;
  stale?: boolean;
}) {
  const [sort, setSort] = useState<{ key: Key; ascending: boolean } | null>(null);
  const sorted = useMemo(() => {
    if (!sort) return rows;
    const by = COLUMNS.find((c) => c.key === sort.key)!.sort;
    return [...rows].sort((a, b) => {
      const x = by(a);
      const y = by(b);
      const order = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y));
      return sort.ascending ? order : -order;
    });
  }, [rows, sort]);
  return (
    <div className={cn("flex min-h-0 flex-1 flex-col transition-opacity", stale && "opacity-60")}>
      <div role="row" className={cn(GRID, "h-8 shrink-0 items-center border-b border-line bg-mantle/60 px-3 text-[11px] font-medium text-muted")}>
        {COLUMNS.map((column) => (
          <button
            key={column.key}
            type="button"
            aria-label={`Sort by ${column.label === "#" ? "line" : column.label.toLowerCase()}`}
            onClick={() =>
              setSort((was) =>
                was?.key === column.key ? (was.ascending ? { key: column.key, ascending: false } : null) : { key: column.key, ascending: true },
              )
            }
            className="flex items-center gap-1 text-left hover:text-fg"
          >
            {column.label}
            {sort?.key === column.key && (sort.ascending ? <ArrowUp className="size-3" /> : <ArrowDown className="size-3" />)}
          </button>
        ))}
      </div>
      <VirtualList
        rows={sorted}
        label={label}
        rowHeight={38}
        selected={sorted.findIndex((row) => row.lineNumber === selected)}
        onSelect={(index) => onSelect(sorted[index].lineNumber)}
        rowClassName="px-0"
        empty="Nothing to parse yet."
        render={(row) => (
          <div className={cn(GRID, "w-full items-center px-3")}>
            <span className="text-right font-mono text-[11px] text-faint tabular-nums">{row.lineNumber}</span>
            <span className="truncate text-[12.5px]" title={row.input}>
              <SourceText segments={row.segments} wrap={false} />
            </span>
            <span className="truncate font-medium text-name" title={row.ingredient.name}>
              {row.ingredient.name || <span className="text-faint">—</span>}
            </span>
            <Amounts amounts={row.amounts} className="flex-nowrap overflow-hidden text-[12px]" />
            <span className={cn("truncate", row.ingredient.modifier ? "text-violet" : "text-faint")} title={row.ingredient.modifier ?? ""}>
              {row.ingredient.modifier ?? "—"}
            </span>
            <ConfidenceBadge row={row} />
          </div>
        )}
      />
    </div>
  );
}
