// App: the widget window root. Holds the current widget shape and whether the
// detail panel is open. Widget shape switching persists to localStorage so the
// user's choice survives restarts (a settings page hook replaces this in p5).

import { useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import "./App.css";
import { CardWidget } from "./components/widgets/CardWidget";
import { PillWidget } from "./components/widgets/PillWidget";
import { GaugeWidget } from "./components/widgets/GaugeWidget";
import { DetailPanel } from "./components/DetailPanel";
import { useTodaySummary } from "./hooks/useTodaySummary";

type WidgetShape = "card" | "pill" | "gauge";

function loadShape(): WidgetShape {
  const s = localStorage.getItem("widget_shape");
  return s === "pill" || s === "gauge" || s === "card" ? s : "card";
}

function App() {
  const [shape, setShape] = useState<WidgetShape>(loadShape);
  const [detailOpen, setDetailOpen] = useState(false);
  const { summary, loading } = useTodaySummary();

  // When the detail panel is open, the window grows; Tauri resizes it for us
  // in a later phase. For now both views render in the same window.
  if (detailOpen) {
    return (
      <DetailPanel
        onClose={() => {
          setDetailOpen(false);
          // Shrink back to widget size.
          getCurrentWindow()
            .setSize(new LogicalSize(240, 300))
            .catch(() => {});
        }}
      />
    );
  }

  const open = () => {
    setDetailOpen(true);
    getCurrentWindow()
      .setSize(new LogicalSize(360, 560))
      .catch(() => {});
  };

  const cycleShape = () => {
    const next: WidgetShape = shape === "card" ? "pill" : shape === "pill" ? "gauge" : "card";
    setShape(next);
    localStorage.setItem("widget_shape", next);
    // Resize to fit the new shape.
    const sizes = {
      card: { width: 240, height: 300 },
      pill: { width: 320, height: 80 },
      gauge: { width: 180, height: 220 },
    };
    getCurrentWindow().setSize(new LogicalSize(sizes[next].width, sizes[next].height)).catch(() => {});
  };

  return (
    <div className="widget-root">
      {shape === "card" && (
        <CardWidget summary={summary} loading={loading} onOpenDetail={open} />
      )}
      {shape === "pill" && (
        <PillWidget summary={summary} loading={loading} onOpenDetail={open} />
      )}
      {shape === "gauge" && (
        <GaugeWidget summary={summary} loading={loading} onOpenDetail={open} />
      )}
      <div className="shape-cycle" onClick={cycleShape} title="切换悬浮窗样式">
        ⇄
      </div>
    </div>
  );
}

export default App;
