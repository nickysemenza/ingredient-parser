import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
const fixture = JSON.parse(
  readFileSync(
    process.env.UI_FIXTURE_PATH ?? "/tmp/food-app-qa/frontend-fixture.json",
    "utf8",
  ),
);
interface FixtureItem {
  kind: string;
  id: string;
  title: string;
  sections?: { ingredients: { raw: string }[]; steps: { text: string }[] }[];
}
const extraction = fixture.extraction;
const bookTitle: string = extraction.cookbook.source.title;
const items: FixtureItem[] = extraction.cookbook.chapters.flatMap(
  (chapter: { items: FixtureItem[] }) => chapter.items,
);
const recipe = items.find((item) => item.kind === "recipe")!;
const ingredient = recipe.sections![0].ingredients[0].raw;
const chapterTitle: string = extraction.cookbook.chapters[0].title;
const model: string = extraction.report.usage_by_model[0].model;
const chunkId: string = extraction.report.chunks[0].id;
const savedRun = fixture.runs[0];

async function openCookbooks(page: Page) {
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Library", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".library-entry").first()).toBeVisible({
    timeout: 15_000,
  });
}
async function openSavedRun(page: Page) {
  await openCookbooks(page);
  await page
    .locator(".run-history")
    .getByRole("button", { name: "Open", exact: true })
    .first()
    .click();
  await expect(page.getByRole("heading", { name: bookTitle })).toBeVisible();
}
const calls = (page: Page) =>
  page.evaluate(
    () =>
      (
        window as unknown as {
          __calls: { command: string; args: Record<string, unknown> }[];
        }
      ).__calls ?? [],
  );

test.beforeEach(async ({ page }) => {
  await page.addInitScript((data) => {
    const pixel =
      "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
    (
      window as unknown as {
        __FIXTURE_INVOKE__: (
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<unknown>;
      }
    ).__FIXTURE_INVOKE__ = async (command, args) => {
      const recorder = window as unknown as {
        __calls: { command: string; args: unknown }[];
      };
      recorder.__calls ??= [];
      recorder.__calls.push({ command, args });
      if (
        command === "reveal_file" ||
        command === "open_source_url" ||
        command === "cancel_extraction" ||
        command === "delete_run"
      )
        return null;
      if (command === "parse_batch") return data.ingredients;
      if (command === "inspect_ingredient") {
        const inspection = data.inspections[String(args?.input)];
        if (!inspection)
          throw new Error(
            `Missing real inspection fixture for ${String(args?.input)}`,
          );
        return inspection;
      }
      if (command === "dialog_open")
        return args?.kind === "directory" ? "fixture-library" : "fixture.epub";
      if (command === "dialog_save") return "fixture-run.json";
      if (command === "load_corpus") return data.corpus;
      if (command === "load_recipe") return data.webRecipe;
      if (command === "scale_web_recipe") {
        if (args?.factor !== 2)
          throw new Error("Only actual 2× web scaling fixture is available");
        return data.scaledWebRecipe;
      }
      if (command === "scan_library") return data.library;
      if (command === "open_book") return data.book;
      if (command === "estimate_book") return data.estimate;
      if (command === "gateway_status")
        return { ...data.gateway, configured: true, error: null };
      if (command === "list_runs") return data.runs;
      if (command === "open_run") return data.extraction;
      if (command === "book_image")
        return { path: String(args?.image), dataUrl: pixel };
      if (command === "load_cover")
        return { path: String(args?.path), dataUrl: pixel };
      if (command === "extract_book") {
        const channel = args?.onProgress as {
          onmessage: (value: unknown) => void;
        };
        channel.onmessage({
          phase: "extract",
          done: 1,
          total: 4,
          in_flight: 2,
          failed: 0,
          cached: 1,
          recipes_so_far: 2,
          cost_so_far_usd: 0.0011,
          elapsed_ms: 4200,
          eta: {
            remaining_low_ms: 8000,
            remaining_high_ms: 64000,
            projected_cost_usd: 0.0029,
          },
          active_models: [data.estimate.ladder[0]],
        });
        const control = window as unknown as {
          __holdExtraction?: boolean;
          __finishExtraction?: () => void;
        };
        if (control.__holdExtraction)
          await new Promise<void>((resolve) => {
            control.__finishExtraction = resolve;
          });
        return data.runs[0];
      }
      throw new Error(`Unimplemented fixture command: ${command}`);
    };
  }, fixture);
});

for (const size of [
  { width: 1440, height: 900 },
  { width: 800, height: 560 },
])
  test(`browse and inspect at ${size.width}×${size.height}`, async ({
    page,
  }) => {
    await page.setViewportSize(size);
    await page.goto("/");
    await page
      .getByRole("textbox", { name: "Ingredient lines" })
      .fill(
        fixture.ingredients
          .map((row: { input: string }) => row.input)
          .join("\n"),
      );
    await page.getByRole("button", { name: "Parse", exact: true }).click();
    await expect(
      page.getByRole("region", { name: "Ingredient inspector" }),
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/parser-${size.width}.png`,
      fullPage: true,
    });
    await page.getByRole("button", { name: "Light appearance" }).click();
    await page.screenshot({
      path: `test-results/parser-light-${size.width}.png`,
      fullPage: true,
    });
    await page.getByRole("button", { name: "Dark appearance" }).click();
    await openSavedRun(page);
    await expect(page.locator(".book-tree")).toContainText(chapterTitle);
    await expect(page.locator(".book-tree")).toContainText(recipe.title);
    await expect(page.locator(".item-view .document-heading h2")).toHaveText(
      recipe.title,
    );
    await expect(page.locator(".item-view .instructions")).toContainText(
      recipe.sections![0].steps[0].text,
    );
    await page.screenshot({
      path: `test-results/cookbook-${size.width}.png`,
      fullPage: true,
    });
    await page
      .locator(".item-view .ingredient-lines button:not(.chip)")
      .filter({ hasText: ingredient })
      .first()
      .click();
    await expect(
      page.getByRole("region", { name: "Ingredient inspector" }),
    ).toBeVisible();
    await expect(page.locator(".run-inspector .inspector-input")).toHaveText(
      ingredient,
    );
    await page.screenshot({
      path: `test-results/cookbook-inspector-${size.width}.png`,
      fullPage: true,
    });
    await page.getByRole("button", { name: "Close inspector" }).click();
    await expect(page.locator(".run-inspector")).toHaveCount(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    expect((await calls(page)).some((c) => c.command === "extract_book")).toBe(
      false,
    );
    await page.getByRole("button", { name: "Light appearance" }).click();
    await page.screenshot({
      path: `test-results/cookbook-light-${size.width}.png`,
      fullPage: true,
    });
  });

test("the default library scan is offline and opens a book's estimate", async ({
  page,
}) => {
  await page.goto("/");
  await openCookbooks(page);
  const entry = fixture.library[0];
  await expect(page.locator(".library-books")).toContainText(entry.title);
  await page.getByRole("textbox", { name: "Find book" }).fill(entry.title);
  await expect(page.locator(".library-entry")).toHaveCount(1);
  await page.getByRole("button", { name: "Grid view" }).click();
  await expect(page.getByRole("button", { name: "List view" })).toBeVisible();
  await page.locator(".library-entry > button").first().click();
  await expect(
    page.getByRole("region", { name: "Extraction estimate" }),
  ).toContainText(fixture.estimate.assumptions[0]);
  await expect(page.locator(".book-panel")).toContainText(
    fixture.book.classified.reasons[0],
  );
  await expect(page.locator(".book-panel")).toContainText(
    fixture.book.outline.chapters[0],
  );
  await expect(page.getByRole("button", { name: "Extract" })).toBeEnabled();
  const recorded = await calls(page);
  expect(
    recorded.find((c) => c.command === "scan_library")?.args.directory,
  ).toBe("");
  expect(recorded.find((c) => c.command === "open_book")?.args.path).toBe(
    entry.path,
  );
  expect(recorded.some((c) => c.command === "extract_book")).toBe(false);
  expect(recorded.some((c) => c.command === "dialog_open")).toBe(false);
  await page.reload();
  expect(
    await page.evaluate(() => localStorage.getItem("v1:library-grid")),
  ).toBe("true");
});

test("extraction reports progress, then opens the saved run it produced", async ({
  page,
}) => {
  await page.goto("/");
  await openCookbooks(page);
  await page.locator(".library-entry > button").first().click();
  await page.evaluate(() => {
    (window as unknown as { __holdExtraction: boolean }).__holdExtraction =
      true;
  });
  await page.getByRole("button", { name: "Extract", exact: true }).click();
  const progress = page.getByRole("status", { name: "Extraction progress" });
  await expect(progress).toContainText("1/4 chunks");
  await expect(progress).toContainText("2 in flight");
  await expect(progress).toContainText("1 cached");
  await expect(progress).toContainText("$0.0011 so far");
  await expect(progress).toContainText("~8.0 s–1 m 4 s left");
  await expect(progress).toContainText(fixture.estimate.ladder[0]);
  await expect(
    page.getByRole("button", { name: "Extract", exact: true }),
  ).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Cancel" })).toBeVisible();
  await expect(page.locator(".status-bar")).toContainText("Extracting 1/4");
  await page.evaluate(() => {
    (
      window as unknown as { __finishExtraction: () => void }
    ).__finishExtraction();
  });
  await expect(page.getByRole("heading", { name: bookTitle })).toBeVisible();
  await expect(page.locator(".book-tree")).toContainText(recipe.title);
  const recorded = await calls(page);
  expect(recorded.find((c) => c.command === "extract_book")?.args.path).toBe(
    fixture.book.path,
  );
  expect(
    recorded.filter((c) => c.command === "open_run").at(-1)?.args.path,
  ).toBe(savedRun.path);
});

test("diagnostics report the chunk table, the call log and the crosscheck", async ({
  page,
}) => {
  await page.goto("/");
  await openSavedRun(page);
  await page.getByRole("tab", { name: "Diagnostics" }).click();
  await expect(page.getByRole("region", { name: "Run summary" })).toContainText(
    extraction.report.run_id,
  );
  await expect(page.getByRole("region", { name: "Run summary" })).toContainText(
    `${extraction.report.crosscheck.matched}/${extraction.report.crosscheck.nav_titles} contents titles matched`,
  );
  await expect(page.getByRole("region", { name: "Chunks" })).toContainText(
    chunkId,
  );
  await expect(
    page.getByRole("region", { name: "Usage by model" }),
  ).toContainText(model);
  const log = page.getByRole("listbox", { name: "Model calls" });
  await expect(log.getByRole("option").first()).toContainText(model);
  await expect(log.getByRole("option")).toHaveCount(
    extraction.report.calls.length,
  );
  await page.getByRole("region", { name: "Stage timings" }).isVisible();
  await page.locator("details.card summary").click();
  await expect(page.locator("details.card .json")).toContainText(
    extraction.report.run_id,
  );
  await page.getByRole("tab", { name: "Recipes" }).click();
  await expect(page.locator(".item-view")).toContainText(recipe.title);
});

test("a reference chip jumps to the recipe it names", async ({ page }) => {
  await page.goto("/");
  await openSavedRun(page);
  const linked = items.find(
    (item) =>
      item.kind === "recipe" &&
      item.sections?.some((section) =>
        section.ingredients.some(
          (line) => (line as { ref?: unknown }).ref !== null,
        ),
      ),
  )!;
  await page
    .locator(".book-tree")
    .getByRole("button", { name: new RegExp(linked.title) })
    .click();
  const chip = page.locator(".item-view .chip").first();
  const target = (await chip.innerText()).trim();
  await chip.click();
  await expect(page.locator(".item-view .document-heading h2")).toHaveText(
    target,
  );
});

test("run history deletes a saved run after confirmation", async ({ page }) => {
  page.on("dialog", (dialog) => void dialog.accept());
  await page.goto("/");
  await openCookbooks(page);
  await expect(page.locator(".run-history")).toContainText(savedRun.book);
  await page
    .locator(".run-history")
    .getByRole("button", { name: "Delete", exact: true })
    .first()
    .click();
  await expect
    .poll(async () =>
      (await calls(page)).find((c) => c.command === "delete_run"),
    )
    .toBeTruthy();
  const recorded = await calls(page);
  expect(recorded.find((c) => c.command === "delete_run")?.args.path).toBe(
    savedRun.path,
  );
  expect(recorded.some((c) => c.command === "extract_book")).toBe(false);
});

test("an unconfigured gateway blocks extraction and names its config file", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const host = window as unknown as {
      __FIXTURE_INVOKE__: (
        command: string,
        args?: Record<string, unknown>,
      ) => Promise<unknown>;
    };
    const original = host.__FIXTURE_INVOKE__;
    host.__FIXTURE_INVOKE__ = async (command, args) => {
      const value = await original(command, args);
      if (command !== "gateway_status") return value;
      return {
        ...(value as object),
        configured: false,
        error: "CLOUDFLARE_AI_GATEWAY_BASE_URL is unset",
      };
    };
  });
  await page.goto("/");
  await openCookbooks(page);
  await page.locator(".library-entry > button").first().click();
  await expect(page.getByRole("button", { name: "Extract" })).toBeDisabled();
  await expect(page.locator(".book-panel")).toContainText(
    "Gateway credentials are not configured",
  );
  await expect(page.locator(".book-panel")).toContainText(
    fixture.gateway.configPath,
  );
  await expect(page.locator(".book-panel")).toContainText(
    "CLOUDFLARE_AI_GATEWAY_BASE_URL is unset",
  );
  expect((await calls(page)).some((c) => c.command === "extract_book")).toBe(
    false,
  );
});

test("corpus scoring preserves field context during ingredient inspection", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Corpus", exact: true }).click();
  await page.getByRole("button", { name: "Load & score" }).click();
  const regression = fixture.corpus.cases.find((row: { status: string }) =>
    ["regression", "xfail", "promote"].includes(row.status.toLowerCase()),
  );
  expect(regression).toBeTruthy();
  await page
    .getByRole("combobox", { name: "Show" })
    .selectOption(
      regression.status[0].toUpperCase() +
        regression.status.slice(1).toLowerCase(),
    );
  await page
    .getByRole("textbox", { name: "Find corpus input" })
    .fill(regression.input);
  const list = page.getByRole("listbox", { name: "Corpus rows" });
  await expect(list.getByRole("option")).toHaveCount(1);
  await list.getByRole("option").click();
  await expect(page.getByRole("table")).toContainText(
    regression.fields[0].expected,
  );
  await page.getByRole("button", { name: "Inspect ingredient" }).click();
  await expect(page.locator(".corpus-evidence .inspector-input")).toHaveText(
    regression.input,
  );
  await expect(
    page.getByRole("tab", { name: "Corpus", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "Back to field comparison" }).click();
  await expect(page.getByRole("table")).toContainText(
    regression.fields[0].actual,
  );
});

test("web recipe loads human sections and scales through the backend", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Recipe URL" }).click();
  await page
    .getByRole("textbox", { name: "Recipe URL" })
    .fill(fixture.webRecipe.url);
  await page.getByRole("button", { name: "Load recipe" }).click();
  await expect(
    page.getByRole("heading", { name: fixture.webRecipe.title, exact: true }),
  ).toBeVisible();
  await page.getByRole("tab", { name: "Recipe & source" }).click();
  await page
    .getByRole("button", { name: "Open original recipe in browser" })
    .click();
  await expect(page.locator(".web-recipe .instructions")).toContainText(
    fixture.webRecipe.sections[0].instructions[0],
  );
  await page
    .getByRole("combobox", { name: "Web recipe scale" })
    .selectOption("2");
  await expect(page.locator(".web-recipe .ingredient-lines")).toContainText(
    fixture.scaledWebRecipe.sections[0].ingredients[0],
  );
  const recorded = await calls(page);
  expect(recorded.find((c) => c.command === "open_source_url")?.args.url).toBe(
    fixture.webRecipe.url,
  );
  expect(
    recorded.find((c) => c.command === "scale_web_recipe")?.args.factor,
  ).toBe(2);
});

test("parser failure evidence belongs to the selected failure input", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("textbox", { name: "Ingredient lines" })
    .fill(
      fixture.ingredients.map((r: { input: string }) => r.input).join("\n"),
    );
  await page.getByRole("button", { name: "Parse", exact: true }).click();
  await page
    .getByRole("listbox", { name: "Parsed ingredients" })
    .getByRole("option")
    .filter({ hasText: "???" })
    .click();
  await expect(page.locator(".parser .inspector-input")).toHaveText("???");
  await page.getByRole("tab", { name: "Trace tree", exact: true }).click();
  await expect(page.locator(".tree-node").first()).toBeVisible();
  await page.screenshot({
    path: "test-results/parser-failure.png",
    fullPage: true,
  });
});
