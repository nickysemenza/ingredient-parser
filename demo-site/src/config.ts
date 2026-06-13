/* Demo constants — the knobs that tune the page without touching component
   logic. Kept in one place so example data and the CORS proxy aren't buried
   inside JSX. */

export const CORS_PROXY =
  import.meta.env.VITE_CORS_PROXY ?? "https://cors.nicky.workers.dev/?target=";

export const EXAMPLE_INGREDIENTS = [
  "2 cups all-purpose flour, sifted",
  "1/2 cup butter, softened",
  "3 large eggs, beaten",
  "1 tsp vanilla extract",
  "2 tbsp olive oil, extra virgin",
];

// The names the rich-text parser highlights in the demo instruction line.
export const DEMO_INGREDIENT_NAMES = ["flour", "water", "salt"];

// Scale presets offered in the scraper. 0.5 renders as ½ (see Scraper).
export const SCALE_OPTIONS = [0.5, 1, 2, 3];

export const DEFAULT_INGREDIENT = "1 cup / 120 grams flour, sifted";
export const DEFAULT_RICH_TEXT =
  "Add 1/2 cup / 236 grams water to the bowl with the salt and mix.";
export const DEFAULT_SCRAPE_URL =
  "https://cooking.nytimes.com/recipes/1020830-caramelized-shallot-pasta";
