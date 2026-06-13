/* Shared UI primitives — consolidate the styling repeated across the demo's
   inputs so the accent focus ring is defined once. */

// Accent focus ring shared by every text/number input on the page.
export const FOCUS_RING =
  "focus:border-accent-400 focus:ring-2 focus:ring-accent-200 focus:outline-none";

// Standard demo text input (ingredient line, rich-text line). The scraper's
// URL field and the scale number field are styled inline since they differ in
// size/shape, but they reuse FOCUS_RING.
export const TextInput: React.FC<React.InputHTMLAttributes<HTMLInputElement>> = ({
  className = "",
  ...props
}) => (
  <input
    type="text"
    className={`w-full rounded-lg border border-zinc-300 bg-white px-4 py-3 text-base transition ${FOCUS_RING} ${className}`}
    {...props}
  />
);
