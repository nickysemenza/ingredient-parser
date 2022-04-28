import { useContext, useState } from "react";
import { WasmContext } from "./wasmContext";
import ReactJson from "react-json-view";
export const Demo: React.FC<{}> = ({}) => {
  const w = useContext(WasmContext);

  const [text, setText] = useState("1 cup / 120 grams flour, sifted");
  const parsed = w?.parse_ingredient(text);
  if (!w) {
    return null;
  }
  return (
    <div>
      <section className="w-full px-8 py-16 bg-gray-100 xl:px-8">
        <div className="max-w-5xl mx-auto">
          <div className="flex flex-col items-center md:flex-row">
            <div className="w-full space-y-5 md:w-3/5 md:pr-16">
              <h2 className="text-2xl font-extrabold leading-none text-black sm:text-3xl md:text-5xl">
                ingredient-parser
              </h2>
              <p className="text-xl text-gray-600 md:pr-16">
                Type in an ingredient and see the result below. This demo runs
                using WASM!
              </p>
              <Debug data={parsed} compact />
            </div>

            <div className="w-full mt-16 md:mt-0 md:w-2/5">
              <div className="relative z-10 h-auto p-8 py-10 overflow-hidden bg-white border-b-2 border-gray-300 rounded-lg shadow-2xl px-7">
                <h3 className="mb-6 text-2xl font-medium text-center">
                  Try it out!
                </h3>
                <input
                  type="text"
                  name="email"
                  className="block w-full px-4 py-3 mb-4 border-2 border-transparent border-gray-200 rounded-lg focus:ring focus:ring-blue-500 focus:outline-none"
                  placeholder="Email address"
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                />
              </div>
            </div>
          </div>
        </div>
      </section>

      <section className="text-gray-700 bg-white body-font">
        <div className="container flex flex-col items-center px-8 py-8 mx-auto max-w-7xl sm:flex-row">
          <p className="mt-4 text-sm text-gray-500 sm:ml-4 sm:px-4 sm:border-r sm:border-gray-200 sm:mt-0">
            © 2021 Nicky Semenza
          </p>

          <span className="inline-flex justify-center mt-4 space-x-5 sm:ml-auto sm:mt-0 sm:justify-start">
            <a
              href="https://github.com/nickysemenza/ingredient-parser"
              target="_blank"
              rel="noreferrer"
            >
              <img
                alt="GitHub Repo stars"
                src="https://img.shields.io/github/stars/nickysemenza/ingredient-parser?style=social"
              />
            </a>
            <a
              href="https://crates.io/crates/ingredient"
              target="_blank"
              rel="noreferrer"
            >
              <img
                alt="Crates.io"
                src="https://img.shields.io/crates/v/ingredient"
              />
            </a>
            <a
              href="https://crates.io/crates/ingredient"
              target="_blank"
              rel="noreferrer"
            >
              <img alt="docs.rs" src="https://docs.rs/ingredient/badge.svg" />
            </a>
          </span>
        </div>
      </section>
    </div>
  );
};

const Debug: React.FC<{ data: any; compact?: boolean }> = ({ data, compact }) =>
  !compact ? (
    <ReactJson src={data} theme="monokai" />
  ) : (
    <div className="rounded-md bg-gray-800 dark:bg-gray-300 text-purple-300 text-xs">
      <pre className="scrollbar-none m-0 p-0">
        <code className="inline-block w-auto p-4 scrolling-touch">
          {JSON.stringify(data, null, 2)}
        </code>
      </pre>
    </div>
  );

export default Debug;
