// Shared data hook for the widget views: fetches today's summary on mount and
// refreshes periodically. Retries on failure and never gets stuck loading so
// the widget always shows whatever data it has.

import { useEffect, useRef, useState } from "react";
import { api, type Summary } from "../lib/api";

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
          // Backend not ready yet (still indexing, etc.). Retry sooner than the
          // normal interval so the widget recovers without a long wait.
          console.warn("getTodaySummary failed, will retry:", err);
          if (aliveRef.current) {
            setTimeout(refresh, RETRY_MS);
          }
        }
      );

    refresh();
    const id = setInterval(refresh, REFRESH_MS);
    return () => {
      aliveRef.current = false;
      clearInterval(id);
    };
  }, []);

  return { summary, loading };
}
