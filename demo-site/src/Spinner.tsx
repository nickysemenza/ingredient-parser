/* WASM-free module so main.tsx can render the loading screen before the WASM
   chunk is fetched. */

export const Spinner: React.FC = () => (
  <svg
    className="h-5 w-5 animate-spin"
    viewBox="0 0 24 24"
    fill="none"
    aria-hidden="true"
  >
    <circle
      className="opacity-25"
      cx="12"
      cy="12"
      r="10"
      stroke="currentColor"
      strokeWidth="4"
    />
    <path
      className="opacity-75"
      fill="currentColor"
      d="M4 12a8 8 0 0 1 8-8v4a4 4 0 0 0-4 4H4z"
    />
  </svg>
);

export const LoadingScreen: React.FC = () => (
  <div className="flex min-h-screen items-center justify-center text-zinc-400">
    <Spinner />
    <span className="ml-3 text-sm">Loading parser…</span>
  </div>
);
