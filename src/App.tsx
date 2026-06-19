// App: the widget window root. Holds the current widget shape and whether the
// detail panel is open. Window is user-resizable: decorations are off so we
// render our own thin resize edges that call startResize(direction).

import { useEffect, useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import "./App.css";
import { CardWidget } from "./components/widgets/CardWidget";
import { PillWidget } from "./components/widgets/PillWidget";
import { GaugeWidget } from "./components/widgets/GaugeWidget";
import { DetailPanel } from "./components/DetailPanel";
import { SettingsPage } from "./components/SettingsPage";
import { useTodaySummary } from "./hooks/useTodaySummary";

type WidgetShape = "card" | "pill" | "gauge";

function loadShape(): WidgetShape {
  const s = localStorage.getItem("widget_shape");
  return s === "pill" || s === "gauge" || s === "card" ? s : "card";
}

// Thin edges + corners that drive native window resizing on a frameless
// window. Each calls startResizeDragging with the directions it represents.
function startResize(dir: "East" | "North" | "NorthEast" | "NorthWest" | "South" | "SouthEast" | "SouthWest" | "West") {
  getCurrentWindow().startResizeDragging(dir).catch(() => {});
}

function ResizeHandles() {
  return (
    <>
      <div className="rz rz-top" onMouseDown={() => startResize("North")} />
      <div className="rz rz-bottom" onMouseDown={() => startResize("South")} />
      <div className="rz rz-left" onMouseDown={() => startResize("West")} />
      <div className="rz rz-right" onMouseDown={() => startResize("East")} />
      <div className="rz rz-tl" onMouseDown={() => startResize("NorthWest")} />
      <div className="rz rz-tr" onMouseDown={() => startResize("NorthEast")} />
      <div className="rz rz-bl" onMouseDown={() => startResize("SouthWest")} />
      <div className="rz rz-br" onMouseDown={() => startResize("SouthEast")} />
    </>
  );
}

function App() {
  const [shape, setShape] = useState<WidgetShape>(loadShape);
  const [detailOpen, setDetailOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { summary, loading } = useTodaySummary();

  // Detail panel dispatches this custom event to open settings.
  useEffect(() => {
    const h = () => setSettingsOpen(true);
    window.addEventListener("open-settings", h);
    return () => window.removeEventListener("open-settings", h);
  }, []);

  if (settingsOpen) {
    return (
      <>
        <SettingsPage
          onBack={() => {
            setSettingsOpen(false);
            if (!detailOpen) {
              getCurrentWindow().setSize(new LogicalSize(240, 300)).catch(() => {});
            }
          }}
          onShapeChange={(s) => setShape(s)}
        />
        <ResizeHandles />
      </>
    );
  }

  if (detailOpen) {
    return (
      <>
        <DetailPanel
          onClose={() => {
            setDetailOpen(false);
            getCurrentWindow().setSize(new LogicalSize(240, 300)).catch(() => {});
          }}
        />
        <ResizeHandles />
      </>
    );
  }

  const open = () => {
    setDetailOpen(true);
    getCurrentWindow()
      .setSize(new LogicalSize(360, 560))
      .catch(() => {});
  };

  const cycleShape = () => {
    const next: WidgetShape =
      shape === "card" ? "pill" : shape === "pill" ? "gauge" : "card";
    setShape(next);
    localStorage.setItem("widget_shape", next);
    // Nudge to a sensible default size for the new shape, but the user can
    // then drag it to anything they like.
    const sizes = {
      card: { width: 240, height: 300 },
      pill: { width: 340, height: 90 },
      gauge: { width: 200, height: 240 },
    };
    getCurrentWindow()
      .setSize(new LogicalSize(sizes[next].width, sizes[next].height))
      .catch(() => {});
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
      <ResizeHandles />
    </div>
  );
}

export default App;
