import { useCallback, useContext, useEffect, useState } from "react";
import { RichItem, wasm, WasmContext, ScrapedRecipe } from "./wasmContext";
import ReactJson from "react-json-view";
export const Demo: React.FC = () => {
  const w = useContext(WasmContext);

  const [text, setText] = useState("1 cup / 120 grams flour, sifted");
  const parsed = w?.parse_ingredient(text);

  const exampleIngredients = [
    "2 cups all-purpose flour, sifted",
    "1/2 cup butter, softened", 
    "3 large eggs, beaten",
    "1 tsp vanilla extract",
    "2 tbsp olive oil, extra virgin"
  ];

  const [richText, setRichText] = useState(
    "Add 1/2 cup / 236 grams water to the bowl with the salt and mix."
  );
  const ingredientNames = ["flour", "water", "salt"];
  const parsedRich = w?.parse_rich_text(richText, ingredientNames);

  if (!w) {
    return null;
  }
  return (
    <div className="min-h-screen">
      <section className="w-full px-6 py-12 gradient-hero xl:px-6 relative overflow-hidden">
        <div className="absolute inset-0 bg-black opacity-10"></div>
        <div className="relative z-10 max-w-4xl mx-auto">
          <div className="text-center mb-10">
            <h1 className="text-3xl md:text-5xl lg:text-6xl font-extrabold leading-tight text-white mb-4">
              <span className="bg-gradient-to-r from-white to-purple-200 bg-clip-text text-transparent">
                ingredient-parser
              </span>
            </h1>
            <p className="text-lg md:text-xl text-purple-100 max-w-2xl mx-auto leading-relaxed">
              Parse recipe ingredients into structured data.
              <br className="hidden md:block"/>
              <span className="font-semibold text-white">Built with Rust + WASM</span>
            </p>
          </div>
          <div className="flex justify-center">
            <div className="w-full max-w-xl">
              <div className="glass rounded-xl p-6 shadow-xl backdrop-blur-lg border border-white border-opacity-30 animate-fade-in-up hover-lift transition-all">
                <h3 className="mb-4 text-xl font-semibold text-center text-white">
                  ✨ Try it out!
                </h3>
                <div className="space-y-4">
                  <div className="relative">
                    <input
                      type="text"
                      placeholder="Enter an ingredient (e.g., '2 cups flour, sifted')"
                      className="w-full px-4 py-3 text-base rounded-lg border border-white border-opacity-30 bg-white bg-opacity-90 backdrop-blur-sm focus:bg-white focus:ring-2 focus:ring-purple-300 focus:ring-opacity-50 focus:outline-none transition-all duration-300 placeholder-gray-500"
                      value={text}
                      onChange={(e) => setText(e.target.value)}
                    />
                  </div>
                  
                  <div className="text-center">
                    <p className="text-white text-xs mb-2 opacity-80">Try these examples:</p>
                    <div className="flex flex-wrap gap-1 justify-center">
                      {exampleIngredients.map((ingredient, index) => (
                        <button
                          key={index}
                          onClick={() => setText(ingredient)}
                          className="px-2 py-1 text-xs bg-purple-600 bg-opacity-70 hover:bg-opacity-90 text-white rounded-md transition-all duration-200 hover:scale-105 backdrop-blur-sm border border-purple-300 shadow-sm"
                        >
                          {ingredient}
                        </button>
                      ))}
                    </div>
                  </div>
                  
                  <div className="bg-white bg-opacity-90 backdrop-blur-sm rounded-lg p-3">
                    <Debug data={parsed} compact />
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section className="w-full px-6 py-12 bg-gradient-to-br from-gray-50 to-white">
        <div className="max-w-5xl mx-auto">
          <div className="mb-12">
            <div className="text-center mb-8">
              <h2 className="text-2xl md:text-4xl font-extrabold bg-gradient-to-r from-purple-600 to-blue-600 bg-clip-text text-transparent mb-3">
                🌐 Recipe Scraper
              </h2>
              <p className="text-lg text-gray-600 max-w-2xl mx-auto">
                Extract structured ingredient data from any recipe URL
              </p>
            </div>
            <div className="gradient-card rounded-xl p-6 shadow-lg border border-gray-100">
              <Scraper />
            </div>
          </div>
          <div className="mb-12">
            <div className="text-center mb-8">
              <h2 className="text-2xl md:text-4xl font-extrabold bg-gradient-to-r from-green-600 to-teal-600 bg-clip-text text-transparent mb-3">
                📝 Rich Text Parser
              </h2>
              <p className="text-lg text-gray-600 max-w-2xl mx-auto mb-4">
                Parse ingredient names and amounts from freeform recipe instructions.
              </p>
              <div className="flex justify-center">
                <div className="bg-gray-50 rounded-lg p-3 border border-gray-200">
                  <Debug data={{ ingredientNames: ingredientNames }} compact />
                </div>
              </div>
            </div>
            <div className="gradient-card rounded-xl p-6 shadow-lg border border-gray-100">
              <div className="space-y-4">
                <input
                  type="text"
                  placeholder="Enter recipe instructions..."
                  className="w-full px-4 py-3 text-base rounded-lg border border-gray-300 focus:ring-2 focus:ring-green-300 focus:ring-opacity-50 focus:border-green-400 focus:outline-none transition-all duration-300"
                  value={richText}
                  onChange={(e) => setRichText(e.target.value)}
                />
                <div className="bg-white rounded-lg p-4 border border-gray-200 min-h-[80px]">
                  <div className="text-base leading-relaxed">
                    {w && parsedRich && formatRichText(w, parsedRich)}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section className="bg-gradient-to-r from-gray-900 to-black text-white">
        <div className="max-w-5xl mx-auto px-6 py-8">
          <div className="flex flex-col md:flex-row items-center justify-between">
            <div className="mb-6 md:mb-0">
              <h3 className="text-2xl font-bold bg-gradient-to-r from-purple-400 to-blue-400 bg-clip-text text-transparent">
                ingredient-parser
              </h3>
              <p className="text-gray-400 mt-2">
                © {new Date().getFullYear()} Nicky Semenza
              </p>
            </div>
            
            <div className="flex flex-wrap items-center gap-4">
              <a
                href="https://github.com/nickysemenza/ingredient-parser"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center px-4 py-2 bg-white bg-opacity-10 hover:bg-opacity-20 rounded-lg transition-all duration-300 backdrop-blur-sm"
              >
                <span className="mr-2">⭐</span>
                <img
                  alt="GitHub Repo stars"
                  src="https://img.shields.io/github/stars/nickysemenza/ingredient-parser?style=social"
                  className="filter invert"
                />
              </a>
              <a
                href="https://crates.io/crates/ingredient"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center px-4 py-2 bg-white bg-opacity-10 hover:bg-opacity-20 rounded-lg transition-all duration-300 backdrop-blur-sm"
              >
                <span className="mr-2">📦</span>
                <img
                  alt="Crates.io"
                  src="https://img.shields.io/crates/v/ingredient?style=flat&color=white"
                />
              </a>
              <a
                href="https://docs.rs/ingredient"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center px-4 py-2 bg-white bg-opacity-10 hover:bg-opacity-20 rounded-lg transition-all duration-300 backdrop-blur-sm"
              >
                <span className="mr-2">📚</span>
                <img 
                  alt="docs.rs" 
                  src="https://docs.rs/ingredient/badge.svg" 
                />
              </a>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
};

const Scraper: React.FC = () => {
  const w = useContext(WasmContext);
  const [scrapedRecipe, setRecipe] = useState<ScrapedRecipe | undefined>(
    undefined
  );
  const [url, setURL] = useState(
    "https://cooking.nytimes.com/recipes/1020830-caramelized-shallot-pasta"
  );

  const doScrape = useCallback(async () => {
    let res = await fetch("https://cors.nicky.workers.dev/?target=" + url);
    let body = await res.text();
    w && setRecipe(w.scrape(body, url));
  }, [w, url]);

  useEffect(() => {
    doScrape();
  }, [w, url, doScrape]);

  const ingredientNames =
    scrapedRecipe && w
      ? scrapedRecipe.ingredients.map((i) => w.parse_ingredient(i).name)
      : [];

  return (
    <div className="space-y-8">
      <div className="flex flex-col lg:flex-row gap-8">
        <div className="flex-1">
          <div className="space-y-4">
            <input
              type="text"
              placeholder="Enter recipe URL (e.g., from NYTimes Cooking)"
              className="w-full px-6 py-4 text-lg rounded-xl border border-gray-300 focus:ring-4 focus:ring-blue-300 focus:ring-opacity-50 focus:border-blue-400 focus:outline-none transition-all duration-300"
              value={url}
              onChange={(e) => setURL(e.target.value)}
            />
            {scrapedRecipe && (
              <div className="bg-gradient-to-r from-blue-50 to-purple-50 rounded-xl p-6">
                <h3 className="text-2xl font-bold text-gray-800 mb-2">
                  {scrapedRecipe.name}
                </h3>
                <div className="flex items-center text-gray-600">
                  <span className="mr-2">🍽️</span>
                  <span>Recipe successfully scraped!</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="lg:w-1/3">
          {scrapedRecipe && (
            <img
              className="w-full h-64 lg:h-48 object-cover rounded-xl shadow-lg"
              src={scrapedRecipe.image}
              alt={scrapedRecipe.name}
            />
          )}
        </div>
      </div>

      {scrapedRecipe && (
        <div className="grid md:grid-cols-2 gap-8">
          <div className="bg-white rounded-2xl p-6 shadow-lg border border-gray-100">
            <h4 className="text-2xl font-bold text-gray-800 mb-6 flex items-center">
              <span className="mr-2">🧂</span>
              Ingredients
            </h4>
            <div className="space-y-3">
              {w &&
                scrapedRecipe.ingredients.map((i, index) => {
                  const p = w.parse_ingredient(i);
                  return (
                    <div 
                      key={index}
                      className="flex items-center justify-between py-3 px-4 bg-gray-50 rounded-xl hover:bg-gray-100 transition-colors duration-200"
                    >
                      <div className="flex-1">
                        <div className="font-semibold text-gray-800 underline decoration-purple-400 decoration-2">
                          {p.name}
                        </div>
                        {p.modifier && (
                          <div className="text-sm italic text-gray-600 mt-1">
                            {p.modifier}
                          </div>
                        )}
                      </div>
                      <div className="text-right font-medium text-purple-600 ml-4">
                        {p.amounts
                          .filter((a) => a.unit !== "$" && a.unit !== "kcal")
                          .map((a) => w.format_amount(a))
                          .join(" / ")}
                      </div>
                    </div>
                  );
                })}
            </div>
          </div>
          
          <div className="bg-white rounded-2xl p-6 shadow-lg border border-gray-100">
            <h4 className="text-2xl font-bold text-gray-800 mb-6 flex items-center">
              <span className="mr-2">👩‍🍳</span>
              Instructions
            </h4>
            <ol className="space-y-4">
              {w &&
                scrapedRecipe.instructions.map((instruction, index) => (
                  <li 
                    key={index}
                    className="flex items-start p-4 bg-gray-50 rounded-xl hover:bg-gray-100 transition-colors duration-200"
                  >
                    <div className="flex-shrink-0 w-8 h-8 bg-gradient-to-r from-blue-500 to-purple-500 text-white rounded-full flex items-center justify-center text-sm font-bold mr-4">
                      {index + 1}
                    </div>
                    <div className="flex-1 text-gray-700 leading-relaxed">
                      {formatRichText(w, w.parse_rich_text(instruction, ingredientNames))}
                    </div>
                  </li>
                ))}
            </ol>
          </div>
        </div>
      )}
    </div>
  );
};

const Debug: React.FC<{ data: any; compact?: boolean }> = ({ data, compact }) =>
  !compact ? (
    <div className="bg-gray-900 rounded-xl p-4 overflow-auto">
      <ReactJson 
        src={data} 
        theme="monokai" 
        displayDataTypes={false}
        displayObjectSize={false}
        collapsed={1}
      />
    </div>
  ) : (
    <div className="rounded-xl bg-gradient-to-r from-gray-800 to-gray-900 text-green-300 text-sm shadow-lg">
      <div className="flex items-center px-4 py-2 bg-gray-700 rounded-t-xl">
        <span className="text-purple-300 font-mono text-xs">JSON Output</span>
      </div>
      <pre className="p-4 overflow-auto scrollbar-thin scrollbar-thumb-gray-600 scrollbar-track-gray-800">
        <code className="text-green-300 font-mono text-sm leading-relaxed">
          {JSON.stringify(data, null, 2)}
        </code>
      </pre>
    </div>
  );

export const formatRichText = (w: wasm, text: RichItem[]) => {
  return text.map((t, x) => {
    if (t.kind === "Text") {
      return t.value;
    } else if (t.kind === "Ing") {
      return (
        <span
          className="inline px-2 py-1 bg-orange-100 text-orange-800 rounded-md font-semibold mx-1 border border-orange-200"
          key={x + "a"}
        >
          {t.value}
        </span>
      );
    } else if (t.kind === "Measure") {
      let val = t.value.pop();
      if (!val) {
        return null;
      }
      if (val.unit === "whole") {
        val.unit = "";
      }
      return (
        <span
          className="inline px-2 py-1 bg-green-100 text-green-800 rounded-md font-semibold mx-1 border border-green-200"
          key={x}
        >
          {w.format_amount(val)}
        </span>
      );
    } else {
      return null;
    }
  });
};
