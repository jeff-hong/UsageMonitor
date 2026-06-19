// Shared data hook for the widget views: fetches today's summary on mount and
// refreshes every minute so the floating window stays current without waiting
// for the backend's 30s indexer tick.

import { useEffect, useState } from "react";
import { api, type Summary } from "../lib/api";

export function useTodaySummary(): { summary: Summary | null; loading: boolean } {
  const [summary, setSummary] = useState<Summary | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    const refresh = () =>
      api.getTodaySummary().then((s) => {
        if (alive) {
          setSummary(s);
          setLoading(false);
        }
      });
    refresh();
    const id = setInterval(refresh, 60_000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  return { summary, loading };
}
