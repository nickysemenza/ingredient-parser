import type { ReactNode } from "react";

/** A labelled fact list; `null` entries are dropped so callers can be terse. */
export function Facts({ items }: { items: ([string, ReactNode] | null)[] }) {
  return (
    <dl className="fields">
      {items
        .filter((item): item is [string, ReactNode] => item !== null)
        .map(([term, value]) => (
          <div key={term}>
            <dt>{term}</dt>
            <dd>{value}</dd>
          </div>
        ))}
    </dl>
  );
}

export interface Row {
  key: string;
  cells: ReactNode[];
  className?: string;
}
/** A report table whose first cell of each row is its header. */
export function Table({ head, rows }: { head: string[]; rows: Row[] }) {
  return (
    <table className="report-table">
      <thead>
        <tr>
          {head.map((label) => (
            <th key={label}>{label}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.key} className={row.className}>
            {row.cells.map((cell, index) =>
              index ? (
                <td key={index}>{cell}</td>
              ) : (
                <th scope="row" key={index}>
                  {cell}
                </th>
              ),
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
