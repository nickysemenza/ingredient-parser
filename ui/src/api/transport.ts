import { invoke } from "@tauri-apps/api/core";
import { wasm } from "./wasm";

declare global {
  interface Window {
    /** Browser E2E: answers desktop-only commands from a recorded fixture. */
    __FIXTURE_INVOKE__?: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
    /** Browser E2E: files (base64) the worker serves under native paths. */
    __FIXTURE_FILES__?: Record<string, string>;
  }
}

export const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Which shell the app runs in. Fixture runs exercise the desktop shell. */
export type Shell = "desktop" | "web";
export const shell: Shell =
  isTauri() || (typeof window !== "undefined" && window.__FIXTURE_INVOKE__) ? "desktop" : "web";
export const isDesktop = shell === "desktop";

/** Where portable commands run: native Rust, or Rust compiled to WASM. */
export const engine = isTauri() ? "Native Rust" : "Rust · WebAssembly";

export class DesktopOnly extends Error {
  constructor(what: string) {
    super(`${what} needs the desktop app.`);
  }
}

let fixturesLoaded: Promise<void> | null = null;
function loadFixtureFiles() {
  fixturesLoaded ??= Promise.all(
    Object.entries(window.__FIXTURE_FILES__ ?? {}).map(([path, base64]) => {
      const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
      return wasm.put(path, bytes.buffer);
    }),
  ).then(() => undefined);
  return fixturesLoaded;
}

/** A portable command (`food_core::COMMANDS`): the same name and arguments on
 *  both hosts, answered by the same Rust. */
export async function core<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (isTauri()) return invoke<T>("core", { command, args });
  if (window.__FIXTURE_FILES__) await loadFixtureFiles();
  return wasm.call<T>(command, args);
}

/** A desktop-only command. */
export async function native<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (window.__FIXTURE_INVOKE__) return window.__FIXTURE_INVOKE__(command, args) as Promise<T>;
  if (!isTauri()) throw new DesktopOnly(command);
  return invoke<T>(command, args);
}

/** Errors arrive as strings from Rust and Errors from JavaScript. */
export function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
