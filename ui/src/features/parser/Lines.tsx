import { Eraser, Link2, ListPlus } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { api, isDesktop } from "../../api";
import { copyText } from "../../api/platform";
import { useCommands } from "../../app/commands";
import { param, setParams } from "../../app/router";
import { useReportStatus } from "../../app/status";
import { Split } from "../../components/layout";
import { ActionMenu, Button, IconButton, Textarea } from "../../components/ui";
import { EXAMPLE_LINES } from "../../lib/examples";
import { useQuery } from "../../lib/query";
import { useStored } from "../../lib/stored";
import { IngredientTable } from "./IngredientTable";
import { InspectPane } from "./InspectPane";

export function Lines() {
  const [stored, setStored] = useStored("v2:lines", EXAMPLE_LINES.slice(0, 5).join("\n"));
  const [input, setInputState] = useState(() => param("q") ?? stored);
  const setInput = (value: string) => {
    setInputState(value);
    setStored(value);
  };
  useEffect(() => {
    if (!isDesktop) setParams({ q: input.length <= 1500 ? input : null });
  }, [input]);

  const parsed = useQuery(input.trim() ? () => api.parseLines(input) : null, [input], 120);
  const rows = useMemo(() => (parsed.value ?? []).filter((r) => r.input.trim()), [parsed.value]);
  const [selected, setSelected] = useState<number | null>(null);
  const current = rows.find((r) => r.lineNumber === selected) ?? rows[0] ?? null;
  const review = rows.filter((r) => r.reviewReasons.length > 0).length;

  useReportStatus("parser", {
    message: parsed.loading ? "Parsing…" : rows.length ? `${rows.length} line${rows.length === 1 ? "" : "s"} parsed` : "Ready",
    detail: review ? `${review} to review` : undefined,
  });

  const share = async () => {
    await copyText(location.href);
    toast.success("Link copied");
  };
  const load = (lines: string[]) => setInput(lines.join("\n"));
  useCommands(
    "parser-lines",
    [
      { id: "lines-examples", title: "Load example lines", group: "Parser", icon: <ListPlus />, run: () => load(EXAMPLE_LINES) },
      { id: "lines-clear", title: "Clear lines", group: "Parser", icon: <Eraser />, run: () => setInput("") },
      ...(isDesktop ? [] : [{ id: "lines-share", title: "Copy a link to these lines", group: "Parser", icon: <Link2 />, run: () => void share() }]),
    ],
    [],
  );

  return (
    <Split
      id="lines"
      initial={60}
      min={35}
      max={75}
      left={
        <>
          <div className="shrink-0 border-b border-line p-3">
            <div className="relative">
              <label htmlFor="lines-input" className="sr-only">
                Ingredient lines
              </label>
              <Textarea
                id="lines-input"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                rows={Math.min(9, Math.max(4, input.split("\n").length + 1))}
                spellCheck={false}
                placeholder="One ingredient per line, e.g. 2 cups flour, sifted"
                className="font-mono text-[13px]"
              />
            </div>
            <div className="mt-2 flex items-center justify-between gap-2">
              <span className="text-[11.5px] text-faint">Parses as you type · one ingredient per line</span>
              <div className="flex items-center gap-1">
                <ActionMenu
                  label="Examples"
                  items={[
                    { label: "Common lines", onSelect: () => load(EXAMPLE_LINES) },
                    { label: "Baking", onSelect: () => load(EXAMPLE_LINES.slice(1, 6)) },
                    { label: "Hard cases", onSelect: () => load(EXAMPLE_LINES.slice(6)) },
                  ]}
                />
                {!isDesktop && (
                  <IconButton label="Copy link" onClick={() => void share()}>
                    <Link2 />
                  </IconButton>
                )}
                <Button size="sm" variant="ghost" onClick={() => setInput("")} disabled={!input}>
                  Clear
                </Button>
              </div>
            </div>
            {parsed.error && <p className="mt-2 text-[12px] text-bad">{parsed.error}</p>}
          </div>
          <IngredientTable rows={rows} selected={current?.lineNumber ?? null} onSelect={setSelected} stale={parsed.loading} />
        </>
      }
      right={<InspectPane input={current?.input ?? null} />}
    />
  );
}
