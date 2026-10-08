// The public site, end to end, against the real WASM build. Expected values
// come from the inputs themselves (the authored line, the corpus file), not
// from the implementation.
import { expect, test, type Page } from "@playwright/test";
import { existsSync, readFileSync } from "node:fs";

const QA = process.env.UI_QA_DIR ?? "/tmp/food-app-qa";
const shot = (page: Page, name: string) => page.screenshot({ path: `test-results/web-${name}.png` });

test("the landing page parses a line live and shows where each part went", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto("/");
  const decomposition = page.getByTestId("hero-decomposition");
  await expect(decomposition).toHaveText("1 cup / 120 grams flour, sifted");
  const result = page.getByRole("definition");
  await expect(page.getByLabel("Parsed ingredient")).toContainText("flour");
  await expect(decomposition.locator('[data-field="name"]')).toHaveText("flour");
  await expect(decomposition.locator('[data-field="modifier"]')).toHaveText("sifted");
  await expect(result.filter({ hasText: "1 cup" }).first()).toBeVisible();
  await shot(page, "landing");

  const input = page.getByRole("textbox", { name: "Ingredient line" });
  await input.fill("2-3 cloves garlic, minced");
  await expect(decomposition).toHaveText("2-3 cloves garlic, minced");
  await expect(decomposition.locator('[data-field="name"]')).toHaveText("garlic");
  await expect(decomposition.locator('[data-field="modifier"]')).toHaveText("minced");
  // The authored line round-trips through the segments, character for character.
  await input.fill("Kosher salt and freshly ground pepper, to taste");
  await expect(decomposition).toHaveText("Kosher salt and freshly ground pepper, to taste");

  await expect(page.getByRole("region", { name: "How it reads a line" })).toContainText("Grammar");
  await expect(page.getByTestId("rich-text").locator('[data-kind="measure"]').first()).toBeVisible();
  await expect(page.getByTestId("rich-text").locator('[data-kind="ingredient"]', { hasText: "flour" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("the workbench parses many lines, inspects one, and restores a shared link", async ({ page }) => {
  const lines = ["2 cups all-purpose flour, sifted", "3 large eggs, beaten", "oil, for frying"];
  await page.goto("/");
  await page.getByRole("link", { name: "Open workbench" }).click();
  await expect(page).toHaveURL(/\/parser/);
  const editor = page.getByRole("textbox", { name: "Ingredient lines" });
  await editor.fill(lines.join("\n"));
  const table = page.getByRole("listbox", { name: "Parsed ingredients" });
  await expect(table.getByRole("option")).toHaveCount(3);
  for (const line of lines) await expect(table).toContainText(line);
  await table.getByRole("option").nth(1).click();
  const inspector = page.getByRole("region", { name: "Ingredient inspector" });
  await expect(inspector).toContainText("3 large eggs, beaten");
  await inspector.getByRole("tab", { name: "Stages" }).click();
  await expect(inspector).toContainText("Normalize");
  await inspector.getByRole("tab", { name: "Trace" }).click();
  await expect(inspector.getByRole("button").filter({ hasText: /\S/ }).first()).toBeVisible();
  await inspector.getByRole("tab", { name: "JSON" }).click();
  await expect(inspector).toContainText('"name": "eggs"');
  await shot(page, "parser");

  // The URL carries the lines; a fresh visit restores them.
  const url = page.url();
  expect(new URL(url).searchParams.get("q")).toBe(lines.join("\n"));
  const fresh = await page.context().newPage();
  await fresh.goto(url);
  await expect(fresh.getByRole("listbox", { name: "Parsed ingredients" }).getByRole("option")).toHaveCount(3);
  await fresh.close();

  // Back returns to the landing page.
  await page.goBack();
  await expect(page.getByTestId("hero-decomposition")).toBeVisible();
});

test("the corpus scores in the browser, built in or uploaded", async ({ page }) => {
  await page.goto("/parser?mode=corpus");
  await page.getByRole("button", { name: "Score corpus" }).click();
  const rows = page.getByRole("listbox", { name: "Corpus rows" });
  await expect(rows.getByRole("option").first()).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole("table", { name: "Field comparison" })).toBeVisible();

  const corpus = `${QA}/corpus.jsonl`;
  test.skip(!existsSync(corpus), "run create_run_fixture first");
  const cases = readFileSync(corpus, "utf8").split("\n").filter((l) => l.trim().startsWith("{")).length;
  await page.getByLabel("Corpus file").setInputFiles(corpus);
  await expect(page.getByLabel("Workspace status")).toContainText(`${cases} cases scored`);
  await page.getByRole("textbox", { name: "Find corpus input" }).fill("fixture flour");
  await expect(rows.getByRole("option")).toHaveCount(1);
  await rows.getByRole("option").first().click();
  await expect(page.getByRole("table", { name: "Field comparison" })).toContainText("intentionally mismatched label");
  await page.getByRole("tab", { name: "Inspect" }).click();
  await expect(page.getByRole("region", { name: "Ingredient inspector" })).toContainText("1 cup fixture flour");
  await shot(page, "corpus");
});

test("a dropped EPUB and its run open offline, with photos and source lines", async ({ page }) => {
  const fixture = `${QA}/frontend-fixture.json`;
  test.skip(!existsSync(fixture), "run create_run_fixture first");
  const data = JSON.parse(readFileSync(fixture, "utf8"));
  const title: string = data.extraction.cookbook.source.title;
  await page.goto("/cookbooks");
  await expect(page.getByRole("button", { name: "Export bundle" })).toHaveCount(0);

  await page.getByLabel("EPUB or run files", { exact: true }).setInputFiles(`${QA}/cookbook.epub`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(title);
  await expect(page.getByRole("region", { name: "Extraction", exact: true })).toContainText("desktop app");
  await expect(page.getByRole("button", { name: /^Extract$/ })).toHaveCount(0);
  await shot(page, "book");

  await page.getByRole("button", { name: "Back to opened files" }).click();
  await page.getByLabel("EPUB or run files", { exact: true }).setInputFiles(data.runs[0].path);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(title);
  const tree = page.getByRole("region", { name: "Book contents" });
  const recipe = data.extraction.cookbook.chapters.flatMap((c: { items: { kind: string; title: string }[] }) => c.items as { kind: string; title: string; photos: unknown[] }[]).find((i: { kind: string; photos: unknown[] }) => i.kind === "recipe" && i.photos.length > 0);
  await tree.getByRole("button", { name: new RegExp(recipe.title) }).click();
  await expect(page.getByRole("heading", { level: 2, name: recipe.title })).toBeVisible();
  // The photo is read from the dropped EPUB by the worker.
  await expect(page.getByRole("article", { name: recipe.title }).locator('img[src^="data:image/"]')).toHaveCount(recipe.photos.length);
  await page.getByRole("tab", { name: "Source" }).click();
  await expect(page.getByLabel("Source lines")).toContainText(recipe.title);
  await shot(page, "run");
});
