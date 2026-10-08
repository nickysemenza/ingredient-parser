/// <reference lib="webworker" />
// The web build's backend: food-wasm in a module worker, so parsing never
// blocks the page. Messages carry the same command names and JSON arguments
// the desktop sends through Tauri.
import init, { dispatch, list_runs, put_file } from "../wasm/pkg/food_wasm.js";
import type { WorkerReply, WorkerRequest } from "./wasm";

const ready = init();

self.onmessage = async ({ data }: MessageEvent<WorkerRequest>) => {
  let reply: WorkerReply;
  try {
    await ready;
    let value: unknown = null;
    if (data.kind === "call") value = JSON.parse(dispatch(data.command, JSON.stringify(data.args ?? {})));
    else if (data.kind === "put") put_file(data.path, new Uint8Array(data.bytes));
    else if (data.kind === "runs") value = JSON.parse(list_runs());
    reply = { id: data.id, ok: true, value };
  } catch (error) {
    reply = { id: data.id, ok: false, error: error instanceof Error ? error.message : String(error) };
  }
  self.postMessage(reply);
};
