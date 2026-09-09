import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { ChevronDown, X } from "lucide-react";

export function useStored<T>(key: string, initial: T, allowed?: readonly T[]) {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(key);
      if (!raw) return initial;
      const parsed: unknown = JSON.parse(raw);
      return typeof parsed === typeof initial &&
        (typeof parsed !== "number" || Number.isFinite(parsed)) &&
        (!allowed || allowed.includes(parsed as T))
        ? (parsed as T)
        : initial;
    } catch {
      return initial;
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* Preferences are optional when storage is unavailable. */
    }
  }, [key, value]);
  return [value, setValue] as const;
}
export function Tabs<T extends string>({
  value,
  items,
  onChange,
  label,
}: {
  value: T;
  items: readonly T[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="tabs" role="tablist" aria-label={label}>
      {items.map((item, i) => (
        <button
          key={item}
          role="tab"
          aria-selected={value === item}
          tabIndex={value === item ? 0 : -1}
          onClick={() => onChange(item)}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
              e.preventDefault();
              const next =
                items[
                  (i + (e.key === "ArrowRight" ? 1 : items.length - 1)) %
                    items.length
                ];
              onChange(next);
              (
                e.currentTarget.parentElement?.children[
                  items.indexOf(next)
                ] as HTMLElement
              )?.focus();
            }
          }}
        >
          {item}
        </button>
      ))}
    </div>
  );
}
export function Menu({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const close = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node))
        ref.current.open = false;
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  return (
    <details
      className="menu"
      ref={ref}
      onKeyDown={(e) => {
        if (e.key === "Escape" && ref.current) {
          ref.current.open = false;
          ref.current.querySelector("summary")?.focus();
        }
      }}
    >
      <summary>
        {label}
        <ChevronDown size={13} />
      </summary>
      <div
        className="menu-content"
        onClick={(e) => {
          if (
            (e.target as HTMLElement).closest("button") &&
            !(e.target as HTMLElement).closest("[data-keep-open]") &&
            ref.current
          )
            ref.current.open = false;
        }}
      >
        {children}
      </div>
    </details>
  );
}
export function Split({
  id,
  left,
  right,
  initial = 45,
  min = 25,
  max = 70,
  className = "",
}: {
  id: string;
  left: ReactNode;
  right: ReactNode;
  initial?: number;
  min?: number;
  max?: number;
  className?: string;
}) {
  const [fraction, setFraction] = useStored(`v1:split:${id}`, initial);
  const ref = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const bound = (n: number) => Math.max(min, Math.min(max, n));
  return (
    <div
      className={`split ${className}`}
      ref={ref}
      style={{ "--split": `${bound(fraction)}%` } as CSSProperties}
    >
      <div className="split-first">{left}</div>
      <div
        className="split-handle"
        role="separator"
        aria-label="Resize panes"
        aria-orientation="vertical"
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={Math.round(bound(fraction))}
        tabIndex={0}
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
              e.key === "Home"
                ? min
                : e.key === "End"
                  ? max
                  : bound(fraction + (e.key === "ArrowLeft" ? -2 : 2)),
            );
          }
        }}
      />
      <div className="split-second">{right}</div>
    </div>
  );
}
export function VirtualList<T>({
  rows,
  height = 34,
  selected,
  onSelect,
  render,
  label,
}: {
  rows: T[];
  height?: number;
  selected: number;
  onSelect: (n: number) => void;
  render: (row: T, i: number) => ReactNode;
  label: string;
}) {
  const [scroll, setScroll] = useState(0);
  const [viewport, setViewport] = useState(400);
  const ref = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const observer = new ResizeObserver(() => setViewport(node.clientHeight));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const node = ref.current;
    if (!node || selected < 0) return;
    const top = selected * height;
    if (top < node.scrollTop) node.scrollTop = top;
    else if (top + height > node.scrollTop + node.clientHeight)
      node.scrollTop = top + height - node.clientHeight;
  }, [selected, height]);
  const start = Math.max(0, Math.floor(scroll / height) - 5),
    end = Math.min(rows.length, Math.ceil((scroll + viewport) / height) + 5);
  return (
    <div
      className="virtual-list"
      ref={ref}
      role="listbox"
      aria-label={label}
      tabIndex={0}
      aria-activedescendant={
        selected >= start && selected < end ? `${id}-${selected}` : undefined
      }
      onScroll={(e) => setScroll(e.currentTarget.scrollTop)}
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
      <div style={{ height: rows.length * height, position: "relative" }}>
        {rows.slice(start, end).map((row, offset) => {
          const i = start + offset;
          return (
            <div
              id={`${id}-${i}`}
              role="option"
              aria-selected={selected === i}
              key={i}
              className={`virtual-row ${selected === i ? "selected" : ""}`}
              style={{
                position: "absolute",
                top: i * height,
                height,
                left: 0,
                right: 0,
              }}
              onClick={() => onSelect(i)}
            >
              {render(row, i)}
            </div>
          );
        })}
      </div>
      {!rows.length && <p className="empty-inline">No matching rows.</p>}
    </div>
  );
}
export function JsonView({ value }: { value: unknown }) {
  return <pre className="json">{JSON.stringify(value, null, 2)}</pre>;
}
export function Fields({ value }: { value: unknown }) {
  if (!value || typeof value !== "object")
    return <span>{String(value ?? "—")}</span>;
  return (
    <dl className="fields">
      {Object.entries(value).map(([key, v]) => (
        <div key={key}>
          <dt>{key.replaceAll("_", " ")}</dt>
          <dd>
            {v === null ? (
              "—"
            ) : typeof v === "object" ? (
              <JsonView value={v} />
            ) : (
              String(v)
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}
export function Notice({ error, clear }: { error: string; clear: () => void }) {
  return (
    <div role="alert" className="notice">
      <span>{error}</span>
      <button
        aria-label="Dismiss error"
        className="icon-button"
        onClick={clear}
      >
        <X size={16} />
      </button>
    </div>
  );
}

export function Modal({
  children,
  onClose,
  label,
}: {
  children: ReactNode;
  onClose: () => void;
  label: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const node = ref.current;
    node?.showModal();
    return () => node?.close();
  }, []);
  return (
    <dialog
      className="modal"
      ref={ref}
      aria-labelledby={label}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          const r = e.currentTarget.getBoundingClientRect();
          if (
            e.clientX < r.left ||
            e.clientX > r.right ||
            e.clientY < r.top ||
            e.clientY > r.bottom
          )
            onClose();
        }
      }}
    >
      {children}
    </dialog>
  );
}
