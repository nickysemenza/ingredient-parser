import { BookCheck, Globe, Rows3 } from "lucide-react";
import { isDesktop } from "../../api";
import { param, setParams } from "../../app/router";
import { Segmented } from "../../components/ui";
import { useStored } from "../../lib/stored";
import { Corpus } from "./Corpus";
import { Lines } from "./Lines";
import { Recipe } from "./Recipe";

const MODES = ["Lines", "Recipe", "Corpus"] as const;
type Mode = (typeof MODES)[number];
const ICONS = { Lines: <Rows3 />, Recipe: <Globe />, Corpus: <BookCheck /> };

function initialMode(stored: Mode): Mode {
  const wanted = param("mode");
  return MODES.find((m) => m.toLowerCase() === wanted) ?? stored;
}

export default function Parser() {
  const [mode, setMode] = useStored<Mode>("v2:parser-mode", "Lines", MODES);
  const shown = isDesktop ? mode : initialMode(mode);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center justify-between gap-4 border-b border-line px-4">
        <h1 className="text-[15px] font-semibold tracking-tight">Parser</h1>
        <Segmented
          label="Parser source"
          value={shown}
          items={MODES}
          onChange={(next) => {
            setMode(next);
            setParams({ mode: next === "Lines" ? null : next.toLowerCase(), q: null, url: null });
          }}
          render={(item) => (
            <>
              {ICONS[item]}
              {item}
            </>
          )}
        />
      </header>
      {shown === "Lines" ? <Lines /> : shown === "Recipe" ? <Recipe /> : <Corpus />}
    </div>
  );
}
