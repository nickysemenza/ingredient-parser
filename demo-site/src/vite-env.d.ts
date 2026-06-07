/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Base URL of the CORS proxy used by the recipe scraper. The target
   *  URL is appended as a query param (e.g. `?target=`). */
  readonly VITE_CORS_PROXY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
