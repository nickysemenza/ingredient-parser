import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
const fixture = JSON.parse(
  readFileSync(
    process.env.UI_FIXTURE_PATH ?? "/tmp/food-app-qa/frontend-fixture.json",
    "utf8",
  ),
);
async function openExtraction(page: Page) {
  await expect(
    page.getByRole("heading", { name: "Library", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Extraction history", exact: true })
    .click();
  await page
    .locator(".run-history")
    .getByRole("button", { name: "Open", exact: true })
    .first()
    .click();
}
async function openSource(page: Page) {
  await expect(
    page.getByRole("heading", { name: "Library", exact: true }),
  ).toBeVisible();
  await page.locator(".workspace-heading summary").click();
  await page
    .getByRole("button", { name: "Cookbook EPUB…", exact: true })
    .click();
}
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
      if (command === "cookbook_models")
        return [
          { id: "automatic", label: "Automatic", enabled: true, status: "Verified recovery" },
          {
            id: "gemini-2.5-flash",
            label: "Gemini 2.5 Flash",
            enabled: true,
            status: "Baseline",
          },
        ];
      if (command === "extraction_preview")
        return {
          total: 3,
          cached: 0,
          pending: 3,
          lowUsd: 0,
          highUsd: 0.01,
          reservationUsd: 0.1,
          basis: "Fixture estimate",
          extraction: { usd: [0, 0.01], basis: "Fixture extraction", uncalibrated: true },
          verification: { usd: [0, 0], basis: "Fixture verification", uncalibrated: false },
        };
      if (command === "cookbook_results") return { rows: [{
        latest: { path: data.cookbook.path, title: "Fixture cookbook", model: "test-model", promptVersion: "v8", configurations: ["test-model / v8", "parent-model / v7"], createdAt: null, recipes: 3, completed: 2, total: 10, incomplete: true, reservedUsd: 0.1, newSpendUsd: null, unresolvedUsd: 0.1, qualityFlags: 1 },
        runs: 2, failedChunks: 0, pendingChunks: 8, contentReviewFlags: 1, processingSuccessRate: 1, attempts: null, failedAttempts: null,
      }], unreadable: [] };
      if (command === "cookbook_runs")
        return [
          {
            path: data.cookbook.path,
            title: "Fixture cookbook",
            model: data.cookbook.model,
            promptVersion: "v5",
            configurations: [],
            createdAt: null,
            recipes: 3,
            completed: 3,
            total: 3,
            incomplete: false,
            reservedUsd: 0,
            newSpendUsd: null,
            unresolvedUsd: 0,
          },
        ];
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
          active: 2, failed: 0, elapsedSeconds: 4, estimatedUsd: 0, stopping: false,
        });
        const control = window as unknown as {
          __holdExtraction?: boolean;
          __finishExtraction?: () => void;
        };
        if (control.__holdExtraction) {
          await new Promise<void>((resolve) => {
            control.__finishExtraction = resolve;
          });
        }
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
    await openExtraction(page);
    await expect(
      page.getByRole("region", { name: "Extraction feedback", exact: true }),
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
        page.getByRole("button", { name: "Back to source" }),
      ).toBeVisible();
      await page.getByRole("button", { name: "Back to source" }).click();
    } else await page.getByRole("button", { name: "Close inspector" }).click();
    await expect(page.getByRole("combobox", { name: "Review status", exact: true })).toHaveCount(0);
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
  await openSource(page);
  await page.getByRole("button", { name: "Extraction…", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(
    dialog.getByLabel("Cache only (no network requests)"),
  ).not.toBeChecked();
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

test("extraction saves automatically and preferences restore idle", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await openSource(page);
  await page.getByRole("button", { name: "Extraction…", exact: true }).click();
  await page.evaluate(() => {
    (window as unknown as { __holdExtraction: boolean }).__holdExtraction = true;
  });
  await page.getByRole("button", { name: "Extract", exact: true }).click();
  await expect(page.locator(".cookbooks .progress")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Extraction…", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Save review", exact: true }),
  ).toHaveCount(0);
  const calls = await page.evaluate(
    () =>
      (
        window as unknown as {
          __calls: {
            command: string;
            args: { request?: { allowNetwork: boolean; out: string; resume: boolean; model: string; strategy: string } };
          }[];
        }
      ).__calls,
  );
  const extraction = calls.find((c) => c.command === "extract_run");
  expect(extraction?.args.request?.allowNetwork).toBe(true);
  expect(extraction?.args.request?.model).toBe("automatic");
  expect(extraction?.args.request?.resume).toBe(false);
  expect(extraction?.args.request?.out).toBe("");
  expect(extraction?.args.request?.strategy).toBe("indexed");
  expect(calls.some((c) => c.command === "dialog_save")).toBe(false);
  await page.evaluate(() => {
    (window as unknown as { __finishExtraction: () => void }).__finishExtraction();
  });
  await expect(
    page.getByRole("button", { name: "Extraction…", exact: true }),
  ).toBeEnabled();
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Library", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(() =>
      (
        (window as unknown as { __calls?: { command: string }[] }).__calls ?? []
      ).filter(
        (c) => c.command !== "scan_library" && c.command !== "load_cover",
      ),
    ),
  ).toHaveLength(0);
});
test("source navigation requires no manual approval", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await openExtraction(page);
  await page.getByRole("option", { name: /Flatbread/ }).click();
  await expect(page.locator(".document-heading h2")).toContainText("Flatbread");
  await expect(page.getByRole("combobox", { name: "Review status" })).toHaveCount(0);
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
test("library scan opens existing results or source and preserves grid preference", async ({
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
  await page.locator(".library-entry > button").first().click();
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
  expect(
    calls.find(
      (c) => c.command === (book.runs?.length ? "open_run" : "inspect_book"),
    )?.args.path,
  ).toBe(book.runs?.[0]?.path ?? book.path);
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
  await openExtraction(page);
  await page
    .locator('select[aria-label="Recipe scale"]:visible')
    .selectOption("2");
  await expect(page.locator(".ingredient-lines:visible")).toContainText(
    fixture.scaledCookbook.sections[0].ingredients[0],
  );
  await page.getByText("Extraction tools", { exact: true }).click();
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
  await page.getByRole("button", { name: "Back to source" }).click();
  await page.getByText("Extraction tools", { exact: true }).click();
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
  await page.getByText("Extraction tools", { exact: true }).click();
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
  await openExtraction(page);
  await page.getByText("Extraction tools", { exact: true }).click();
  await page
    .getByRole("button", { name: "Reveal extraction in Finder", exact: true })
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
    page.getByRole("heading", { name: "Library", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(() =>
      (
        (window as unknown as { __calls?: { command: string }[] }).__calls ?? []
      ).filter(
        (c) => c.command !== "scan_library" && c.command !== "load_cover",
      ),
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
  await page.getByRole("button", { name: "Clear recent extractions" }).click();
  await expect(page.locator(".recent-run")).toHaveCount(0);
});

test("legacy review shortcuts do not write or advance", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await openExtraction(page);
  const original = await page.locator(".document-heading h2").innerText();
  await page.keyboard.press("Meta+Alt+KeyA");
  await page.keyboard.press("Meta+Shift+Enter");
  await expect(page.locator(".document-heading h2")).toHaveText(original);
  await expect(page.getByRole("button", { name: "Save review" })).toHaveCount(0);
  await expect(page.getByLabel("Extraction feedback")).toContainText("Not assessed");
  expect(await page.evaluate(() => (window as unknown as { __calls: {command: string}[] }).__calls.filter(c => c.command === "save_review"))).toHaveLength(0);
});

test("model dropdown focus and selection update preflight without extracting", async ({
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
      if (command === "cookbook_models")
        return [
          { id: "automatic", label: "Automatic", enabled: true, status: "Verified recovery" },
          {
            id: "gemini-2.5-flash",
            label: "Baseline",
            enabled: true,
            status: "Test",
          },
          { id: "gpt-5.6-luna", label: "Luna", enabled: true, status: "Test" },
        ];
      if (command === "extraction_preview") {
        const request = args?.request as { model: string };
        return {
          total: 3,
          cached: 1,
          pending: 2,
          lowUsd: 0.001,
          highUsd: request.model === "gpt-5.6-luna" ? 0.002 : 0.004,
          reservationUsd: 0.1,
          basis: "Fixture estimate",
          extraction: { usd: [0.001, request.model === "gpt-5.6-luna" ? 0.002 : 0.004], basis: "Fixture extraction", uncalibrated: true },
          verification: { usd: [0, 0], basis: "Fixture verification", uncalibrated: false },
        };
      }
      return original(command, args);
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await openSource(page);
  await page.getByRole("button", { name: "Extraction…", exact: true }).click();
  await page.getByText("Advanced options", { exact: true }).click();
  const picker = page.getByRole("combobox", { name: "Model", exact: true });
  await expect(picker).toHaveValue("automatic");
  const concurrency = page.getByRole("combobox", { name: "Concurrent requests", exact: true });
  await expect(concurrency).toHaveValue("4");
  await concurrency.selectOption("8");
  await expect(concurrency).toHaveValue("8");
  await expect(page.getByText(/estimated additional cost/)).toContainText(
    "<$0.01–<$0.01",
  );
  await page.screenshot({ path: "/tmp/cookbook-extraction-preview.png" });
  await picker.focus();
  await expect(picker).toBeFocused();
  await picker.selectOption("gpt-5.6-luna");
  await expect(picker).toHaveValue("gpt-5.6-luna");
  await expect(page.getByText(/estimated additional cost/)).toContainText(
    "<$0.01–<$0.01",
  );
  await expect(page.getByLabel("Spending limit (USD)")).toHaveValue("10");
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
});

test("saved history opens, compares, reveals and exports without extraction", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const host = window as unknown as {
      __FIXTURE_INVOKE__: (
        command: string,
        args?: Record<string, unknown>,
      ) => Promise<unknown>;
      __historyActions: string[];
    };
    host.__historyActions = [];
    const original = host.__FIXTURE_INVOKE__;
    host.__FIXTURE_INVOKE__ = async (command, args) => {
      if (command === "cookbook_runs")
        return [
          {
            path: "fixture.json",
            epubSha256: "same-book",
            status: "complete",
            title: "Fixture cookbook",
            model: "gemini-2.5-flash",
            promptVersion: "v5",
            configurations: ["gemini-2.5-flash / v5", "gpt-5.6-luna / v5"],
            createdAt: 1788950000,
            recipes: 3,
            completed: 3,
            total: 3,
            incomplete: false,
            reservedUsd: 0.01,
            newSpendUsd: 0.002,
            unresolvedUsd: 0,
          },
        ].flatMap((row) => [row, { ...row, path: "second.json", status: "interrupted", incomplete: true }, { ...row, path: "other.json", epubSha256: "other-book" }]);
      if (["export_run", "reveal_file", "run_diff"].includes(command)) {
        host.__historyActions.push(command);
        return command === "run_diff" ? { sameSource: true } : null;
      }
      return original(command, args);
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page
    .getByRole("button", { name: "Extraction history", exact: true })
    .click();
  await expect(page.getByText(/Mixed: gemini/).first()).toBeVisible();
  await expect(page.getByText(/0.0020 estimated new spend/).first()).toBeVisible();
  await page.getByRole("button", { name: "Reveal", exact: true }).first().click();
  await page.getByRole("button", { name: "Export", exact: true }).first().click();
  await page
    .locator(".run-history")
    .getByRole("button", { name: "Open", exact: true })
    .first().click();
  await page
    .getByRole("button", { name: "Extraction history", exact: true })
    .click();
  await page.getByRole("checkbox", { name: /Compare .*fixture.json/ }).check();
  await expect(page.getByRole("checkbox", { name: /Compare .*other.json/ })).toBeDisabled();
  await page.getByRole("checkbox", { name: /Compare .*second.json/ }).check();
  await page.getByRole("button", { name: "Compare selected extractions", exact: true }).click();
  const actions = await page.evaluate(
    () =>
      (window as unknown as { __historyActions: string[] }).__historyActions,
  );
  expect(actions).toEqual(["reveal_file", "export_run", "run_diff"]);
});

test("default library opens without a folder dialog", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Open saved run" }),
  ).toHaveCount(0);

  await expect(
    page.getByRole("heading", { name: "Library", exact: true }),
  ).toBeVisible();
  const calls = await page.evaluate(
    () =>
      (
        window as unknown as {
          __calls: { command: string; args: { directory?: string } }[];
        }
      ).__calls,
  );
  expect(calls.find((c) => c.command === "scan_library")?.args.directory).toBe(
    "",
  );
  expect(calls.some((c) => c.command === "dialog_open")).toBe(false);
});

test("a library cookbook opens its latest extraction without file selection", async ({
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
      const result = await original(command, args);
      if (command !== "scan_library") return result;
      const runs = await original("cookbook_runs", { book: null });
      return (result as { cookbook: boolean }[]).map((book) => ({
        ...book,
        runs: book.cookbook ? runs : [],
      }));
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page.locator(".library-entry > button").first().click();
  await expect(
    page.getByRole("region", { name: "Extraction feedback", exact: true }),
  ).toBeVisible();
  const calls = await page.evaluate(
    () => (window as unknown as { __calls: { command: string }[] }).__calls,
  );
  expect(calls.some((call) => call.command === "open_run")).toBe(true);
  expect(
    calls.some(
      (call) =>
        call.command === "dialog_open" || call.command === "inspect_book",
    ),
  ).toBe(false);
});

test("extraction shows accounting and stops without losing its result", async ({ page }) => {
  await page.addInitScript(() => {
    const host = window as unknown as { __FIXTURE_INVOKE__: (command: string, args?: Record<string, unknown>) => Promise<unknown> };
    const original = host.__FIXTURE_INVOKE__;
    let finish: ((result: unknown) => void) | undefined;
    host.__FIXTURE_INVOKE__ = async (command, args) => {
      if (command === "extract_run") {
        const channel = args?.onProgress as { onmessage: (data: unknown) => void };
        channel.onmessage({ path: "stopped.json", completed: 1, total: 8, recipes: 2, active: 4, failed: 0, elapsedSeconds: 12, estimatedUsd: 0.02, reservedUsd: 0.50, unresolvedUsd: 0.48, activeModels: ["GLM 5.3 Flash (extracting)", "Gemini 2.5 Flash (verifying)"], stopping: false });
        return new Promise((resolve) => { finish = resolve; });
      }
      if (command === "cancel_extraction") {
        const result = await original("open_run", { path: "stopped.json" });
        finish?.({ ...(result as object), path: "stopped.json", incomplete: true, status: "cancelled" });
        return null;
      }
      return original(command, args);
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await openSource(page);
  await page.getByRole("button", { name: "Extraction…", exact: true }).click();
  await page.getByRole("button", { name: "Extract", exact: true }).click();
  await expect(page.locator(".progress")).toContainText("4 active");
  await expect(page.locator(".progress")).toContainText("12s");
  await expect(page.locator(".progress")).toContainText("$0.0200 estimated");
  await expect(page.locator(".progress")).toContainText("GLM 5.3 Flash (extracting)");
  await expect(page.locator(".progress")).toContainText("Gemini 2.5 Flash (verifying)");
  await expect(page.locator(".progress")).toContainText("$0.4800 reserved (not confirmed spend)");
  await page.getByRole("button", { name: "Stop extraction", exact: true }).click();
  await expect(page.locator(".run-heading")).toContainText("cancelled extraction");
  await expect(page.getByRole("button", { name: "Stop extraction", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Extraction…", exact: true }).click();
  await page.getByLabel("Resume this extraction").check();
  await expect(page.getByRole("button", { name: "Resume extraction", exact: true })).toBeEnabled();
});

test("source checks expose suspect method text and navigate to its source", async ({ page }) => {
  await page.addInitScript(() => {
    const host = window as unknown as { __FIXTURE_INVOKE__: (command: string, args?: Record<string, unknown>) => Promise<unknown> };
    const original = host.__FIXTURE_INVOKE__;
    host.__FIXTURE_INVOKE__ = async (command, args) => {
      const value = await original(command, args);
      if (command !== "open_run") return value;
      const book = value as { documents: { path: string }[] };
      return { ...book, feedback: { phase: "Incomplete", stopReason: "model stages exhausted", policy: ["GLM 5.3 Flash", "Gemini 2.5 Flash"], extractionUsd: 0.01, verificationUsd: 0.02, unresolvedUsd: 0, checks: [], findings: [{ category: "fidelity", source: book.documents[1].path, lines: [], model: "Gemini 2.5 Flash", resolved: false, message: "Possible method step stored in notes: Pour into molds and freeze until firm." }] } };
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await openExtraction(page);
  await page.locator(".source-checks summary").click();
  await expect(page.locator(".source-checks")).toContainText("Unresolved · fidelity");
  await expect(page.locator(".source-checks")).toContainText("Possible method step stored in notes");
  const source = await page.locator(".source-checks button").textContent();
  await page.locator(".source-checks button").click();
  await expect(page.locator(".document-heading")).toContainText(source ?? "");
});

test("source checks distinguish processing errors from content warnings", async ({ page }) => {
  await page.addInitScript(() => {
    const host = window as unknown as { __FIXTURE_INVOKE__: (command: string, args?: Record<string, unknown>) => Promise<unknown> };
    const original = host.__FIXTURE_INVOKE__;
    host.__FIXTURE_INVOKE__ = async (command, args) => {
      const value = await original(command, args);
      if (command !== "open_run") return value;
      const book = value as { documents: { path: string }[] };
      return { ...book, feedback: { phase: "Incomplete", stopReason: "verification budget exhausted", policy: ["GLM 5.3 Flash", "Gemini 2.5 Flash"], extractionUsd: 0.12, verificationUsd: 0.02, unresolvedUsd: 0.01, checks: ["Group 1 verified"], findings: [
        { category: "processing", source: book.documents[0].path, lines: [], model: "GLM 5.3 Flash", resolved: true, message: "request timeout: response deadline exceeded" },
        { category: "fidelity", source: book.documents[1].path, lines: [1], model: "Gemini 2.5 Flash", resolved: false, message: "Recipe has ingredients but no method." },
      ] } };
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await openExtraction(page);
  await page.locator(".source-checks summary").click();
  await expect(page.locator(".source-checks")).toContainText("Recovered · processing");
  await expect(page.locator(".source-checks")).toContainText("Unresolved · fidelity");
  await expect(page.locator(".source-checks")).toContainText("request timeout: response deadline exceeded");
});

test("model results distinguish partial coverage and open the underlying extraction", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Cookbooks", exact: true }).click();
  await page.getByRole("button", { name: "Model results", exact: true }).click();
  const table = page.getByRole("table", { name: "Processing success by book and model" });
  await expect(table).toContainText("100.0%");
  await expect(table).toContainText("2/10 complete");
  await expect(table).toContainText("8 pending");
  await expect(table).toContainText("Mixed: test-model / v8, parent-model / v7");
  await expect(table).toContainText("Unknown");
  await page.getByRole("searchbox", { name: "Filter books or models" }).fill("absent");
  await expect(page.getByText("No matching books or models.")).toBeVisible();
  await page.getByRole("searchbox", { name: "Filter books or models" }).fill("parent-model");
  await table.getByRole("button", { name: /Open Fixture cookbook/ }).click();
  await expect(page.getByRole("heading", { name: "Extracted result", exact: true })).toBeVisible();
});
