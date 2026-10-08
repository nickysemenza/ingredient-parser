// The page's side of the WASM worker: one request id per message, replies
// matched back to their promises. The worker starts on first use.
export type WorkerRequest = { id: number } & (
  | { kind: "call"; command: string; args?: Record<string, unknown> }
  | { kind: "put"; path: string; bytes: ArrayBuffer }
  | { kind: "runs" }
);
export type WorkerReply = { id: number } & ({ ok: true; value: unknown } | { ok: false; error: string });

type Pending = { resolve: (value: unknown) => void; reject: (error: Error) => void };
type Request = WorkerRequest extends infer R ? (R extends WorkerRequest ? Omit<R, "id"> : never) : never;

let worker: Worker | null = null;
let next = 0;
const pending = new Map<number, Pending>();

function start(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = ({ data }: MessageEvent<WorkerReply>) => {
    const entry = pending.get(data.id);
    pending.delete(data.id);
    if (!entry) return;
    if (data.ok) entry.resolve(data.value);
    else entry.reject(new Error(data.error));
  };
  worker.onerror = (event) => {
    const error = new Error(event.message || "The parser stopped unexpectedly. Reload the page.");
    for (const entry of pending.values()) entry.reject(error);
    pending.clear();
  };
  return worker;
}

function send<T>(request: Request, transfer: Transferable[] = []): Promise<T> {
  const id = ++next;
  return new Promise<T>((resolve, reject) => {
    pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
    start().postMessage({ ...request, id }, transfer);
  });
}

export const wasm = {
  call: <T>(command: string, args?: Record<string, unknown>) => send<T>({ kind: "call", command, args }),
  /** Make a file the user opened readable by name. */
  put: async (path: string, bytes: ArrayBuffer) => {
    await send<null>({ kind: "put", path, bytes }, [bytes]);
  },
  runs: <T>() => send<T>({ kind: "runs" }),
};
