import { BookOpen, Command as CommandIcon, FlaskConical, Monitor, Moon, Sun, Undo2 } from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { engine, isDesktop, message } from "../api";
import { confirmAction, onMenu, quitApp, setCloseBlocked, syncWindow } from "../api/platform";
import { Kbd, Spinner, Tip } from "../components/ui";
import { cn } from "../lib/cn";
import { OPEN_BOOK, useCommands } from "./commands";
import { Palette, openPalette } from "./Palette";
import { navigate, type Workspace } from "./router";
import { useReports } from "./status";
import { APPEARANCES, useTheme, type Appearance } from "./theme";

const Parser = lazy(() => import("../features/parser/Parser"));
const Cookbooks = lazy(() => import("../features/cookbooks/Cookbooks"));

const NAV: { id: Workspace; label: string; icon: ReactNode; shortcut: string }[] = [
  { id: "parser", label: "Parser", icon: <FlaskConical />, shortcut: "⌘1" },
  { id: "cookbooks", label: "Cookbooks", icon: <BookOpen />, shortcut: "⌘2" },
];

const APPEARANCE_ICON: Record<Appearance, ReactNode> = { system: <Monitor />, light: <Sun />, dark: <Moon /> };

function Brand() {
  const mark = (
    <span className="flex items-center gap-2">
      <span className="flex size-6 items-center justify-center rounded-md bg-accent text-[12px] font-bold text-accent-ink">ip</span>
      <span className="text-[13px] font-semibold tracking-tight">ingredient-parser</span>
    </span>
  );
  if (isDesktop) return mark;
  return (
    <a
      href={import.meta.env.BASE_URL}
      onClick={(event) => {
        event.preventDefault();
        navigate("landing");
      }}
      className="rounded-md hover:opacity-80"
    >
      {mark}
    </a>
  );
}

function AppearanceToggle() {
  const { appearance, setAppearance } = useTheme();
  return (
    <div role="radiogroup" aria-label="Appearance" className="flex rounded-[8px] border border-line bg-control p-0.5">
      {APPEARANCES.map((value) => (
        <Tip key={value} label={`${value[0].toUpperCase()}${value.slice(1)} appearance`}>
          <button
            type="button"
            role="radio"
            aria-checked={appearance === value}
            aria-label={`${value} appearance`}
            onClick={() => setAppearance(value)}
            className={cn(
              "flex h-6 flex-1 items-center justify-center rounded-[6px] text-muted transition-colors hover:text-fg [&_svg]:size-3.5",
              appearance === value && "border border-line bg-evidence text-fg shadow-sm",
            )}
          >
            {APPEARANCE_ICON[value]}
          </button>
        </Tip>
      ))}
    </div>
  );
}

export function Workbench({ workspace }: { workspace: Workspace }) {
  const { setAppearance, dark } = useTheme();
  const reports = useReports();
  const [visited, setVisited] = useState<Set<Workspace>>(() => new Set([workspace]));
  useEffect(() => setVisited((was) => (was.has(workspace) ? was : new Set([...was, workspace]))), [workspace]);

  const busy = Object.values(reports).some((r) => r?.busy);
  const busyRef = useRef(busy);
  busyRef.current = busy;
  const status = reports[workspace];
  const running = Object.entries(reports).find(([, r]) => r?.busy)?.[1];
  const title = `${status?.title ?? NAV.find((n) => n.id === workspace)!.label} — Ingredient Parser`;

  useEffect(() => {
    syncWindow(dark, title).catch((error) => toast.error(message(error)));
  }, [dark, title]);

  useEffect(() => {
    setCloseBlocked(busy).catch((error) => toast.error(message(error)));
    const guard = (event: BeforeUnloadEvent) => {
      if (busy) event.preventDefault();
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [busy]);

  useEffect(() => {
    let asking = false;
    return onMenu(async (id) => {
      if (id === "parser" || id === "cookbooks") navigate(id);
      if (id === "open") {
        navigate("cookbooks");
        setTimeout(() => window.dispatchEvent(new Event(OPEN_BOOK)));
      }
      if (id === "quit" && !asking) {
        asking = true;
        try {
          if (
            !busyRef.current ||
            (await confirmAction("An extraction is still running. Quit the application?", { okLabel: "Quit" }))
          ) {
            await setCloseBlocked(false);
            await quitApp();
          }
        } catch (error) {
          toast.error(message(error));
        } finally {
          asking = false;
        }
      }
    });
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
      const index = ["1", "2"].indexOf(event.key);
      if (index >= 0) {
        event.preventDefault();
        navigate(NAV[index].id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useCommands(
    "shell",
    [
      ...NAV.map((n) => ({
        id: `go-${n.id}`,
        title: `Go to ${n.label}`,
        group: "Navigate",
        icon: n.icon,
        shortcut: n.shortcut,
        run: () => navigate(n.id),
      })),
      ...(isDesktop
        ? []
        : [{ id: "go-home", title: "Go to the home page", group: "Navigate", icon: <Undo2 />, run: () => navigate("landing") }]),
      ...APPEARANCES.map((value) => ({
        id: `appearance-${value}`,
        title: `Use ${value} appearance`,
        group: "Appearance",
        icon: APPEARANCE_ICON[value],
        keywords: ["theme", "dark", "light"],
        run: () => setAppearance(value),
      })),
    ],
    [],
  );

  return (
    <div className="flex h-dvh bg-base text-fg">
      <aside className="flex w-[212px] shrink-0 flex-col border-r border-line bg-mantle px-3 pt-3 pb-3 max-md:w-14 max-md:px-2">
        <div className="flex h-9 items-center px-1.5 max-md:hidden">
          <Brand />
        </div>
        <button
          type="button"
          onClick={openPalette}
          className="mt-3 flex h-8 items-center gap-2 rounded-control border border-line bg-evidence px-2.5 text-[12.5px] text-faint transition-colors hover:border-line-strong hover:text-muted max-md:justify-center max-md:px-0"
        >
          <CommandIcon className="size-3.5" />
          <span className="flex-1 text-left max-md:hidden">Commands</span>
          <span className="max-md:hidden">
            <Kbd>⌘K</Kbd>
          </span>
        </button>
        <nav aria-label="Workspaces" className="mt-4 flex flex-col gap-0.5">
          {NAV.map((item) => (
            <button
              key={item.id}
              type="button"
              aria-current={workspace === item.id ? "page" : undefined}
              onClick={() => navigate(item.id)}
              className={cn(
                "group flex h-8 items-center gap-2.5 rounded-control px-2.5 text-[13px] font-medium transition-colors max-md:justify-center max-md:px-0 [&_svg]:size-4",
                workspace === item.id ? "bg-evidence text-fg shadow-sm ring-1 ring-line" : "text-muted hover:bg-hover hover:text-fg",
              )}
            >
              <span className={workspace === item.id ? "text-accent" : ""}>{item.icon}</span>
              <span className="flex-1 text-left max-md:hidden">{item.label}</span>
              {reports[item.id]?.busy ? (
                <Spinner className="size-3.5 max-md:hidden" />
              ) : (
                <span className="text-[11px] text-faint opacity-0 transition-opacity group-hover:opacity-100 max-md:hidden">
                  {item.shortcut}
                </span>
              )}
            </button>
          ))}
        </nav>
        <div className="mt-auto max-md:hidden">
          <AppearanceToggle />
        </div>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <main className="relative flex min-h-0 flex-1 flex-col">
          {NAV.map(({ id }) =>
            visited.has(id) ? (
              <div key={id} className="flex min-h-0 flex-1 flex-col" hidden={workspace !== id}>
                <Suspense
                  fallback={
                    <div className="flex flex-1 items-center justify-center">
                      <Spinner />
                    </div>
                  }
                >
                  {id === "parser" ? <Parser /> : <Cookbooks active={workspace === "cookbooks"} />}
                </Suspense>
              </div>
            ) : null,
          )}
        </main>
        <footer
          aria-label="Workspace status"
          className="flex h-7 shrink-0 items-center justify-between gap-4 border-t border-line bg-mantle px-4 text-[11.5px] text-muted"
        >
          <span role="status" className="flex min-w-0 items-center gap-2 truncate">
            {running && <Spinner className="size-3" />}
            {running?.message ?? status?.message ?? "Ready"}
          </span>
          <span className="flex shrink-0 items-center gap-3 tabular-nums">
            {(running ?? status)?.detail && <span>{(running ?? status)?.detail}</span>}
            <span className="flex items-center gap-1.5">
              <span className="size-1.5 rounded-full bg-ok" />
              {engine}
            </span>
          </span>
        </footer>
      </div>
      <Palette />
    </div>
  );
}
