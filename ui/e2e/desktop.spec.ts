// The desktop shell, end to end, in WebKit. Native-only commands (library,
// run store, models, export) are answered from a fixture recorded by
// `cargo run -p food-app --example create_run_fixture -- $UI_QA_DIR`; every
// portable command (parse, inspect, open book/run, images, source) runs the
// real Rust in WASM against that fixture's files.
import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

const QA = process.env.UI_QA_DIR ?? "/tmp/food-app-qa";
const fixture = JSON.parse(readFileSync(`${QA}/frontend-fixture.json`, "utf8"));
interface FixtureItem {
  kind: string;
  id: string;
  title: string;
  photos: unknown[];
  sections?: { ingredients: { raw: string; ref: unknown }[]; steps: { text: string }[] }[];
}
const extraction = fixture.extraction;
const bookTitle: string = extraction.cookbook.source.title;
const items: FixtureItem[] = extraction.cookbook.chapters.flatMap((c: { items: FixtureItem[] }) => c.items);
const recipe = items.find((i) => i.kind === "recipe")!;
const ingredient = recipe.sections![0].ingredients[0].raw;
const savedRun = fixture.runs[0];
const book = fixture.library[0];
const shot = (page: Page, name: string) => page.screenshot({ path: `test-results/desktop-${name}.png` });

type Call = { command: string; args: Record<string, unknown> };
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls?: Call[] }).__calls ?? []);

test.beforeEach(async ({ page }) => {
  await page.addInitScript((data) => {
    const host = window as unknown as {
      __calls: Call[];
      __holdExtraction?: boolean;
      __finishExtraction?: () => void;
      __gatewayUnconfigured?: boolean;
    } & Window;
    host.__FIXTURE_FILES__ = data.files;
    host.__FIXTURE_INVOKE__ = async (command, args = {}) => {
      (host.__calls ??= []).push({ command, args });
      switch (command) {
        case "reveal_file":
        case "open_source_url":
        case "cancel_extraction":
        case "delete_run":
          return null;
        case "dialog_open":
          return args.kind === "directory" ? "fixture-library" : data.library[0].path;
        case "fetch_html":
          return data.recipeHtml;
        case "scan_library":
          return data.library;
        case "list_runs":
          return data.runs;
        case "estimate_book":
          return data.estimate;
        case "backend_statuses":
          return [];
        case "catalog_status":
          return null;
        case "gateway_status":
          return host.__gatewayUnconfigured
            ? { ...data.gateway, configured: false, error: "CLOUDFLARE_AI_GATEWAY_BASE_URL is unset" }
            : { ...data.gateway, configured: true, error: null };
        case "export_bundle":
          return data.bundle;
        case "extract_book": {
          const channel = args.onProgress as { onmessage: (value: unknown) => void };
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
            eta: { remaining_low_ms: 8000, remaining_high_ms: 64000, projected_cost_usd: 0.0029 },
            active_models: [data.estimate.ladder[0]],
          });
          if (host.__holdExtraction) await new Promise<void>((resolve) => (host.__finishExtraction = resolve));
          return data.runs[0];
        }
      }
      throw new Error(`Unimplemented fixture command: ${command}`);
    };
  }, fixture);
});

async function openLibrary(page: Page) {
  await page.goto("/");
  await page.getByRole("navigation", { name: "Workspaces" }).getByRole("button", { name: "Cookbooks" }).click();
  await expect(page.getByRole("region", { name: "Cookbook library" })).toContainText(book.title);
}

async function openSavedRun(page: Page) {
  await openLibrary(page);
  await page.getByRole("region", { name: "Run history" }).getByRole("button", { name: /^Open run of/ }).first().click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(bookTitle);
}

test("the parser workspace parses and inspects with native-shaped results", async ({ page }) => {
  await page.goto("/");
  // No landing page on the desktop; the parser opens directly.
  await expect(page.getByRole("heading", { level: 1, name: "Parser" })).toBeVisible();
  await page.getByRole("textbox", { name: "Ingredient lines" }).fill(`${ingredient}\n1 cup (240 g) water`);
  await expect(page.getByRole("listbox", { name: "Parsed ingredients" }).getByRole("option")).toHaveCount(2);
  await expect(page.getByRole("region", { name: "Ingredient inspector" })).toContainText(ingredient);
  // Shell state survives a relaunch.
  await page.getByRole("radio", { name: "dark appearance" }).click();
  await page.getByRole("navigation", { name: "Workspaces" }).getByRole("button", { name: "Cookbooks" }).click();
  await page.reload();
  await expect(page.getByRole("heading", { level: 1, name: "Cookbooks" })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await shot(page, "library-dark");
});

test("a web recipe loads through the native fetcher and scales", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Recipe" }).click();
  await page.getByRole("textbox", { name: "Recipe URL" }).fill(fixture.recipeUrl);
  await page.getByRole("button", { name: "Load recipe" }).click();
  const article = page.getByRole("article", { name: "Recipe" });
  await expect(article).toContainText("Weeknight soup");
  await expect(article).toContainText("1 cup (240 g) water");
  await page.getByRole("button", { name: "2×" }).click();
  await expect(article.getByRole("button", { name: /water/ })).toContainText("2 cups");
  const fetched = (await calls(page)).find((c) => c.command === "fetch_html");
  expect(fetched?.args.url).toBe(fixture.recipeUrl);
  await shot(page, "recipe");
});

test("opening a book shows its estimate, and extraction reports progress then opens the run", async ({ page }) => {
  await openLibrary(page);
  await page.getByRole("button", { name: `Open ${book.title}` }).click();
  await expect(page.getByRole("region", { name: "Extraction estimate" })).toContainText(fixture.estimate.assumptions[0]);
  await expect(page.getByRole("region", { name: "Cookbook classification" })).toBeVisible();
  await shot(page, "book");
  await page.evaluate(() => ((window as unknown as { __holdExtraction: boolean }).__holdExtraction = true));
  await page.getByRole("button", { name: "Extract", exact: true }).click();
  const progress = page.getByRole("status", { name: "Extraction progress" });
  await expect(progress).toContainText("1/4 chunks");
  await expect(progress).toContainText("$0.0011");
  await expect(progress).toContainText("~8.0 s–1 m 4 s left");
  await expect(page.getByLabel("Workspace status")).toContainText("Extracting 1/4");
  await shot(page, "extracting");
  await page.evaluate(() => (window as unknown as { __finishExtraction: () => void }).__finishExtraction());
  await expect(page.getByRole("region", { name: "Book contents" })).toContainText(recipe.title);
  const recorded = await calls(page);
  expect(recorded.find((c) => c.command === "extract_book")?.args.path).toBe(book.path);
  expect(recorded.find((c) => c.command === "scan_library")?.args.directory).toBe("");
});

test("an unconfigured gateway blocks extraction and names its config file", async ({ page }) => {
  await page.addInitScript(() => ((window as unknown as { __gatewayUnconfigured: boolean }).__gatewayUnconfigured = true));
  await openLibrary(page);
  await page.getByRole("button", { name: `Open ${book.title}` }).click();
  await expect(page.getByRole("region", { name: "Extraction", exact: true })).toContainText("CLOUDFLARE_AI_GATEWAY_BASE_URL is unset");
  await expect(page.getByRole("button", { name: "Extract", exact: true })).toBeDisabled();
});

test("a saved run: items, photos, inspector, references, source and diagnostics", async ({ page }) => {
  await openSavedRun(page);
  const tree = page.getByRole("region", { name: "Book contents" });
  await tree.getByRole("button", { name: new RegExp(recipe.title) }).click();
  await expect(page.getByRole("heading", { level: 2, name: recipe.title })).toBeVisible();
  await expect(page.getByRole("article", { name: recipe.title })).toContainText(recipe.sections![0].steps[0].text);
  await expect(page.getByRole("article", { name: recipe.title }).locator('img[src^="data:image/"]')).toHaveCount(recipe.photos.length);
  await page.getByRole("article", { name: recipe.title }).getByRole("button", { name: new RegExp(escape(ingredient)) }).first().click();
  await expect(page.getByRole("region", { name: "Ingredient inspector" })).toContainText(ingredient);
  await shot(page, "run");
  await page.getByRole("button", { name: "Close inspector" }).click();
  await expect(page.getByRole("region", { name: "Ingredient inspector" })).toHaveCount(0);

  await page.getByRole("tab", { name: "Source" }).click();
  await expect(page.getByLabel("Source lines")).toContainText(recipe.title);
  await page.getByRole("tab", { name: "Extracted" }).click();

  const linked = items.find((i) => i.kind === "recipe" && i.sections?.some((s) => s.ingredients.some((l) => l.ref)));
  if (linked) {
    await tree.getByRole("button", { name: new RegExp(linked.title) }).click();
    const chip = page.getByRole("article", { name: linked.title }).locator("button.rounded-full").first();
    const target = (await chip.innerText()).trim();
    await chip.click();
    await expect(page.getByRole("heading", { level: 2, name: target })).toBeVisible();
  }

  await page.getByRole("tab", { name: "Diagnostics" }).click();
  await expect(page.getByRole("region", { name: "Run summary" })).toContainText(extraction.report.run_id);
  await expect(page.getByRole("region", { name: "Chunks" })).toContainText(extraction.report.chunks[0].id);
  await expect(page.getByRole("listbox", { name: "Model calls" }).getByRole("option").first()).toContainText(extraction.report.usage_by_model[0].model);
  await shot(page, "diagnostics");
  await page.getByRole("tab", { name: "Recipes" }).click();
});

test("export and delete go through the native commands", async ({ page }) => {
  page.on("dialog", (dialog) => void dialog.accept());
  await openSavedRun(page);
  await page.getByRole("button", { name: "Export bundle" }).click();
  await expect(page.getByText(fixture.bundle.path)).toBeVisible();
  await page.getByRole("button", { name: "Back to library" }).click();
  await page.getByRole("region", { name: "Run history" }).getByRole("button", { name: "Delete run" }).first().click();
  await expect.poll(async () => (await calls(page)).find((c) => c.command === "delete_run")?.args.path).toBe(savedRun.path);
  const exported = (await calls(page)).find((c) => c.command === "export_bundle");
  expect(exported?.args).toEqual({ run: savedRun.path, book: book.path });
  expect((await calls(page)).some((c) => c.command === "extract_book")).toBe(false);
});

test("the command palette navigates and opens a book", async ({ page }) => {
  await page.goto("/");
  await page.keyboard.press("Meta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("cookbooks");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { level: 1, name: "Cookbooks" })).toBeVisible();
  await page.keyboard.press("Meta+k");
  await palette.getByRole("combobox").fill("open epub");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(bookTitle);
  await page.keyboard.press("Meta+1");
  await expect(page.getByRole("heading", { level: 1, name: "Parser" })).toBeVisible();
});

function escape(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
