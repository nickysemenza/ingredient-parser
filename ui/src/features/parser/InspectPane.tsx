import { MousePointerClick } from "lucide-react";
import { api } from "../../api";
import { Inspector } from "../../components/ingredient";
import { EmptyState, Spinner } from "../../components/ui";
import { useQuery } from "../../lib/query";

/** The inspector for one input line, loaded on demand. */
export function InspectPane({ input, onClose, title }: { input: string | null; onClose?: () => void; title?: string }) {
  const inspection = useQuery(input === null ? null : () => api.inspect(input), [input]);
  if (input === null)
    return (
      <EmptyState icon={<MousePointerClick />} title="Select a line">
        Its fields, the pipeline stages and the grammar trace appear here.
      </EmptyState>
    );
  if (inspection.error) return <EmptyState title="Could not inspect this line">{inspection.error}</EmptyState>;
  if (!inspection.value)
    return (
      <div className="flex flex-1 items-center justify-center">
        <Spinner />
      </div>
    );
  return <Inspector data={inspection.value} onClose={onClose} title={title} />;
}
