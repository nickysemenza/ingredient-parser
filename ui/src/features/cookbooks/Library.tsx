import { BookOpen, FileUp, FolderOpen, LayoutGrid, List, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { LibraryBook } from "../../api";
import { pickPath } from "../../api/platform";
import { Badge, Button, IconButton, Input } from "../../components/ui";
import { cn } from "../../lib/cn";
import { useStored } from "../../lib/stored";
import { CatalogPanel } from "./Catalog";
import { Cover, plural } from "./parts";

/** The desktop library: a Calibre folder of EPUBs. */
export function Library({
  books,
  directory,
  loading,
  onDirectory,
  onOpenBook,
  onOpenFile,
  onCataloging,
}: {
  books: LibraryBook[];
  directory: string;
  loading: boolean;
  onDirectory: (value: string) => void;
  onOpenBook: (path: string) => void;
  onOpenFile: () => void;
  onCataloging: (busy: boolean) => void;
}) {
  const [selected, setSelected] = useState<string[]>([]);
  useEffect(() => setSelected((paths) => paths.filter((p) => books.some((b) => b.path === p))), [books]);
  const [grid, setGrid] = useStored("v2:library-grid", true);
  const [filter, setFilter] = useState("");
  const needle = filter.trim().toLowerCase();
  const shown = books.filter((b) => !needle || b.title.toLowerCase().includes(needle) || b.authors.some((a) => a.toLowerCase().includes(needle)));
  const toggle = (path: string, on: boolean) => setSelected((was) => (on ? [...was, path] : was.filter((p) => p !== path)));
  return (
    <section aria-label="Cookbook library" className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={() => void pickPath("directory").then((value) => value && onDirectory(value))}>
          <FolderOpen />
          Library folder…
        </Button>
        <Button onClick={onOpenFile}>
          <BookOpen />
          Open EPUB…
        </Button>
        <div className="relative ml-auto w-60">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-faint" />
          <Input aria-label="Find book" placeholder="Find book…" className="pl-8" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
        <IconButton label={grid ? "List view" : "Grid view"} onClick={() => setGrid(!grid)}>
          {grid ? <List /> : <LayoutGrid />}
        </IconButton>
      </div>
      <p className="text-[11.5px] text-faint">
        <span className="font-mono">{directory || "Default Calibre folder"}</span> · {loading ? "Scanning…" : `${shown.length} of ${plural(books.length, "book")}`}
        {selected.length > 0 && ` · ${selected.length} selected for catalog`}
      </p>
      {selected.length > 0 && <CatalogPanel paths={selected} onBusy={onCataloging} />}
      {shown.length ? (
        <ul className={cn(grid ? "grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3" : "divide-y divide-line overflow-hidden rounded-panel border border-line bg-evidence")}>
          {shown.map((book) => (
            <li key={book.path} className={cn("group relative", grid ? "" : "flex items-center gap-3 px-3 py-2")}>
              <input
                type="checkbox"
                aria-label={`Select ${book.title} for catalog`}
                checked={selected.includes(book.path)}
                onChange={(e) => toggle(book.path, e.target.checked)}
                className={cn("accent-(--accent)", grid && "absolute top-2 left-2 z-1 size-4 opacity-0 group-hover:opacity-100 checked:opacity-100")}
              />
              <button
                type="button"
                disabled={Boolean(book.error)}
                onClick={() => onOpenBook(book.path)}
                aria-label={`Open ${book.title}`}
                className={cn("min-w-0 text-left disabled:opacity-50", grid ? "flex w-full flex-col gap-2" : "flex flex-1 items-center gap-3")}
              >
                <Cover path={book.path} className={grid ? "aspect-[2/3] w-full shadow-sm transition-transform group-hover:-translate-y-0.5 group-hover:shadow-md" : "h-12 w-8 shrink-0"} />
                <span className="min-w-0">
                  <span className="line-clamp-2 text-[12.5px] leading-snug font-medium">{book.title}</span>
                  <span className="block truncate text-[11.5px] text-muted">{book.authors.join(", ") || "Unknown author"}</span>
                  <span className="mt-1 flex flex-wrap gap-1">
                    {book.cookbookHint && <Badge tone="accent">cookbook</Badge>}
                    {book.runs.length > 0 && <Badge>{plural(book.runs.length, "run")}</Badge>}
                  </span>
                  {book.error && <span className="block text-[11px] text-bad">{book.error}</span>}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="rounded-panel border border-dashed border-line p-8 text-center text-[12.5px] text-muted">
          {loading ? "Scanning the library folder…" : books.length ? "No book matches this filter." : "No EPUBs in this folder. Choose another folder, or open a single EPUB."}
        </p>
      )}
    </section>
  );
}

/** The browser's library: whatever the user drops in. Nothing leaves the
 *  page; the files are read by the WASM worker. */
export function DropZone({ onFiles, compact }: { onFiles: (files: File[]) => void; compact?: boolean }) {
  const input = useRef<HTMLInputElement>(null);
  const [over, setOver] = useState(false);
  return (
    <div
      role="button"
      tabIndex={0}
      aria-label="Open EPUB or run files"
      onClick={() => input.current?.click()}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && input.current?.click()}
      onDragOver={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        setOver(false);
        onFiles([...e.dataTransfer.files]);
      }}
      className={cn(
        "flex cursor-pointer flex-col items-center justify-center rounded-panel border-2 border-dashed text-center transition-colors",
        compact ? "gap-1 p-4" : "gap-3 p-10",
        over ? "border-accent bg-accent/8" : "border-line bg-evidence hover:border-line-strong",
      )}
    >
      <input ref={input} type="file" multiple hidden accept=".epub,.json,application/epub+zip,application/json" aria-label="EPUB or run files" onChange={(e) => {
        onFiles([...(e.target.files ?? [])]);
        e.target.value = "";
      }} />
      <span className={cn("flex items-center justify-center rounded-2xl bg-accent/10 text-accent", compact ? "size-8" : "size-12")}>
        <FileUp className={compact ? "size-4" : "size-5"} />
      </span>
      <span className="text-[13.5px] font-medium">Drop a cookbook EPUB or a saved run</span>
      {!compact && <span className="max-w-md text-[12.5px] text-muted">Files stay in this tab: the parser and EPUB reader run here, compiled to WebAssembly. Add the run's EPUB too to see its photos and source lines.</span>}
    </div>
  );
}
