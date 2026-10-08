// Shell integration: dialogs, clipboard, links, window chrome. Each call has a
// desktop implementation and a browser one.
import { isTauri, native } from "./transport";

/** Choose an EPUB or a folder on the desktop. `null` when cancelled. */
export async function pickPath(kind: "epub" | "directory"): Promise<string | null> {
  if (window.__FIXTURE_INVOKE__) return native<string | null>("dialog_open", { kind });
  const { open } = await import("@tauri-apps/plugin-dialog");
  return (await open({
    directory: kind === "directory",
    multiple: false,
    filters: kind === "directory" ? undefined : [{ name: "Cookbook EPUB", extensions: ["epub"] }],
  })) as string | null;
}

export async function confirmAction(
  message: string,
  { title = "Are you sure?", okLabel = "Continue", cancelLabel = "Cancel" } = {},
): Promise<boolean> {
  if (!isTauri()) return window.confirm(message);
  const { confirm } = await import("@tauri-apps/plugin-dialog");
  return confirm(message, { title, kind: "warning", okLabel, cancelLabel });
}

export async function copyText(text: string) {
  if (isTauri()) {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
  } else await navigator.clipboard.writeText(text);
}

/** Open a recipe page in the user's browser. */
export async function openUrl(url: string) {
  if (isTauri() || window.__FIXTURE_INVOKE__) await native<void>("open_source_url", { url });
  else window.open(url, "_blank", "noopener,noreferrer");
}

export const revealFile = (path: string) => native<void>("reveal_file", { path });

/** Match the native window to the app's appearance and title. */
export async function syncWindow(dark: boolean, title: string) {
  document.title = title;
  if (isTauri()) await native<void>("set_shell_appearance", { dark, title });
}

/** Ask before closing the window while a job runs. */
export async function setCloseBlocked(blocked: boolean) {
  if (isTauri()) await native<void>("set_close_blocked", { blocked });
}

export const quitApp = () => native<void>("quit_app");

/** Native menu items (File › Open…, View › Parser, …). */
export function onMenu(handler: (id: string) => void): () => void {
  if (!isTauri()) return () => {};
  let dispose: (() => void) | undefined;
  let cancelled = false;
  void import("@tauri-apps/api/event").then(({ listen }) =>
    listen<string>("app-menu", (event) => handler(event.payload)).then((unlisten) => {
      if (cancelled) unlisten();
      else dispose = unlisten;
    }),
  );
  return () => {
    cancelled = true;
    dispose?.();
  };
}
