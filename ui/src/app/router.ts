// Three places: the landing page and two workspaces. The web build keeps them
// in the URL so they can be shared; the desktop has no landing page and
// remembers the last workspace.
import { useCallback, useSyncExternalStore } from "react";
import { isDesktop } from "../api";
import { read } from "../lib/stored";

export const WORKSPACES = ["parser", "cookbooks"] as const;
export type Workspace = (typeof WORKSPACES)[number];
export type Route = "landing" | Workspace;

const KEY = "v2:workspace";
const base = import.meta.env.BASE_URL.replace(/\/$/, "");

function fromPath(pathname: string): Route {
  const rest = pathname.slice(base.length).replace(/^\/+|\/+$/g, "");
  return (WORKSPACES as readonly string[]).includes(rest) ? (rest as Workspace) : "landing";
}

let desktopRoute: Route = isDesktop ? read<Workspace>(KEY, "parser", WORKSPACES) : "landing";
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((listener) => listener());

function subscribe(listener: () => void) {
  listeners.add(listener);
  window.addEventListener("popstate", listener);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("popstate", listener);
  };
}

const current = (): Route => (isDesktop ? desktopRoute : fromPath(location.pathname));

/** Go somewhere. `search` replaces the query string on the web. */
export function navigate(route: Route, search = "") {
  if (isDesktop) {
    desktopRoute = route === "landing" ? "parser" : route;
    localStorage.setItem(KEY, JSON.stringify(desktopRoute));
  } else {
    const path = `${base}/${route === "landing" ? "" : route}${search}`;
    if (path !== location.pathname + location.search) history.pushState(null, "", path);
    window.scrollTo(0, 0);
  }
  notify();
}

export function useRoute(): [Route, typeof navigate] {
  const route = useSyncExternalStore(subscribe, current, () => "landing" as Route);
  return [route, useCallback(navigate, [])];
}

/** A query parameter on the web; `null` on the desktop. */
export function param(name: string): string | null {
  return isDesktop ? null : new URLSearchParams(location.search).get(name);
}

/** Replace query parameters without adding history entries. */
export function setParams(values: Record<string, string | null>) {
  if (isDesktop) return;
  const params = new URLSearchParams(location.search);
  for (const [key, value] of Object.entries(values)) {
    if (value === null || value === "") params.delete(key);
    else params.set(key, value);
  }
  const search = params.toString();
  history.replaceState(null, "", location.pathname + (search ? `?${search}` : ""));
}
