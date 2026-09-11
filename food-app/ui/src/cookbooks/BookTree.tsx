import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Image as ImageIcon, Link2, Search } from "lucide-react";
import {
  api,
  type Chapter,
  type ImageRef,
  type IngredientInspection,
  type Item,
  type Recipe,
  type RecipeRef,
  type Section,
  type Step,
} from "../bridge";
import { Inspector } from "../Inspector";
import {
  findItem,
  flattenChapters,
  formatAmounts,
  formatSpan,
  type TreeRow,
} from "./format";

/** Chapters and their items, filtered by title, as the run's navigation. */
export function BookTree({
  chapters,
  selected,
  onSelect,
}: {
  chapters: Chapter[];
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const [filter, setFilter] = useState("");
  const rows = useMemo(
    () => flattenChapters(chapters, filter),
    [chapters, filter],
  );
  const names = useMemo(() => {
    const map = new Map<string, string>();
    for (const chapter of chapters)
      for (const item of chapter.items) map.set(item.id, item.title);
    return map;
  }, [chapters]);
  return (
    <section className="book-tree" aria-label="Book contents">
      <label className="search">
        <Search size={14} />
        <input
          aria-label="Find item"
          placeholder="Find recipe…"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </label>
      <div className="scroll tree-rows">
        {rows.map((row: TreeRow) =>
          row.kind === "chapter" ? (
            <h3 className="tree-chapter" key={`chapter-${row.key}`}>
              {row.title}
              <span className="caption">{row.items}</span>
            </h3>
          ) : (
            <button
              className="tree-item"
              key={row.key}
              aria-current={selected === row.key ? "true" : undefined}
              onClick={() => onSelect(row.key)}
            >
              <span className="tree-title">
                {row.item.title}
                <span className={`badge kind ${row.item.kind}`}>
                  {row.item.kind}
                </span>
              </span>
              <span className="caption">
                {row.item.kind === "recipe"
                  ? `${row.counts.ingredients} lines · ${row.counts.steps} steps · ${row.counts.photos} photos · ${row.counts.refs} refs`
                  : `${row.counts.steps} steps · ${row.counts.photos} photos`}
              </span>
              {row.item.kind === "recipe" && row.item.variant_of && (
                <span className="caption">
                  variation of{" "}
                  {names.get(row.item.variant_of) ?? row.item.variant_of}
                </span>
              )}
            </button>
          ),
        )}
        {!rows.length && <p className="empty-inline">No matching items.</p>}
      </div>
    </section>
  );
}

function RefChip({
  reference,
  title,
  onJump,
}: {
  reference: RecipeRef;
  title: string | undefined;
  onJump: (id: string) => void;
}) {
  return (
    <button
      className="chip"
      onClick={() => onJump(reference.target_id)}
      title={`${reference.kind} reference matched by ${reference.method}`}
    >
      <Link2 size={12} />
      {title ?? reference.text}
    </button>
  );
}

function Photo({
  image,
  source,
  onNeed,
}: {
  image: ImageRef;
  source: string | undefined;
  onNeed: (path: string) => void;
}) {
  useEffect(() => onNeed(image.path), [image.path, onNeed]);
  return (
    <figure>
      {source ? (
        <img className="recipe-image" src={source} alt={image.alt ?? ""} />
      ) : (
        <p className="image-placeholder">
          <ImageIcon size={16} />
          {source === "" ? "Image unavailable" : image.path}
        </p>
      )}
      {image.caption && <figcaption>{image.caption}</figcaption>}
    </figure>
  );
}

function Lines({
  section,
  titleOf,
  onInspect,
  onJump,
}: {
  section: Section;
  titleOf: (id: string) => string | undefined;
  onInspect: (raw: string) => void;
  onJump: (id: string) => void;
}) {
  return (
    <ul className="ingredient-lines">
      {section.ingredients.map((line) => (
        <li key={line.line}>
          <button onClick={() => onInspect(line.raw)}>
            <span className="mono">{line.raw}</span>
            <span className="parsed">
              <span className="ingredient-name">{line.parsed.name}</span>
              {line.parsed.amounts.length > 0 && (
                <span className="amount">
                  {formatAmounts(line.parsed.amounts)}
                </span>
              )}
              {line.parsed.modifier && (
                <span className="caption">{line.parsed.modifier}</span>
              )}
              <span className={`badge confidence ${line.confidence}`}>
                {line.confidence}
              </span>
            </span>
          </button>
          {line.ref && (
            <RefChip
              reference={line.ref}
              title={titleOf(line.ref.target_id)}
              onJump={onJump}
            />
          )}
        </li>
      ))}
    </ul>
  );
}

function RecipeDetail({
  recipe,
  titleOf,
  onInspect,
  onJump,
}: {
  recipe: Recipe;
  titleOf: (id: string) => string | undefined;
  onInspect: (raw: string) => void;
  onJump: (id: string) => void;
}) {
  const times = recipe.meta.times;
  const timings = (
    [
      ["active", times?.active],
      ["total", times?.total],
      ["prep", times?.prep],
      ["cook", times?.cook],
    ] as const
  ).filter(([, value]) => Boolean(value));
  return (
    <>
      {recipe.meta.recipe_yield && (
        <p className="recipe-yield">{recipe.meta.recipe_yield}</p>
      )}
      {timings.length > 0 && (
        <p className="caption">
          {timings.map(([label, value]) => `${label} ${value}`).join(" · ")}
        </p>
      )}
      {recipe.meta.description.map((paragraph, index) => (
        <p key={index}>{paragraph}</p>
      ))}
      {recipe.meta.equipment.length > 0 && (
        <ul className="equipment">
          {recipe.meta.equipment.map((tool) => (
            <li key={tool}>{tool}</li>
          ))}
        </ul>
      )}
      {recipe.sections.map((section, index) => (
        <section className="recipe-section" key={section.name ?? index}>
          {section.name && <h4>{section.name}</h4>}
          <Lines
            section={section}
            titleOf={titleOf}
            onInspect={onInspect}
            onJump={onJump}
          />
          <ol className="instructions">
            {section.steps.map((step) => (
              <li key={step.line}>
                {step.text}
                {step.refs.map((reference, n) => (
                  <RefChip
                    key={n}
                    reference={reference}
                    title={titleOf(reference.target_id)}
                    onJump={onJump}
                  />
                ))}
              </li>
            ))}
          </ol>
        </section>
      ))}
      {recipe.notes.length > 0 && (
        <section className="notes">
          <h4>Notes</h4>
          {recipe.notes.map((note) => (
            <p key={note.line}>
              {note.label && <strong>{note.label}: </strong>}
              {note.text}
              {note.refs.map((reference, n) => (
                <RefChip
                  key={n}
                  reference={reference}
                  title={titleOf(reference.target_id)}
                  onJump={onJump}
                />
              ))}
            </p>
          ))}
        </section>
      )}
    </>
  );
}

function Prose({
  paragraphs,
  steps = [],
}: {
  paragraphs: string[];
  steps?: Step[];
}) {
  return (
    <>
      {paragraphs.map((paragraph, index) => (
        <p key={index}>{paragraph}</p>
      ))}
      {steps.length > 0 && (
        <ol className="instructions">
          {steps.map((step) => (
            <li key={step.line}>{step.text}</li>
          ))}
        </ol>
      )}
    </>
  );
}

/** The selected chapter item, and the ingredient inspector it can open. */
export function ItemView({
  item,
  chapters,
  bookPath,
  onJump,
  onError,
}: {
  item: Item;
  chapters: Chapter[];
  bookPath: string | null;
  onJump: (id: string) => void;
  onError: (value: string) => void;
}) {
  const [photos, setPhotos] = useState<Record<string, string>>({});
  const [inspection, setInspection] = useState<IngredientInspection | null>(
    null,
  );
  const requested = useRef(new Set<string>());
  const titleOf = useCallback(
    (id: string) => findItem(chapters, id)?.title,
    [chapters],
  );
  const onNeedPhoto = useCallback(
    (path: string) => {
      if (requested.current.has(path)) return;
      requested.current.add(path);
      if (!bookPath) {
        setPhotos((previous) => ({ ...previous, [path]: "" }));
        return;
      }
      void api
        .image(bookPath, path)
        .then((image) =>
          setPhotos((previous) => ({ ...previous, [path]: image.dataUrl })),
        )
        .catch(() => setPhotos((previous) => ({ ...previous, [path]: "" })));
    },
    [bookPath],
  );
  const inspect = useCallback(
    (raw: string) => {
      void api
        .inspect(raw)
        .then(setInspection)
        .catch((error: unknown) => onError(String(error)));
    },
    [onError],
  );
  return (
    <div className="run-detail">
      <article className="item-view scroll">
        <header className="document-heading">
          <h2>{item.title}</h2>
          {item.name !== item.title && (
            <p className="caption">unique name: {item.name}</p>
          )}
          <p className="caption">
            {item.kind} · {item.id} · {formatSpan(item.span)}
          </p>
        </header>
        {item.kind === "recipe" ? (
          <RecipeDetail
            recipe={item}
            titleOf={titleOf}
            onInspect={inspect}
            onJump={onJump}
          />
        ) : item.kind === "technique" ? (
          <Prose paragraphs={item.description} steps={item.steps} />
        ) : (
          <Prose paragraphs={item.text} />
        )}
        {item.photos.map((image) => (
          <Photo
            key={image.path}
            image={image}
            source={photos[image.path]}
            onNeed={onNeedPhoto}
          />
        ))}
        {!bookPath && item.photos.length > 0 && (
          <p className="caption">
            Open this book from the library to load its photos.
          </p>
        )}
      </article>
      {inspection && (
        <div className="run-inspector">
          <Inspector data={inspection} onClose={() => setInspection(null)} />
        </div>
      )}
    </div>
  );
}
