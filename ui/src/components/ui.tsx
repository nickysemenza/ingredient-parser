// The app's primitives. Base UI supplies behavior and accessibility; the
// classes here are the whole visual system (see styles.css for tokens).
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { ChevronDown, LoaderCircle } from "lucide-react";
import {
  forwardRef,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from "react";
import { cn } from "../lib/cn";

const focus = "focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent";

const buttonBase = cn(
  "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-control font-medium whitespace-nowrap select-none",
  "transition-colors disabled:pointer-events-none disabled:opacity-45",
  focus,
);
const variants = {
  primary: "bg-accent text-accent-ink hover:brightness-110 active:brightness-95 shadow-sm",
  secondary: "border border-line bg-control text-fg hover:bg-hover active:bg-line",
  ghost: "text-muted hover:bg-hover hover:text-fg active:bg-line",
  danger: "border border-line bg-control text-bad hover:bg-bad/10",
};
const sizes = {
  sm: "h-7 px-2.5 text-[12px] [&_svg]:size-3.5",
  md: "h-8 px-3 text-[13px] [&_svg]:size-4",
  lg: "h-10 px-4 text-sm [&_svg]:size-4",
  icon: "size-7 [&_svg]:size-4",
};

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: keyof typeof variants;
  size?: keyof typeof sizes;
  busy?: boolean;
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", busy, className, children, disabled, type = "button", ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      disabled={disabled || busy}
      className={cn(buttonBase, variants[variant], sizes[size], className)}
      {...props}
    >
      {busy && <LoaderCircle className="animate-spin" aria-hidden />}
      {children}
    </button>
  );
});

/** An icon button whose label shows as a tooltip and names it for assistive tech. */
export function IconButton({
  label,
  children,
  shortcut,
  ...props
}: Omit<ButtonProps, "size" | "aria-label"> & { label: string; shortcut?: string }) {
  return (
    <Tip label={label} shortcut={shortcut}>
      <Button variant="ghost" size="icon" aria-label={label} {...props}>
        {children}
      </Button>
    </Tip>
  );
}

export function Tip({ label, shortcut, children }: { label: ReactNode; shortcut?: string; children: ReactNode }) {
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger render={children as React.ReactElement} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner sideOffset={6} className="z-50">
          <BaseTooltip.Popup className="flex items-center gap-2 rounded-md border border-line bg-evidence px-2 py-1 text-[12px] text-fg shadow-lg transition-opacity duration-100 data-ending-style:opacity-0 data-starting-style:opacity-0">
            {label}
            {shortcut && <Kbd>{shortcut}</Kbd>}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

export const TipProvider = BaseTooltip.Provider;

const field = cn(
  "w-full rounded-control border border-line bg-evidence text-fg placeholder:text-faint",
  "transition-colors hover:border-line-strong focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/20",
  "disabled:opacity-50",
);

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input(
  { className, ...props },
  ref,
) {
  return <input ref={ref} className={cn(field, "h-8 px-2.5", className)} {...props} />;
});

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(
  function Textarea({ className, ...props }, ref) {
    return <textarea ref={ref} className={cn(field, "resize-none px-3 py-2.5 leading-relaxed", className)} {...props} />;
  },
);

export function Select({ className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <span className="relative inline-flex">
      <select className={cn(field, "h-8 appearance-none py-0 pr-7 pl-2.5", className)} {...props}>
        {children}
      </select>
      <ChevronDown className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 text-muted" />
    </span>
  );
}

const tones = {
  neutral: "bg-control text-muted border-line",
  accent: "bg-accent/10 text-accent border-accent/25",
  ok: "bg-ok/10 text-ok border-ok/25",
  warn: "bg-warn/10 text-warn border-warn/30",
  bad: "bg-bad/10 text-bad border-bad/25",
  amount: "bg-amount/10 text-amount border-amount/25",
  violet: "bg-violet/10 text-violet border-violet/25",
};
export type Tone = keyof typeof tones;

export function Badge({
  tone = "neutral",
  className,
  children,
  title,
}: {
  tone?: Tone;
  className?: string;
  children: ReactNode;
  title?: string;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex h-5 shrink-0 items-center gap-1 rounded-full border px-2 text-[11px] font-medium whitespace-nowrap [&_svg]:size-3",
        tones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-[18px] min-w-[18px] items-center justify-center rounded border border-line bg-control px-1 font-sans text-[10.5px] font-medium text-muted">
      {children}
    </kbd>
  );
}

export function Spinner({ className }: { className?: string }) {
  return <LoaderCircle aria-hidden className={cn("size-4 animate-spin text-muted", className)} />;
}

/** Underlined tabs. `items` are the values; `labels` optionally rename them. */
export function Tabs<T extends string>({
  value,
  items,
  onChange,
  label,
  render,
  className,
}: {
  value: T;
  items: readonly T[];
  onChange: (value: T) => void;
  label: string;
  render?: (item: T) => ReactNode;
  className?: string;
}) {
  return (
    <BaseTabs.Root value={value} onValueChange={(next) => onChange(next as T)} className={className}>
      <BaseTabs.List aria-label={label} className="relative flex gap-4 border-b border-line">
        {items.map((item) => (
          <BaseTabs.Tab
            key={item}
            value={item}
            className={cn(
              "flex h-8 items-center gap-1.5 text-[12.5px] font-medium text-muted transition-colors select-none hover:text-fg data-active:text-fg [&_svg]:size-3.5",
              focus,
            )}
          >
            {render ? render(item) : item}
          </BaseTabs.Tab>
        ))}
        <BaseTabs.Indicator className="absolute -bottom-px left-0 h-0.5 w-(--active-tab-width) translate-x-(--active-tab-left) rounded-full bg-accent transition-[translate,width] duration-200 ease-out" />
      </BaseTabs.List>
    </BaseTabs.Root>
  );
}

/** A pill segmented control: one choice of a few. */
export function Segmented<T extends string>({
  value,
  items,
  onChange,
  label,
  render,
  size = "md",
}: {
  value: T;
  items: readonly T[];
  onChange: (value: T) => void;
  label: string;
  render?: (item: T) => ReactNode;
  size?: "sm" | "md";
}) {
  return (
    <BaseTabs.Root value={value} onValueChange={(next) => onChange(next as T)}>
      <BaseTabs.List
        aria-label={label}
        className="relative inline-flex rounded-[8px] border border-line bg-control p-0.5"
      >
        {items.map((item) => (
          <BaseTabs.Tab
            key={item}
            value={item}
            className={cn(
              "relative z-1 flex items-center gap-1.5 rounded-[6px] px-3 font-medium text-muted transition-colors select-none hover:text-fg data-active:text-fg [&_svg]:size-3.5",
              size === "sm" ? "h-6 text-[12px]" : "h-7 text-[12.5px]",
              focus,
            )}
          >
            {render ? render(item) : item}
          </BaseTabs.Tab>
        ))}
        <BaseTabs.Indicator className="absolute top-0.5 left-0 h-[calc(100%-4px)] w-(--active-tab-width) translate-x-(--active-tab-left) rounded-[6px] border border-line bg-evidence shadow-sm transition-[translate,width] duration-200 ease-out" />
      </BaseTabs.List>
    </BaseTabs.Root>
  );
}

export type MenuEntry =
  | { label: string; icon?: ReactNode; onSelect: () => void; disabled?: boolean; danger?: boolean }
  | "separator";

export function ActionMenu({
  label,
  items,
  trigger,
  align = "end",
}: {
  label: string;
  items: MenuEntry[];
  trigger?: ReactNode;
  align?: "start" | "end";
}) {
  return (
    <BaseMenu.Root>
      <BaseMenu.Trigger
        aria-label={label}
        className={cn(buttonBase, variants.secondary, trigger ? sizes.icon : sizes.sm, "data-popup-open:bg-hover")}
      >
        {trigger ?? (
          <>
            {label}
            <ChevronDown />
          </>
        )}
      </BaseMenu.Trigger>
      <BaseMenu.Portal>
        <BaseMenu.Positioner sideOffset={6} align={align} className="z-50 outline-none">
          <BaseMenu.Popup className="min-w-48 origin-(--transform-origin) rounded-[8px] border border-line bg-evidence p-1 shadow-xl outline-none transition-[scale,opacity] duration-100 data-ending-style:scale-95 data-ending-style:opacity-0 data-starting-style:scale-95 data-starting-style:opacity-0">
            {items.map((item, index) =>
              item === "separator" ? (
                <BaseMenu.Separator key={index} className="mx-1 my-1 h-px bg-line" />
              ) : (
                <BaseMenu.Item
                  key={item.label}
                  disabled={item.disabled}
                  onClick={item.onSelect}
                  className={cn(
                    "flex h-7 items-center gap-2 rounded-[5px] px-2 text-[12.5px] outline-none select-none data-disabled:opacity-45 data-highlighted:bg-accent data-highlighted:text-accent-ink [&_svg]:size-3.5",
                    item.danger ? "text-bad" : "text-fg",
                  )}
                >
                  {item.icon}
                  {item.label}
                </BaseMenu.Item>
              ),
            )}
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}

export function EmptyState({
  icon,
  title,
  children,
  actions,
  className,
}: {
  icon?: ReactNode;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-1 flex-col items-center justify-center p-10 text-center animate-in", className)}>
      {icon && (
        <div className="mb-4 flex size-12 items-center justify-center rounded-2xl border border-line bg-mantle text-muted [&_svg]:size-5">
          {icon}
        </div>
      )}
      <h2 className="text-[15px] font-semibold text-fg">{title}</h2>
      {children && <div className="mt-1.5 max-w-sm text-[13px] text-muted">{children}</div>}
      {actions && <div className="mt-5 flex flex-wrap justify-center gap-2">{actions}</div>}
    </div>
  );
}

/** A labelled fact list; `null` entries are dropped so callers can be terse. */
export function Facts({ items, className }: { items: ([string, ReactNode] | null)[]; className?: string }) {
  return (
    <dl className={cn("grid grid-cols-[minmax(7rem,auto)_1fr] gap-x-4 gap-y-1.5 text-[12.5px]", className)}>
      {items
        .filter((item): item is [string, ReactNode] => item !== null)
        .map(([term, value]) => (
          <div key={term} className="contents">
            <dt className="text-muted">{term}</dt>
            <dd className="min-w-0 break-words text-fg">{value ?? "—"}</dd>
          </div>
        ))}
    </dl>
  );
}

/** A titled region inside a pane. */
export function Section({
  title,
  aside,
  children,
  className,
  label,
}: {
  title: ReactNode;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
  label?: string;
}) {
  return (
    <section aria-label={label ?? (typeof title === "string" ? title : undefined)} className={cn("rounded-panel border border-line bg-evidence", className)}>
      <header className="flex min-h-10 items-center justify-between gap-3 border-b border-line px-4 py-2">
        <h3 className="text-[12.5px] font-semibold text-fg">{title}</h3>
        {aside && <div className="flex items-center gap-2">{aside}</div>}
      </header>
      <div className="p-4">{children}</div>
    </section>
  );
}

export function JsonView({ value, className }: { value: unknown; className?: string }) {
  return (
    <pre className={cn("overflow-auto rounded-control bg-mantle p-3 font-mono text-[12px] leading-relaxed text-fg", className)}>
      {JSON.stringify(value, null, 2)}
    </pre>
  );
}
