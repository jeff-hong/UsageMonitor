// App: the widget window root. Holds whether the detail panel is open. Window
// is user-resizable: decorations are off so we render our own thin resize edges
// that call startResize(direction).

import { useEffect, useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import "./App.css";
import { CardWidget } from "./components/widgets/CardWidget";
import { DetailPanel } from "./components/DetailPanel";
import { SettingsPage } from "./components/SettingsPage";
import { useTodaySummary } from "./hooks/useTodaySummary";

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

  return (
    <div className="widget-root">
      <CardWidget summary={summary} loading={loading} onOpenDetail={open} />
      <ResizeHandles />
    </div>
  );
}

export default App;
