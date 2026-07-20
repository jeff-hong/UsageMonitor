// Settings page: widget/taskbar mode, scan interval, and the pricing editor.
// The pricing editor is the key feature — it lets the user fill in custom model
// prices (e.g. glm-5.1) so real dollar costs appear, and lists unpriced models
// detected in their usage data as a prompt.

import { useEffect, useState } from "react";
import {
  api,
  fmtTokens,
  getStoredTheme,
  getStoredTokenUnitMode,
  setStoredTheme,
  setStoredTokenUnitMode,
  type Pricing,
  type ThemeMode,
  type TokenUnitMode,
  type UnpricedModel,
} from "../lib/api";
import { nativeDragMouseDown } from "../lib/drag";

type TaskbarMode = "tray" | "live_number" | "off";
type PriceDraft = {
  in: string;
  out: string;
  cacheRead: string;
  cacheCreate: string;
};

const TASKBAR_LABEL: Record<TaskbarMode, string> = {
  tray: "普通图标",
  live_number: "实时数字",
  off: "关闭",
};
const TOKEN_UNIT_LABEL: Record<TokenUnitMode, string> = {
  compact: "K/M",
  wan: "万",
};
const THEME_LABEL: Record<ThemeMode, string> = {
  dark: "深色磨砂",
  light: "纯白",
  neon: "霓虹暗黑",
};

export function SettingsPage({
  onBack,
}: {
  onBack: () => void;
}) {
  const [pricing, setPricing] = useState<Pricing[]>([]);
  const [unpriced, setUnpriced] = useState<UnpricedModel[]>([]);
  const [interval, setIntervalSec] = useState(30);
  const [taskbar, setTaskbar] = useState<TaskbarMode>("tray");
  const [theme, setTheme] = useState<ThemeMode>(() => getStoredTheme());
  const [tokenUnit, setTokenUnit] = useState<TokenUnitMode>(() => getStoredTokenUnitMode());
  const [newModel, setNewModel] = useState("");
  const [draftPrice, setDraftPrice] = useState<PriceDraft>({
    in: "",
    out: "",
    cacheRead: "",
    cacheCreate: "",
  });
  const [editing, setEditing] = useState<Record<string, PriceDraft>>({});
  const [msg, setMsg] = useState("");

  const refresh = () => {
    api.listPricing().then(setPricing);
    api.getUnpricedModels().then(setUnpriced);
    api.getSetting("scan_interval_sec").then((v) => v && setIntervalSec(parseInt(v)));
    api.getSetting("taskbar_mode").then((v) => (v as TaskbarMode) && setTaskbar(v as TaskbarMode));
    api.getSetting("theme").then((v) => {
      const t = v === "light" || v === "neon" ? (v as ThemeMode) : "dark";
      setTheme(t);
      setStoredTheme(t);
    });
    api.getSetting("token_unit_mode").then((v) => {
      const mode = v === "wan" ? "wan" : "compact";
      setTokenUnit(mode);
      setStoredTokenUnitMode(mode);
    });
  };

  useEffect(() => {
    refresh();
  }, []);

  const flash = (m: string) => {
    setMsg(m);
    setTimeout(() => setMsg(""), 2500);
  };

  const savePrice = async (
    model: string,
    inV: string,
    outV: string,
    cacheReadV: string,
    cacheCreateV: string
  ) => {
    const inN = parseFloat(inV) || 0;
    const outN = parseFloat(outV) || 0;
    const cacheReadN = parseFloat(cacheReadV) || 0;
    const cacheCreateN = parseFloat(cacheCreateV) || 0;
    await api.setPricing(model, inN, outN, cacheReadN, cacheCreateN);
    await api.recomputeCost();
    refresh();
    flash(`${model} 单价已保存，花费已重算`);
  };

  const addCustom = async () => {
    if (!newModel.trim()) return;
    await savePrice(newModel.trim(), draftPrice.in, draftPrice.out, draftPrice.cacheRead, draftPrice.cacheCreate);
    setNewModel("");
    setDraftPrice({ in: "", out: "", cacheRead: "", cacheCreate: "" });
  };

  const remove = async (model: string) => {
    await api.deletePricing(model);
    await api.recomputeCost();
    refresh();
    flash(`${model} 已删除`);
  };

  const setIntervalSetting = async (v: number) => {
    setIntervalSec(v);
    await api.setSetting("scan_interval_sec", String(v));
    flash(`扫描间隔已设为 ${v} 秒`);
  };

  const pickTaskbar = async (m: TaskbarMode) => {
    setTaskbar(m);
    await api.setSetting("taskbar_mode", m);
    await api.refreshTaskbar();
    flash(
      m === "live_number"
        ? "任务栏已显示今日 Token 数"
        : m === "off"
          ? "任务栏图标已隐藏"
          : "任务栏已切换为普通图标"
    );
  };

  const pickTheme = async (t: ThemeMode) => {
    setTheme(t);
    setStoredTheme(t);
    await api.setSetting("theme", t);
    flash(`主题已切换为 ${THEME_LABEL[t]}`);
  };

  const pickTokenUnit = async (m: TokenUnitMode) => {
    setTokenUnit(m);
    setStoredTokenUnitMode(m);
    await api.setSetting("token_unit_mode", m);
    window.dispatchEvent(new CustomEvent("token-unit-mode-changed"));
    flash(`Token 单位已切换为 ${TOKEN_UNIT_LABEL[m]}`);
  };

  return (
    <div className="glass-card sub-page">
      <div className="page-head" onMouseDown={nativeDragMouseDown("detail")}>
        <span className="back" data-no-drag role="button" onClick={onBack}>‹</span>
        <span className="page-title">设置</span>
      </div>

      {msg && <div className="flash">{msg}</div>}

      {/* display modes */}
      <Section title="主题">
        <div className="seg-tabs">
          {(["dark", "light", "neon"] as ThemeMode[]).map((t) => (
            <div key={t} className={`tab ${theme === t ? "active" : ""}`} onClick={() => pickTheme(t)}>
              {THEME_LABEL[t]}
            </div>
          ))}
        </div>
        <div className="hint-note">切换全部界面的配色风格，即时生效</div>
      </Section>

      <Section title="任务栏">
        <div className="seg-tabs">
          {(["tray", "live_number", "off"] as TaskbarMode[]).map((m) => (
            <div key={m} className={`tab ${taskbar === m ? "active" : ""}`} onClick={() => pickTaskbar(m)}>
              {TASKBAR_LABEL[m]}
            </div>
          ))}
        </div>
        <div className="hint-note">实时数字会把任务栏按钮标题更新为今日 Token 数和 Claude/Codex 分项</div>
      </Section>

      <Section title="扫描间隔">
        <div className="interval-row">
          <input
            type="range"
            min={5}
            max={300}
            step={5}
            value={interval}
            onChange={(e) => setIntervalSetting(parseInt(e.target.value))}
          />
          <span className="interval-val">{interval} 秒</span>
        </div>
      </Section>

      <Section title="Token 单位">
        <div className="seg-tabs">
          {(["compact", "wan"] as TokenUnitMode[]).map((m) => (
            <div
              key={m}
              className={`tab ${tokenUnit === m ? "active" : ""}`}
              onClick={() => pickTokenUnit(m)}
            >
              {TOKEN_UNIT_LABEL[m]}
            </div>
          ))}
        </div>
      </Section>

      {/* pricing editor — the heart of phase 5 */}
      <Section title="模型定价">
        <button
          className="add-btn"
          style={{ marginBottom: 12 }}
          onClick={async () => {
            const n = await api.syncPricingFromCcswitch();
            refresh();
            flash(n > 0 ? `已从 cc-switch 同步 ${n} 个模型定价` : "未检测到 cc-switch 定价数据");
          }}
        >
          ⟳ 从 cc-switch 同步定价
        </button>
        {unpriced.length > 0 && (
          <div className="unpriced-box">
            <div className="unpriced-title">⚠ 检测到未定价模型（花费显示为 $0）</div>
            {unpriced.map((u) => (
              <div
                key={u.model}
                className="unpriced-row"
                onClick={() => setNewModel(u.model)}
              >
                <span className="unpriced-name">{u.model}</span>
                <span className="unpriced-tok">{fmtTokens(u.tokens)} tok 待计费</span>
                <span className="unpriced-act">点击补价 →</span>
              </div>
            ))}
          </div>
        )}

        <div className="price-list">
          {pricing.map((p) => {
            const e = editing[p.model] ?? {
              in: String(p.in_per_mtok),
              out: String(p.out_per_mtok),
              cacheRead: String(p.cache_read_per_mtok),
              cacheCreate: String(p.cache_create_per_mtok),
            };
            return (
              <div className="price-row" key={p.model}>
                <div className="price-model">
                  {p.model}
                  {p.builtin && <span className="builtin-tag">内置</span>}
                </div>
                <div className="price-inputs">
                  <label>
                    输入成本 (每百万 tokens, USD)
                    <input
                      type="number"
                      step="0.01"
                      value={e.in}
                      onChange={(ev) =>
                        setEditing({ ...editing, [p.model]: { ...e, in: ev.target.value } })
                      }
                    />
                  </label>
                  <label>
                    输出成本 (每百万 tokens, USD)
                    <input
                      type="number"
                      step="0.01"
                      value={e.out}
                      onChange={(ev) =>
                        setEditing({ ...editing, [p.model]: { ...e, out: ev.target.value } })
                      }
                    />
                  </label>
                  <label>
                    缓存读取成本 (每百万 tokens, USD)
                    <input
                      type="number"
                      step="0.01"
                      value={e.cacheRead}
                      onChange={(ev) =>
                        setEditing({ ...editing, [p.model]: { ...e, cacheRead: ev.target.value } })
                      }
                    />
                  </label>
                  <label>
                    缓存写入成本 (每百万 tokens, USD)
                    <input
                      type="number"
                      step="0.01"
                      value={e.cacheCreate}
                      onChange={(ev) =>
                        setEditing({ ...editing, [p.model]: { ...e, cacheCreate: ev.target.value } })
                      }
                    />
                  </label>
                </div>
                <div className="price-actions">
                  <button onClick={() => savePrice(p.model, e.in, e.out, e.cacheRead, e.cacheCreate)}>保存</button>
                  {!p.builtin && <button className="danger" onClick={() => remove(p.model)}>删除</button>}
                </div>
              </div>
            );
          })}
        </div>

        {/* add custom model */}
        <div className="add-model">
          <div className="add-title">添加自定义模型</div>
          <input
            className="add-input"
            placeholder="模型名，如 glm-5.1"
            value={newModel}
            onChange={(e) => setNewModel(e.target.value)}
          />
          <div className="price-inputs">
            <label>
              输入成本 (每百万 tokens, USD)
              <input
                type="number"
                step="0.01"
                value={draftPrice.in}
                onChange={(e) => setDraftPrice({ ...draftPrice, in: e.target.value })}
              />
            </label>
            <label>
              输出成本 (每百万 tokens, USD)
              <input
                type="number"
                step="0.01"
                value={draftPrice.out}
                onChange={(e) => setDraftPrice({ ...draftPrice, out: e.target.value })}
              />
            </label>
            <label>
              缓存读取成本 (每百万 tokens, USD)
              <input
                type="number"
                step="0.01"
                value={draftPrice.cacheRead}
                onChange={(e) => setDraftPrice({ ...draftPrice, cacheRead: e.target.value })}
              />
            </label>
            <label>
              缓存写入成本 (每百万 tokens, USD)
              <input
                type="number"
                step="0.01"
                value={draftPrice.cacheCreate}
                onChange={(e) => setDraftPrice({ ...draftPrice, cacheCreate: e.target.value })}
              />
            </label>
          </div>
          <button className="add-btn" onClick={addCustom}>
            添加并重算
          </button>
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="settings-section">
      <div className="section-title">{title}</div>
      {children}
    </div>
  );
}
