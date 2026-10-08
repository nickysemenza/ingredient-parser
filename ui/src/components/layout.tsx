import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useRef, type CSSProperties, type ReactNode } from "react";
import { cn } from "../lib/cn";
import { useStored } from "../lib/stored";

/** Two resizable panes; the split is remembered per `id`. */
export function Split({
  id,
  left,
  right,
  initial = 50,
  min = 25,
  max = 75,
  className,
}: {
  id: string;
  left: ReactNode;
  right: ReactNode;
  initial?: number;
  min?: number;
  max?: number;
  className?: string;
}) {
  const [fraction, setFraction] = useStored(`v2:split:${id}`, initial);
  const ref = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const bound = (n: number) => Math.max(min, Math.min(max, n));
  if (right === null) return <div className={cn("flex min-h-0 flex-1", className)}>{left}</div>;
  return (
    <div
      ref={ref}
      className={cn("grid min-h-0 flex-1", className)}
      style={{ gridTemplateColumns: `minmax(0, ${bound(fraction)}fr) 1px minmax(0, ${100 - bound(fraction)}fr)` } as CSSProperties}
    >
      <div className="flex min-h-0 min-w-0 flex-col">{left}</div>
      <div
        role="separator"
        aria-label="Resize panes"
        aria-orientation="vertical"
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={Math.round(bound(fraction))}
        tabIndex={0}
        className="group relative z-10 cursor-col-resize bg-line outline-none focus-visible:bg-accent"
        onPointerDown={(e) => {
          dragging.current = true;
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (dragging.current && ref.current) {
            const r = ref.current.getBoundingClientRect();
            setFraction(bound(((e.clientX - r.left) / r.width) * 100));
          }
        }}
        onPointerUp={() => {
          dragging.current = false;
        }}
        onKeyDown={(e) => {
          if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
            e.preventDefault();
            setFraction(
              e.key === "Home" ? min : e.key === "End" ? max : bound(fraction + (e.key === "ArrowLeft" ? -2 : 2)),
            );
          }
        }}
      >
        <span className="absolute inset-y-0 -right-1.5 -left-1.5 group-hover:bg-accent/20" />
      </div>
      <div className="flex min-h-0 min-w-0 flex-col">{right}</div>
    </div>
  );
}

/** A keyboard-navigable listbox that renders only the visible rows. */
export function VirtualList<T>({
  rows,
  rowHeight = 36,
  selected,
  onSelect,
  render,
  label,
  empty = "No matching rows.",
  rowClassName,
}: {
  rows: readonly T[];
  rowHeight?: number;
  selected: number;
  onSelect: (index: number) => void;
  render: (row: T, index: number) => ReactNode;
  label: string;
  empty?: ReactNode;
  rowClassName?: string;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const id = useId();
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parent.current,
    estimateSize: () => rowHeight,
    overscan: 8,
    // jsdom has no layout; render a first screenful there.
    initialRect: { width: 800, height: 600 },
  });
  useEffect(() => {
    if (selected >= 0 && selected < rows.length) virtualizer.scrollToIndex(selected, { align: "auto" });
  }, [selected, rows.length, virtualizer]);
  const items = virtualizer.getVirtualItems();
  const visible = items.some((item) => item.index === selected);
  return (
    <div
      ref={parent}
      role="listbox"
      aria-label={label}
      tabIndex={0}
      aria-activedescendant={visible ? `${id}-${selected}` : undefined}
      className="min-h-0 flex-1 overflow-auto outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-inset"
      onKeyDown={(e) => {
        const n =
          e.key === "ArrowDown"
            ? Math.min(rows.length - 1, selected + 1)
            : e.key === "ArrowUp"
              ? Math.max(0, selected - 1)
              : e.key === "Home"
                ? 0
                : e.key === "End"
                  ? rows.length - 1
                  : null;
        if (n !== null && rows.length) {
          e.preventDefault();
          onSelect(n);
        }
      }}
    >
      {rows.length ? (
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {items.map((item) => {
            const index = item.index;
            const isSelected = index === selected;
            return (
              // Keyboard selection is handled by the listbox, which owns focus.
              // oxlint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus
              <div
                id={`${id}-${index}`}
                role="option"
                aria-selected={isSelected}
                key={item.key}
                data-index={index}
                className={cn(
                  "absolute inset-x-0 flex items-center border-b border-line/60 px-3 text-[12.5px]",
                  isSelected ? "bg-selection shadow-[inset_2px_0_0_var(--accent)]" : "hover:bg-hover/50",
                  rowClassName,
                )}
                style={{ top: item.start, height: rowHeight }}
                onClick={() => onSelect(index)}
              >
                {render(rows[index], index)}
              </div>
            );
          })}
        </div>
      ) : (
        <p className="p-6 text-center text-[12.5px] text-muted">{empty}</p>
      )}
    </div>
  );
}

/** The heading row of a pane. */
export function PaneHeader({ title, children, className }: { title: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <header className={cn("flex h-11 shrink-0 items-center justify-between gap-3 border-b border-line px-4", className)}>
      <h2 className="min-w-0 truncate text-[12.5px] font-semibold text-fg">{title}</h2>
      {children && <div className="flex shrink-0 items-center gap-1.5">{children}</div>}
    </header>
  );
}
