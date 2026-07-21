// API layer: typed wrappers around Tauri commands defined in src-tauri.
// Keeps all invoke() calls in one place so components stay clean and the
// payload shapes match the Rust DTOs in query.rs / models.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Range = "today" | "week" | "month" | "all";
export type TokenUnitMode = "compact" | "wan";
export type ThemeMode = "dark" | "light" | "neon";

const TOKEN_UNIT_MODE_KEY = "token_unit_mode";
const THEME_KEY = "theme";

export interface ToolBreakdown {
  tool: string; // "claude" | "codex"
  cost_usd: number;
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  cache_create_tok: number;
  session_count: number;
  fully_priced: boolean;
}

export interface Summary {
  cost_usd: number;
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  cache_create_tok: number;
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
  cache_create_tok: number;
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
  cache_create_tok: number;
  timestamp: number;
  priced: boolean;
}

export interface Pricing {
  model: string;
  in_per_mtok: number;
  out_per_mtok: number;
  cache_read_per_mtok: number;
  cache_create_per_mtok: number;
  builtin: boolean;
}

export interface IndexProgress {
  indexed: number;
  total: number;
  done: boolean;
}

export interface UnpricedModel {
  model: string;
  tokens: number;
}

export interface ModelBreakdown {
  model: string;
  tool: string;
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  cache_create_tok: number;
  cost_usd: number;
  priced: boolean;
}

export interface ProviderUsage {
  app_type: string;
  provider_name: string;
  provider_id: string;
  mode: "balance" | "plan" | string;
  primary_label: string;
  primary_value: string;
  primary_updated_text: string | null;
  secondary_label: string | null;
  secondary_value: string | null;
  secondary_updated_text: string | null;
  updated_text: string | null;
  ok: boolean;
}

// --- commands ---

export const api = {
  getTodaySummary: () => invoke<Summary>("get_today_summary"),
  getRangeSummary: (range: Range) => invoke<Summary>("get_range_summary", { range }),
  getHistory: (range: Range) => invoke<DayPoint[]>("get_history", { range }),
  getDailySessions: (date: string) => invoke<SessionRow[]>("get_daily_sessions", { date }),
  getProjects: () => invoke<ProjectRow[]>("get_projects"),
  getProjectSessions: (project: string) =>
    invoke<SessionRow[]>("get_project_sessions", { project }),
  getProjectByModel: (project: string) =>
    invoke<ModelBreakdown[]>("get_project_by_model", { project }),
  getByModel: (range: Range) => invoke<ModelBreakdown[]>("get_by_model", { range }),
  getTodayByModel: () => invoke<ModelBreakdown[]>("get_today_by_model"),
  getCurrentProviderUsage: (appType: "claude" | "codex") =>
    invoke<ProviderUsage | null>("get_current_provider_usage", { appType }),
  recomputeCost: () => invoke<void>("recompute_cost"),
  syncPricingFromCcswitch: () => invoke<number>("sync_pricing_from_ccswitch"),
  listPricing: () => invoke<Pricing[]>("list_pricing"),
  setPricing: (
    model: string,
    in_per_mtok: number,
    out_per_mtok: number,
    cache_read_per_mtok: number,
    cache_create_per_mtok: number
  ) =>
    invoke<void>("set_pricing", {
      model,
      in_per_mtok,
      out_per_mtok,
      cache_read_per_mtok,
      cache_create_per_mtok,
    }),
  deletePricing: (model: string) => invoke<boolean>("delete_pricing", { model }),
  getUnpricedModels: () => invoke<UnpricedModel[]>("get_unpriced_models"),
  getSetting: (key: string) => invoke<string | null>("get_setting", { key }),
  setSetting: (key: string, value: string) =>
    invoke<void>("set_setting", { key, value }),
};

// Subscribe to the backend's indexing progress events.
export function onIndexProgress(cb: (p: IndexProgress) => void): Promise<UnlistenFn> {
  return listen<IndexProgress>("index-progress", (e) => cb(e.payload));
}

// --- formatting helpers (shared by every view) ---

export function fmtUsd(n: number): string {
  if (!n || n <= 0) return "$0.0000";
  return `$${n.toFixed(4)}`;
}

export function fmtTokens(n: number): string {
  if (getStoredTokenUnitMode() === "wan" && n >= 10_000) {
    return `${(n / 10_000).toFixed(1)}万`;
  }
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n.toFixed(1)}`;
}

export function getStoredTokenUnitMode(): TokenUnitMode {
  if (typeof window === "undefined") return "compact";
  const mode = window.localStorage.getItem(TOKEN_UNIT_MODE_KEY);
  return mode === "wan" ? "wan" : "compact";
}

export function setStoredTokenUnitMode(mode: TokenUnitMode): void {
  if (typeof window !== "undefined") {
    window.localStorage.setItem(TOKEN_UNIT_MODE_KEY, mode);
  }
}

export function getStoredTheme(): ThemeMode {
  if (typeof window === "undefined") return "dark";
  const t = window.localStorage.getItem(THEME_KEY);
  return t === "light" || t === "neon" ? t : "dark";
}

export function setStoredTheme(theme: ThemeMode): void {
  if (typeof window !== "undefined") {
    window.localStorage.setItem(THEME_KEY, theme);
    document.documentElement.dataset.theme = theme;
    // Notify ALL windows (widget / detail / hover_detail) to re-apply the
    // theme. Each Tauri webview window has its own DOM, so changing
    // data-theme here only affects the current window — without this event
    // the dock capsule wouldn't update when you switch theme in settings.
    import("@tauri-apps/api/event")
      .then(({ emit }) => emit("theme-changed", theme))
      .catch(() => {});
  }
}

export function totalTokens(row: {
  input_tok: number;
  output_tok: number;
  cache_tok: number;
  cache_create_tok?: number;
}): number {
  return row.input_tok + row.output_tok + row.cache_tok + (row.cache_create_tok ?? 0);
}

export function fmtPercent(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0.0%";
  return `${Math.min(n, 100).toFixed(1)}%`;
}

export function cacheHitRate(row: {
  input_tok: number;
  cache_tok: number;
  cache_create_tok?: number;
}): number {
  const totalInputSide = row.input_tok + row.cache_tok + (row.cache_create_tok ?? 0);
  if (totalInputSide <= 0) return 0;
  return (row.cache_tok / totalInputSide) * 100;
}

export function fmtDate(iso: string): string {
  const d = new Date(iso + "T00:00:00");
  return d.toLocaleDateString("zh-CN", { month: "long", day: "numeric" });
}
