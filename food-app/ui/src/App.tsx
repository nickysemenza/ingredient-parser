import { useCallback, useEffect, useRef, useState } from "react";
import { BookOpen, FlaskConical, Moon, Sun } from "lucide-react";
import { call, discardChanges, isNative } from "./bridge";
import { Notice, useStored } from "./components";
import { Parser } from "./Parser";
import { Cookbooks } from "./Cookbooks";
import type { ReviewAction, WorkspaceStatus } from "./shell";
export function App() {
  const [workspace, setWorkspace] = useStored<"Parser" | "Cookbooks">(
    "v1:workspace",
    "Parser",
    ["Parser", "Cookbooks"],
  );
  const [theme, setTheme] = useStored<"dark" | "light">("v1:theme", "dark", [
    "dark",
    "light",
  ]);
  const [bookStatus, setBookStatus] = useState<WorkspaceStatus>({
    title: "Cookbooks",
    canReview: false,
    message: "No cookbook open",
    detail: "",
  });
  const [reviewAction, setReviewAction] = useState<ReviewAction>({
    id: 0,
    command: "",
  });
  const [error, setError] = useState("");
  const [parserBusy, setParserBusy] = useState(false);
  const [bookBusy, setBookBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [openSignal, setOpenSignal] = useState(0);
  const [saveSignal, setSaveSignal] = useState(0);
  const [startupPath, setStartupPath] = useState<string | null>(null);
  const blocked = dirty || parserBusy || bookBusy;
  const blockedRef = useRef(blocked);
  blockedRef.current = blocked;
  const asking = useRef(false);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    if (isNative())
      void call("set_shell_appearance", {
        dark: theme === "dark",
        reviewEnabled: workspace === "Cookbooks" && bookStatus.canReview,
        title: `${workspace === "Cookbooks" ? bookStatus.title : "Parser"} — Ingredient Parser`,
      }).catch((e) => setError(String(e)));
  }, [theme, workspace, bookStatus.title, bookStatus.canReview]);
  useEffect(() => {
    if (isNative())
      void call("set_close_blocked", { blocked }).catch((e) =>
        setError(String(e)),
      );
    const guard = (e: BeforeUnloadEvent) => {
      if (blocked) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [blocked]);
  useEffect(() => {
    if (!isNative()) return;
    let dispose: (() => void) | undefined;
    void call<string | null>("startup_run")
      .then((path) => {
        if (path) {
          setStartupPath(path);
          setWorkspace("Cookbooks");
        }
      })
      .catch((e) => setError(String(e)));
    import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<string>("app-menu", async (e) => {
          if (e.payload.startsWith("review-"))
            setReviewAction((previous) => ({
              id: previous.id + 1,
              command: e.payload,
            }));
          if (e.payload === "parser") setWorkspace("Parser");
          if (e.payload === "cookbooks") setWorkspace("Cookbooks");
          if (e.payload === "open") {
            setWorkspace("Cookbooks");
            setOpenSignal((n) => n + 1);
          }
          if (e.payload === "save") {
            setWorkspace("Cookbooks");
            setSaveSignal((n) => n + 1);
          }
          if (e.payload === "quit" && !asking.current) {
            asking.current = true;
            try {
              if (
                !blockedRef.current ||
                (await discardChanges(
                  "Unsaved changes or an active operation remain. Quit the application?",
                ))
              ) {
                await call("set_close_blocked", { blocked: false });
                await call("quit_app");
              }
            } catch (err) {
              setError(String(err));
            } finally {
              asking.current = false;
            }
          }
        }),
      )
      .then((fn) => {
        dispose = fn;
      })
      .catch((e) => setError(String(e)));
    return () => dispose?.();
  }, []);
  const onError = useCallback((value: string) => setError(value), []);
  return (
    <div className="app">
      <aside className="workspace-rail">
        <div className="app-name">ingredient-parser</div>
        <nav aria-label="Workspaces">
          {(["Parser", "Cookbooks"] as const).map((name) => (
            <button
              key={name}
              aria-current={workspace === name ? "page" : undefined}
              onClick={() => setWorkspace(name)}
            >
              {name === "Parser" ? (
                <FlaskConical size={17} />
              ) : (
                <BookOpen size={17} />
              )}
              <span>{name}</span>
            </button>
          ))}
        </nav>
        <button
          className="appearance"
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
        >
          {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
          <span>{theme === "dark" ? "Light" : "Dark"} appearance</span>
        </button>
      </aside>
      <main>
        {error && <Notice error={error} clear={() => setError("")} />}
        <div className="workspace-host" hidden={workspace !== "Parser"}>
          <Parser onError={onError} onBusy={setParserBusy} />
        </div>
        <div className="workspace-host" hidden={workspace !== "Cookbooks"}>
          <Cookbooks
            onError={onError}
            onBusy={setBookBusy}
            onDirty={setDirty}
            openSignal={openSignal}
            saveSignal={saveSignal}
            startupPath={startupPath}
            active={workspace === "Cookbooks"}
            reviewAction={reviewAction}
            onStatus={setBookStatus}
          />
        </div>
        <footer className="status-bar" aria-label="Workspace status">
          <span role="status">
            {bookBusy
              ? bookStatus.message
              : parserBusy
                ? "Parsing…"
                : workspace === "Cookbooks"
                  ? bookStatus.message
                  : "Ready"}
          </span>
          <span className="status-detail">
            {bookBusy || workspace === "Cookbooks"
              ? bookStatus.detail
              : "Native Rust parser"}
          </span>
        </footer>
      </main>
    </div>
  );
}
