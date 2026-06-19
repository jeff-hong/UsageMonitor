import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface DbStatus {
  ok: boolean;
  path: string;
  pricing_rows: number;
}

interface Pricing {
  model: string;
  in_per_mtok: number;
  out_per_mtok: number;
  cache_per_mtok: number;
  builtin: boolean;
}

// Phase 1 smoke-test UI: confirms the Rust backend boots, the SQLite database
// is created under the app data dir, and the built-in pricing table is seeded.
// This screen is replaced by the real widget UI in phase 4.
function App() {
  const [status, setStatus] = useState<DbStatus | null>(null);
  const [pricing, setPricing] = useState<Pricing[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<DbStatus>("db_status")
      .then((s) => {
        setStatus(s);
        return invoke<Pricing[]>("list_pricing");
      })
      .then(setPricing)
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <div className="glass-card" data-tauri-drag-region>
      <div className="label-tiny">PHASE 1 · BACKEND CHECK</div>
      <h1>UsageMonitor</h1>

      <section className="status-block">
        <div className="status-row">
          <span>后端就绪</span>
          <span className={status?.ok ? "ok" : "pending"}>
            {status?.ok ? "✓" : "…"}
          </span>
        </div>
        <div className="status-row">
          <span>数据库路径</span>
          <span className="mono">{status?.path ?? "—"}</span>
        </div>
        <div className="status-row">
          <span>内置定价条目</span>
          <span className="mono">{status?.pricing_rows ?? 0}</span>
        </div>
      </section>

      {error && <div className="error">{error}</div>}

      <section className="pricing-block">
        <div className="label-tiny">内置参考定价 (USD / 1M tokens)</div>
        <ul className="pricing-list">
          {pricing.map((p) => (
            <li key={p.model}>
              <span className="model">{p.model}</span>
              <span className="prices">
                in ${p.in_per_mtok} · out ${p.out_per_mtok} · cache ${p.cache_per_mtok}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

export default App;
