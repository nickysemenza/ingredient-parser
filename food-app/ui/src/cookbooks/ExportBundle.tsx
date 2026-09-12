import { useState } from "react";
import { api, pick, revealFile } from "../bridge";

/** Export an existing run through the same native operation as the CLI. */
export function ExportBundle({
  runPath,
  sourcePath,
  onBusy,
  onError,
}: {
  runPath: string;
  sourcePath: string | null;
  onBusy: (busy: boolean) => void;
  onError: (message: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [source, setSource] = useState<string | null>(null);
  const [output, setOutput] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const exportRun = async (chooseSource = false) => {
    setBusy(true);
    onBusy(true);
    try {
      const book = chooseSource
        ? await pick("epub")
        : (source ?? sourcePath ?? (await pick("epub")));
      if (!book) return;
      setSource(book);
      const result = await api.exportBundle(runPath, book);
      setOutput(result.path);
      setFailed(false);
    } catch (error) {
      setFailed(true);
      onError(String(error));
    } finally {
      setBusy(false);
      onBusy(false);
    }
  };
  return (
    <div className="bundle-export">
      <div className="actions">
        <button disabled={busy} onClick={() => void exportRun()}>
          {busy ? "Exporting bundle…" : "Export bundle"}
        </button>
        {failed && (
          <button disabled={busy} onClick={() => void exportRun(true)}>
            Choose source EPUB
          </button>
        )}
        {output && (
          <button
            onClick={() =>
              void revealFile(output).catch((error: unknown) =>
                onError(String(error)),
              )
            }
          >
            Reveal bundle in Finder
          </button>
        )}
      </div>
      {output && (
        <p className="caption" role="status">
          Exported {output}
        </p>
      )}
    </div>
  );
}
