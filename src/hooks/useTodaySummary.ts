// Shared data hook for the widget views: fetches today's summary on mount and
// refreshes periodically. Also re-fetches the moment the backend finishes its
// initial index, so the widget doesn't show partial data (e.g. only Claude)
// for up to a full refresh interval after launch.

import { useEffect, useRef, useState } from "react";
import { api, onIndexProgress, type Summary } from "../lib/api";

const REFRESH_MS = 30_000;
const RETRY_MS = 3_000;

export function useTodaySummary(): { summary: Summary | null; loading: boolean } {
  const [summary, setSummary] = useState<Summary | null>(null);
  const [loading, setLoading] = useState(true);
  const aliveRef = useRef(true);

  useEffect(() => {
    aliveRef.current = true;

    const refresh = () =>
      api.getTodaySummary().then(
        (s) => {
          if (aliveRef.current) {
            setSummary(s);
            setLoading(false);
          }
        },
        (err) => {
          // Backend not ready yet. Retry sooner than the normal interval.
          console.warn("getTodaySummary failed, will retry:", err);
          if (aliveRef.current) {
            setTimeout(refresh, RETRY_MS);
          }
        }
      );

    refresh();
    const id = setInterval(refresh, REFRESH_MS);

    // Re-fetch as soon as the backend signals the initial index is done. During
    // indexing the summary can be partial (e.g. only Claude rows in yet), so we
    // need a fresh read the instant the full index completes.
    const stop = onIndexProgress((p) => {
      if (p.done && aliveRef.current) {
        refresh();
      }
    }).catch(() => null as unknown as () => void);

    return () => {
      aliveRef.current = false;
      clearInterval(id);
      stop.then((fn) => fn?.());
    };
  }, []);

  return { summary, loading };
}
