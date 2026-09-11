import { useCallback, useEffect, useRef, useState } from "react";
import { BookOpen, FolderOpen, Grid2x2, List, Search } from "lucide-react";
import { api, pick, type LibraryBook } from "../bridge";
import { useStored } from "../components";

/**
 * Covers are read from the archive one by one, so they are fetched when an
 * entry scrolls into view (or is hovered) and remembered for the session.
 */
const covers = new Map<string, string | null>();
function Cover({ path }: { path: string }) {
  const [url, setUrl] = useState<string | null>(() => covers.get(path) ?? null);
  const ref = useRef<HTMLSpanElement>(null);
  const load = useCallback(() => {
    if (covers.has(path)) return;
    covers.set(path, null);
    api
      .cover(path)
      .then((image) => {
        covers.set(path, image?.dataUrl ?? null);
        setUrl(image?.dataUrl ?? null);
      })
      .catch(() => covers.set(path, null));
  }, [path]);
  useEffect(() => {
    const node = ref.current;
    if (!node || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        load();
        observer.disconnect();
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [load]);
  return (
    <span className="library-cover" ref={ref} onPointerEnter={load}>
      {url ? <img src={url} alt="" /> : <BookOpen size={18} />}
    </span>
  );
}

export function Library({
  books,
  directory,
  loading,
  onDirectory,
  onOpenBook,
  onOpenFile,
}: {
  books: LibraryBook[];
  directory: string;
  loading: boolean;
  onDirectory: (value: string) => void;
  onOpenBook: (path: string) => void;
  onOpenFile: () => void;
}) {
  const [grid, setGrid] = useStored("v1:library-grid", false);
  const [filter, setFilter] = useState("");
  const needle = filter.trim().toLowerCase();
  const shown = books.filter(
    (book) =>
      !needle ||
      book.title.toLowerCase().includes(needle) ||
      book.authors.some((author) => author.toLowerCase().includes(needle)),
  );
  return (
    <section className="library" aria-label="Cookbook library">
      <div className="library-bar">
        <button
          onClick={() =>
            void pick("directory").then((value) => {
              if (value) onDirectory(value);
            })
          }
        >
          <FolderOpen size={14} />
          Library folder…
        </button>
        <button onClick={onOpenFile}>
          <BookOpen size={14} />
          Open EPUB…
        </button>
        <label className="search">
          <Search size={14} />
          <input
            aria-label="Find book"
            placeholder="Find book…"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
        </label>
        <button onClick={() => setGrid(!grid)}>
          {grid ? <List size={14} /> : <Grid2x2 size={14} />}
          {grid ? "List view" : "Grid view"}
        </button>
      </div>
      <p className="caption library-folder">
        {directory || "Default Calibre folder"}
        {" · "}
        {loading
          ? "Scanning…"
          : `${shown.length} of ${books.length} book${books.length === 1 ? "" : "s"}`}
      </p>
      <div className={`library-books scroll ${grid ? "grid" : ""}`}>
        {shown.map((book) => (
          <div className="library-entry" key={book.path}>
            <button
              onClick={() => onOpenBook(book.path)}
              disabled={Boolean(book.error)}
            >
              <Cover path={book.path} />
              <span>
                <strong>{book.title}</strong>
                <span className="caption">
                  {book.authors.join(", ") || "Unknown author"}
                </span>
                <span className="badges">
                  {book.cookbookHint && <span className="badge">cookbook</span>}
                  <span className="badge muted">
                    {book.runs.length} saved run
                    {book.runs.length === 1 ? "" : "s"}
                  </span>
                </span>
                {book.error && (
                  <span className="error-text caption">{book.error}</span>
                )}
              </span>
            </button>
          </div>
        ))}
        {!shown.length && (
          <p className="empty-inline">
            {loading
              ? "Scanning the library folder…"
              : books.length
                ? "No book matches this filter."
                : "No EPUBs in this folder. Choose another folder, or open a single EPUB."}
          </p>
        )}
      </div>
    </section>
  );
}
