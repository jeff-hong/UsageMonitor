// API layer: typed wrappers around Tauri commands defined in src-tauri.
// Keeps all invoke() calls in one place so components stay clean and the
// payload shapes match the Rust DTOs in query.rs / models.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Range = "today" | "week" | "month" | "all";

export interface ToolBreakdown {
  tool: string; // "claude" | "codex"
  cost_usd: number;
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  session_count: number;
  fully_priced: boolean;
}

export interface Summary {
  cost_usd: number;
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  session_count: number;
  fully_priced: boolean;
  tools: ToolBreakdown[];
}

export interface DayPoint {
  date: string;
  cost_usd: number;
  tokens: number;
  session_count: number;
}

export interface ProjectRow {
  project: string;
  cost_usd: number;
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  session_count: number;
  claude_tokens: number;
  codex_tokens: number;
}

export interface SessionRow {
  tool: string;
  project: string | null;
  model: string;
  cost_usd: number;
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  timestamp: number;
  priced: boolean;
}

export interface Pricing {
  model: string;
  in_per_mtok: number;
  out_per_mtok: number;
  cache_per_mtok: number;
  builtin: boolean;
}

export interface IndexProgress {
  indexed: number;
  total: number;
  done: boolean;
}

// --- commands ---

export const api = {
  getTodaySummary: () => invoke<Summary>("get_today_summary"),
  getRangeSummary: (range: Range) => invoke<Summary>("get_range_summary", { range }),
  getHistory: (range: Range) => invoke<DayPoint[]>("get_history", { range }),
  getDailySessions: (date: string) => invoke<SessionRow[]>("get_daily_sessions", { date }),
  getProjects: () => invoke<ProjectRow[]>("get_projects"),
  recomputeCost: () => invoke<void>("recompute_cost"),
  listPricing: () => invoke<Pricing[]>("list_pricing"),
};

// Subscribe to the backend's indexing progress events.
export function onIndexProgress(cb: (p: IndexProgress) => void): Promise<UnlistenFn> {
  return listen<IndexProgress>("index-progress", (e) => cb(e.payload));
}

// --- formatting helpers (shared by every view) ---

export function fmtUsd(n: number): string {
  if (!n || n <= 0) return "$0.00";
  if (n < 0.01) return `<$0.01`;
  return `$${n.toFixed(2)}`;
}

export function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
  return `${n}`;
}

export function fmtDate(iso: string): string {
  const d = new Date(iso + "T00:00:00");
  return d.toLocaleDateString("zh-CN", { month: "long", day: "numeric" });
}
