// Typed bridge to the Rust shell. Command names and payloads mirror
// apps/desktop/src-tauri/src/lib.rs; result types come from @selecta/protocol,
// generated from the Rust structs, so a renamed field is a type error here.

import type { Analysis, GroupProgress, ScanReport, TrackSummary } from "@selecta/protocol";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export interface Settings {
  musicDirs: string[];
}

export const inTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri()) return Promise.reject(new Error("Selecta's backend isn't running. Start the app with `npm run dev`, not the bare UI."));
  return invoke<T>(cmd, args);
}

export const api = {
  settings: () => call<Settings>("settings_get"),
  saveSettings: (settings: Settings) => call<void>("settings_set", { settings }),
  tracks: () => call<TrackSummary[]>("library_tracks"),
  scan: () => call<ScanReport>("library_scan"),
  analysis: (id: string) => call<Analysis>("track_analysis", { id }),
  progress: () => call<GroupProgress[]>("jobs_progress"),
};

/** Raw bytes of a library file, read through Tauri's scoped asset protocol. */
export async function readAudio(path: string): Promise<ArrayBuffer> {
  const res = await fetch(convertFileSrc(path));
  if (!res.ok) throw new Error(`Couldn't read ${path} (${res.status})`);
  return res.arrayBuffer();
}
