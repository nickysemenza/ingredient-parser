import { useState } from "react";
import { X, Copy } from "lucide-react";
import { copy, type IngredientInspection, type TraceNode } from "./bridge";
import { Fields, JsonView, Menu, Tabs } from "./components";
function Tree({
  value,
  label = "Trace",
}: {
  value: TraceNode | null;
  label?: string;
}) {
  if (!value) return <p>No trace available.</p>;
  return (
    <details className={`tree-node ${value.outcome}`} open={label === "Trace"}>
      <summary>
        {value.name} <span>{value.outcome}</span>
      </summary>
      <p className="mono">{value.input}</p>
      <p>{value.detail}</p>
      {value.children.map((node, i) => (
        <Tree key={i} label={node.name} value={node} />
      ))}
    </details>
  );
}
export function Inspector({
  data,
  onClose,
}: {
  data: IngredientInspection;
  onClose?: () => void;
}) {
  const [view, setView] = useState<"Fields" | "Stages" | "Trace tree" | "JSON">(
    "Fields",
  );
  const [copied, setCopied] = useState("");
  const doCopy = async (value: string) => {
    try {
      await copy(value);
      setCopied("Copied");
    } catch (e) {
      setCopied(String(e));
    }
  };
  return (
    <section className="inspector" aria-label="Ingredient inspector">
      <header className="pane-header">
        <h2>Ingredient</h2>
        <div className="actions">
          <Menu label="Actions">
            <button onClick={() => void doCopy(data.result.input)}>
              <Copy size={14} />
              Copy input
            </button>
            <button onClick={() => void doCopy(data.jaegerJson)}>
              Copy Jaeger JSON
            </button>
          </Menu>
          {onClose && (
            <button
              className="icon-button"
              aria-label="Close inspector"
              onClick={onClose}
            >
              <X size={17} />
            </button>
          )}
        </div>
      </header>
      <p className="inspector-input mono">{data.result.input}</p>
      <Tabs
        value={view}
        items={["Fields", "Stages", "Trace tree", "JSON"]}
        onChange={setView}
        label="Ingredient evidence"
      />
      <div className="inspector-content scroll">
        {view === "Fields" ? (
          <>
            <Fields
              value={{
                name: data.result.name,
                amounts: data.result.amounts.join(" / "),
                modifier: data.result.modifier,
                optional: data.result.optional ? "Yes" : "No",
                usage: data.result.usage,
                confidence: data.result.confidence,
              }}
            />
            {data.result.reviewReasons.map((reason) => (
              <p key={reason} className="stale">
                {reason}
              </p>
            ))}
          </>
        ) : view === "Stages" ? (
          <pre className="stage-report">{data.stages}</pre>
        ) : view === "Trace tree" ? (
          <Tree value={data.trace} />
        ) : (
          <JsonView value={data.result.json} />
        )}
      </div>
      {copied && (
        <p role="status" className="caption">
          {copied}
        </p>
      )}
    </section>
  );
}
