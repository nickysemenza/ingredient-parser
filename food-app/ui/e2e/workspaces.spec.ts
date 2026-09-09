import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
const fixture = JSON.parse(
  readFileSync(
    process.env.UI_FIXTURE_PATH ?? "/tmp/food-app-qa/frontend-fixture.json",
    "utf8",
  ),
);
test.beforeEach(async ({ page }) => {
  await page.addInitScript((data) => {
    (
      window as unknown as {
        __FIXTURE_INVOKE__: (
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<unknown>;
      }
    ).__FIXTURE_INVOKE__ = async (command, args) => {
      const calls = window as unknown as {
        __calls: { command: string; args: unknown }[];
      };
      calls.__calls ??= [];
      calls.__calls.push({ command, args });
      if (command === "reveal_file" || command === "open_source_url")
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
        return args?.kind === "directory"
          ? "fixture-library"
          : args?.kind === "epub"
            ? "fixture.epub"
            : "fixture.json";
      if (command === "open_run") return data.cookbook;
      if (command === "inspect_book") return data.sourceOnly;
      if (command === "save_review")
        return {
          ...data.cookbook,
          review: [
            ...data.cookbook.review.filter(
              (r: { document: string }) => r.document !== args?.document,
            ),
            {
              document: args?.document,
              status: args?.status,
              note: args?.note,
            },
          ],
        };
      if (command === "dialog_save") return "extracted-fixture.json";
      if (command === "extract_run") {
        const channel = args?.onProgress as {
          onmessage: (data: unknown) => void;
        };
        channel.onmessage({
          path: "extracted-fixture.json",
          completed: 1,
          total: 3,
          recipes: 1,
          reservedUsd: 0,
        });
        await new Promise((resolve) => setTimeout(resolve, 150));
        return data.cookbook;
      }
      if (command === "load_corpus") return data.corpus;
      if (command === "scan_library") return data.library;
      if (command === "run_stats") return data.stats;
      if (command === "run_audit") return data.audit;
      if (command === "load_recipe") return data.webRecipe;
      if (command === "scale_web_recipe") {
        if (args?.factor !== 2)
          throw new Error("Only actual 2× web scaling fixture is available");
        return data.scaledWebRecipe;
      }
      if (command === "replay_run")
        return { ...data.cookbook, path: args?.out };
      if (command === "load_cover") return null;
      if (command === "load_images") return [];
      if (command === "scale_recipe") {
        if (args?.index !== 0 || args?.factor !== 2)
          throw new Error(
            "Only actual first recipe 2× scaling fixture is available",
          );
        return data.scaledCookbook;
      }
      throw new Error(`Unimplemented fixture command: ${command}`);
    };
  }, fixture);
});
for (const size of [
  { width: 1440, height: 900 },
  { width: 800, height: 560 },
])
  test(`review and inspect at ${size.width}×${size.height}`, async ({
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
    await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
    await page
      .getByRole("button", { name: "Open saved run", exact: true })
      .click();
    await expect(
      page.getByRole("button", { name: "Save review", exact: true }),
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/cookbook-${size.width}.png`,
      fullPage: true,
    });
    const ingredient = page.locator(".ingredient-lines button:visible").first();
    const clickedInput = await ingredient.innerText();
    await ingredient.click();
    await expect(
      page.getByRole("region", { name: "Ingredient inspector" }),
    ).toBeVisible();
    await expect(page.locator(".cookbooks .inspector-input")).toHaveText(
      clickedInput,
    );
    if (size.width === 800)
      await page.screenshot({
        path: "test-results/cookbook-focused-inspector.png",
        fullPage: true,
      });
    if (size.width === 800) {
      await expect(
        page.getByRole("button", { name: "Back to review" }),
      ).toBeVisible();
      await page.getByRole("button", { name: "Back to review" }).click();
    } else await page.getByRole("button", { name: "Close inspector" }).click();
    const select = page.getByRole("combobox", {
      name: "Review status",
      exact: true,
    });
    await select.selectOption("Accepted");
    await page
      .getByRole("button", { name: "Save review", exact: true })
      .click();
    await expect(page.getByText("Saved", { exact: true })).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    const calls = await page.evaluate(
      () => (window as unknown as { __calls: { command: string }[] }).__calls,
    );
    expect(calls.some((c) => c.command === "extract_run")).toBe(false);
    await page.getByRole("button", { name: "Light appearance" }).click();
    await page.screenshot({
      path: `test-results/cookbook-light-${size.width}.png`,
      fullPage: true,
    });
  });
test("opening source is offline and extraction dialog traps focus", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Open cookbook", exact: true })
    .click();
  await page.getByRole("button", { name: "Extraction…", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel("Allow network requests")).not.toBeChecked();
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  expect(
    await page.evaluate(() =>
      (window as unknown as { __calls: { command: string }[] }).__calls.some(
        (c) => c.command === "extract_run",
      ),
    ),
  ).toBe(false);
});

test("cache-only extraction is explicit and preferences restore idle", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Open cookbook", exact: true })
    .click();
  await page.getByRole("button", { name: "Extraction…", exact: true }).click();
  await page
    .getByRole("button", { name: "Extract to saved run…", exact: true })
    .click();
  await expect(page.locator(".cookbooks .progress")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Extraction…", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Save review", exact: true }),
  ).toBeVisible();
  const calls = await page.evaluate(
    () =>
      (
        window as unknown as {
          __calls: {
            command: string;
            args: { request?: { allowNetwork: boolean; out: string } };
          }[];
        }
      ).__calls,
  );
  const extraction = calls.find((c) => c.command === "extract_run");
  expect(extraction?.args.request?.allowNetwork).toBe(false);
  expect(extraction?.args.request?.out).toBe("extracted-fixture.json");
  await page.reload();
  await expect(
    page.getByRole("button", { name: "Open cookbook", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => (window as unknown as { __calls?: unknown[] }).__calls ?? [],
    ),
  ).toHaveLength(0);
});
test("canceling unsaved document switch retains the current document", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Open saved run", exact: true })
    .click();
  await page
    .getByRole("combobox", { name: "Review status", exact: true })
    .selectOption("Incorrect");
  page.once("dialog", (dialog) => dialog.dismiss());
  await page.getByRole("option", { name: /Flatbread/ }).click();
  await expect(page.locator(".document-heading h2")).toHaveText(
    fixture.cookbook.documents[0].blocks.find((b: { tag: string }) =>
      /^h[123]$/.test(b.tag),
    ).text,
  );
  await expect(page.getByText("Unsaved", { exact: true })).toBeVisible();
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
  await expect(page.locator(".web-recipe .instructions")).toContainText(
    fixture.scaledWebRecipe.sections[0].instructions[0],
  );
  const calls = await page.evaluate(
    () =>
      (
        window as unknown as {
          __calls: { command: string; args: Record<string, unknown> }[];
        }
      ).__calls,
  );
  expect(calls.find((c) => c.command === "open_source_url")?.args.url).toBe(
    fixture.webRecipe.url,
  );
  expect(calls.find((c) => c.command === "scale_web_recipe")?.args.factor).toBe(
    2,
  );
});
test("library scan opens source without extraction and preserves grid preference", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page.locator(".workspace-heading summary").click();
  await page.getByRole("button", { name: "Library folder…" }).click();
  const book = fixture.library.find(
    (book: { cookbook: boolean }) => book.cookbook,
  );
  expect(book).toBeTruthy();
  await page.getByRole("textbox", { name: "Find book" }).fill(book.title);
  await expect(page.locator(".library-books")).toContainText(book.title);
  await page.getByRole("button", { name: "Grid view" }).click();
  await expect(page.getByRole("button", { name: "List view" })).toBeVisible();
  await page.locator(".library-books>button").first().click();
  await expect(
    page.getByRole("button", { name: "Extraction…", exact: true }),
  ).toBeVisible();
  const calls = await page.evaluate(
    () =>
      (
        window as unknown as {
          __calls: { command: string; args: Record<string, unknown> }[];
        }
      ).__calls,
  );
  expect(calls.find((c) => c.command === "inspect_book")?.args.path).toBe(
    book.path,
  );
  expect(calls.some((c) => c.command === "extract_run")).toBe(false);
  await page.reload();
  expect(
    await page.evaluate(() => localStorage.getItem("v1:library-grid")),
  ).toBe("true");
});
test("saved run statistics, references, replay and scaling retain real evidence", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Open saved run", exact: true })
    .click();
  await page
    .locator('select[aria-label="Recipe scale"]:visible')
    .selectOption("2");
  await expect(page.locator(".ingredient-lines:visible")).toContainText(
    fixture.scaledCookbook.sections[0].ingredients[0],
  );
  await page.getByText("Run tools", { exact: true }).click();
  await page
    .getByRole("button", { name: "Ingredient statistics", exact: true })
    .click();
  await expect(
    page.getByRole("listbox", { name: "Ingredient statistics" }),
  ).toBeVisible();
  const stat = fixture.stats.names[0];
  await page
    .getByRole("textbox", { name: "Find ingredient name" })
    .fill(stat.name);
  await page
    .getByRole("listbox", { name: "Ingredient statistics" })
    .getByRole("option")
    .first()
    .click();
  const example = page.locator(".stat-example button").first();
  const input = await example.innerText();
  await example.click();
  await expect(page.locator(".tool-inspector .inspector-input")).toHaveText(
    input,
  );
  await page.getByRole("button", { name: "Back to source review" }).click();
  await page.getByText("Run tools", { exact: true }).click();
  await page
    .getByRole("button", { name: "Reference graph", exact: true })
    .click();
  await expect(page.locator(".reference-graph")).toBeVisible();
  const linked = fixture.cookbook.recipes[0];
  await expect(
    page.getByText(
      "No cross-recipe references in this run. Select a recipe to review its source.",
    ),
  ).toBeVisible();
  await page
    .getByRole("button", { name: `Open ${linked.title}`, exact: true })
    .click();
  await expect(page.locator(".document-heading h2")).toContainText(
    linked.title,
  );
  await page.getByText("Run tools", { exact: true }).click();
  await page.getByRole("button", { name: "Replay to new run…" }).click();
  await expect
    .poll(async () =>
      page.evaluate(() =>
        (window as unknown as { __calls: { command: string }[] }).__calls.some(
          (c) => c.command === "replay_run",
        ),
      ),
    )
    .toBe(true);
  await expect(page.locator(".document-heading h2")).toBeVisible();
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

test("recent runs restore idle and reopen through the native bridge", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Open saved run", exact: true })
    .click();
  await page.getByText("Run tools", { exact: true }).click();
  await page
    .getByRole("button", { name: "Reveal saved run in Finder", exact: true })
    .click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (
            window as unknown as { __calls: { command: string }[] }
          ).__calls.filter((call) => call.command === "reveal_file").length,
      ),
    )
    .toBe(1);
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Source and result, side by side" }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => (window as unknown as { __calls?: unknown[] }).__calls ?? [],
    ),
  ).toEqual([]);
  await page
    .locator(".workspace-heading summary")
    .filter({ hasText: /^Open/ })
    .click();
  await page.locator(".recent-run").first().click();
  await expect(page.locator(".document-heading h2")).toBeVisible();
  await page
    .locator(".workspace-heading summary")
    .filter({ hasText: /^Open/ })
    .click();
  await page.getByRole("button", { name: "Clear recent runs" }).click();
  await expect(page.locator(".recent-run")).toHaveCount(0);
});

test("review shortcuts save before advancing and preserve notes", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Open saved run", exact: true })
    .click();
  const original = await page.locator(".document-heading h2").innerText();
  // Put a second real document back into the review queue.
  await page
    .getByRole("listbox", { name: "Source documents" })
    .getByRole("option")
    .filter({ hasText: "Flatbread" })
    .click();
  await page
    .getByRole("combobox", { name: "Review status", exact: true })
    .selectOption("Unreviewed");
  await page.getByRole("button", { name: "Save review", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Save review", exact: true }),
  ).toBeDisabled();
  await page
    .getByRole("listbox", { name: "Source documents" })
    .getByRole("option")
    .first()
    .click();

  await page
    .locator("summary")
    .filter({ hasText: /^Review note/ })
    .click();
  await page
    .getByRole("textbox", { name: "Review note", exact: true })
    .fill("Checked against source");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Meta+Alt+KeyA");
  await expect(
    page.getByRole("combobox", { name: "Review status", exact: true }),
  ).toHaveValue("Accepted");
  await expect(page.getByLabel("Workspace status")).toContainText(
    "Unsaved review",
  );
  await page.keyboard.press("Meta+Shift+Enter");
  await expect(page.locator(".document-heading h2")).not.toHaveText(original);
  const saves = await page.evaluate(() =>
    (
      window as unknown as {
        __calls: { command: string; args: Record<string, unknown> }[];
      }
    ).__calls.filter((call) => call.command === "save_review"),
  );
  expect(saves).toHaveLength(2);
  expect(saves[1].args).toMatchObject({
    status: "Accepted",
    note: "Checked against source",
  });
});

test("failed save keeps the current document and unsaved review", async ({
  page,
}) => {
  await page.goto("/");
  await page.evaluate(() => {
    const previous = (
      window as unknown as {
        __FIXTURE_INVOKE__: (
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<unknown>;
      }
    ).__FIXTURE_INVOKE__;
    (
      window as unknown as { __FIXTURE_INVOKE__: typeof previous }
    ).__FIXTURE_INVOKE__ = (command, args) => {
      if (command === "save_review")
        return Promise.reject(new Error("Review file is read-only"));
      return previous(command, args);
    };
  });
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Open saved run", exact: true })
    .click();
  const original = await page.locator(".document-heading h2").innerText();
  await page.keyboard.press("Meta+Alt+KeyA");
  await page.keyboard.press("Meta+Shift+Enter");
  await expect(page.getByRole("alert")).toContainText(
    "Review file is read-only",
  );
  await expect(page.locator(".document-heading h2")).toHaveText(original);
  await expect(page.getByLabel("Workspace status")).toContainText(
    "Unsaved review",
  );
  await expect(
    page.getByRole("combobox", { name: "Review status", exact: true }),
  ).toHaveValue("Accepted");
});
