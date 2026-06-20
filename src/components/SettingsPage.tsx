// Settings page: widget/taskbar mode, scan interval, and the pricing editor.
// The pricing editor is the key feature — it lets the user fill in custom model
// prices (e.g. glm-5.1) so real dollar costs appear, and lists unpriced models
// detected in their usage data as a prompt.

import { useEffect, useState } from "react";
import { api, fmtTokens, type Pricing, type UnpricedModel } from "../lib/api";

type WidgetShape = "card" | "pill";
type TaskbarMode = "tray" | "live_number" | "off";

const WIDGET_LABEL: Record<WidgetShape, string> = { card: "卡片", pill: "胶囊" };
const TASKBAR_LABEL: Record<TaskbarMode, string> = {
  tray: "托盘图标",
  live_number: "实时数字",
  off: "关闭",
};

export function SettingsPage({
  onBack,
  onShapeChange,
}: {
  onBack: () => void;
  onShapeChange: (s: WidgetShape) => void;
}) {
  const [pricing, setPricing] = useState<Pricing[]>([]);
  const [unpriced, setUnpriced] = useState<UnpricedModel[]>([]);
  const [interval, setIntervalSec] = useState(30);
  const [shape, setShape] = useState<WidgetShape>("card");
  const [taskbar, setTaskbar] = useState<TaskbarMode>("tray");
  const [newModel, setNewModel] = useState("");
  const [draftPrice, setDraftPrice] = useState<{ in: string; out: string; cache: string }>({
    in: "",
    out: "",
    cache: "",
  });
  const [editing, setEditing] = useState<Record<string, { in: string; out: string; cache: string }>>({});
  const [msg, setMsg] = useState("");

  const refresh = () => {
    api.listPricing().then(setPricing);
    api.getUnpricedModels().then(setUnpriced);
    api.getSetting("scan_interval_sec").then((v) => v && setIntervalSec(parseInt(v)));
    api.getSetting("widget_shape").then((v) => (v as WidgetShape) && setShape(v as WidgetShape));
    api.getSetting("taskbar_mode").then((v) => (v as TaskbarMode) && setTaskbar(v as TaskbarMode));
  };

  useEffect(() => {
    refresh();
  }, []);

  const flash = (m: string) => {
    setMsg(m);
    setTimeout(() => setMsg(""), 2500);
  };

  const savePrice = async (model: string, inV: string, outV: string, cacheV: string) => {
    const inN = parseFloat(inV) || 0;
    const outN = parseFloat(outV) || 0;
    const cacheN = parseFloat(cacheV) || 0;
    await api.setPricing(model, inN, outN, cacheN);
    await api.recomputeCost();
    refresh();
    flash(`${model} 单价已保存，花费已重算`);
  };

  const addCustom = async () => {
    if (!newModel.trim()) return;
    await savePrice(newModel.trim(), draftPrice.in, draftPrice.out, draftPrice.cache);
    setNewModel("");
    setDraftPrice({ in: "", out: "", cache: "" });
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

  const pickShape = async (s: WidgetShape) => {
    setShape(s);
    await api.setSetting("widget_shape", s);
    onShapeChange(s);
  };

  const pickTaskbar = async (m: TaskbarMode) => {
    setTaskbar(m);
    await api.setSetting("taskbar_mode", m);
  };

  return (
    <div className="glass-card sub-page">
      <div className="page-head" data-tauri-drag-region>
        <span className="back" onClick={onBack}>‹</span>
        <span className="page-title">设置</span>
      </div>

      {msg && <div className="flash">{msg}</div>}

      {/* display modes */}
      <Section title="悬浮窗样式">
        <div className="seg-tabs">
          {(["card", "pill"] as const).map((s) => (
            <div
              key={s}
              className={`tab ${shape === s ? "active" : ""}`}
              onClick={() => pickShape(s as WidgetShape)}
            >
              {WIDGET_LABEL[s]}
            </div>
          ))}
        </div>
      </Section>

      <Section title="任务栏">
        <div className="seg-tabs">
          {(["tray", "live_number", "off"] as TaskbarMode[]).map((m) => (
            <div key={m} className={`tab ${taskbar === m ? "active" : ""}`} onClick={() => pickTaskbar(m)}>
              {TASKBAR_LABEL[m]}
            </div>
          ))}
        </div>
        <div className="hint-note">任务栏实时数字/托盘将在后续版本实现</div>
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

      {/* pricing editor — the heart of phase 5 */}
      <Section title="模型定价 (USD / 1M tokens)">
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
              cache: String(p.cache_per_mtok),
            };
            return (
              <div className="price-row" key={p.model}>
                <div className="price-model">
                  {p.model}
                  {p.builtin && <span className="builtin-tag">内置</span>}
                </div>
                <div className="price-inputs">
                  <label>
                    输入
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
                    输出
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
                    缓存
                    <input
                      type="number"
                      step="0.01"
                      value={e.cache}
                      onChange={(ev) =>
                        setEditing({ ...editing, [p.model]: { ...e, cache: ev.target.value } })
                      }
                    />
                  </label>
                </div>
                <div className="price-actions">
                  <button onClick={() => savePrice(p.model, e.in, e.out, e.cache)}>保存</button>
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
              输入
              <input
                type="number"
                step="0.01"
                value={draftPrice.in}
                onChange={(e) => setDraftPrice({ ...draftPrice, in: e.target.value })}
              />
            </label>
            <label>
              输出
              <input
                type="number"
                step="0.01"
                value={draftPrice.out}
                onChange={(e) => setDraftPrice({ ...draftPrice, out: e.target.value })}
              />
            </label>
            <label>
              缓存
              <input
                type="number"
                step="0.01"
                value={draftPrice.cache}
                onChange={(e) => setDraftPrice({ ...draftPrice, cache: e.target.value })}
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
