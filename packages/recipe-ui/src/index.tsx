/** Presentation only: callers perform scaling through native Rust or WASM. */
export function RecipeScale({
  value,
  onChange,
  label = "Recipe scale",
  disabled = false,
  presets = [0.5, 1, 2, 3, 4],
  custom = false,
  className,
  buttonClassName,
  activeButtonClassName,
  inputClassName,
}: {
  value: number;
  onChange: (value: number) => void;
  label?: string;
  disabled?: boolean;
  presets?: readonly number[];
  custom?: boolean;
  className?: string;
  buttonClassName?: string;
  activeButtonClassName?: string;
  inputClassName?: string;
}) {
  const change = (next: number) => {
    if (Number.isFinite(next) && next > 0) onChange(next);
  };
  return custom ? (
    <div className={className} role="group" aria-label={label}>
      <span>Scale:</span>
      {presets.map((scale) => (
        <button
          key={scale}
          type="button"
          disabled={disabled}
          aria-pressed={value === scale}
          className={value === scale ? activeButtonClassName : buttonClassName}
          onClick={() => change(scale)}
        >
          {scale === 0.5 ? "½" : scale}x
        </button>
      ))}
      <input
        type="number"
        aria-label={label}
        min="0.1"
        step="0.1"
        className={inputClassName}
        value={value}
        disabled={disabled}
        onChange={(event) => change(event.target.valueAsNumber)}
      />
    </div>
  ) : (
    <label className={className}>
      Scale{" "}
      <select
        aria-label={label}
        disabled={disabled}
        value={value}
        onChange={(event) => change(Number(event.target.value))}
      >
        {presets.map((scale) => (
          <option key={scale} value={scale}>
            {scale}×
          </option>
        ))}
      </select>
    </label>
  );
}

/** React escapes imported content; never render serialized data as HTML. */
export function JsonView({
  value,
  className = "json",
}: {
  value: unknown;
  className?: string;
}) {
  return <pre className={className}>{JSON.stringify(value, null, 2)}</pre>;
}
