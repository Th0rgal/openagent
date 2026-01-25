import { useEffect, useRef, useCallback, useState } from 'react';

interface UseVisibilityPollingOptions {
  /** Polling interval in ms */
  interval: number;
  /** Whether polling is enabled */
  enabled?: boolean;
  /** Whether to pause when tab is hidden (default: true) */
  pauseWhenHidden?: boolean;
  /** Whether to run immediately on mount (default: true) */
  runImmediately?: boolean;
}

/**
 * A polling hook that automatically pauses when the tab is hidden.
 * This saves resources and reduces unnecessary network requests.
 *
 * Usage:
 * ```tsx
 * useVisibilityPolling(fetchData, { interval: 5000 });
 * ```
 */
export function useVisibilityPolling(
  callback: () => void | Promise<void>,
  options: UseVisibilityPollingOptions
) {
  const {
    interval,
    enabled = true,
    pauseWhenHidden = true,
    runImmediately = true,
  } = options;

  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const callbackRef = useRef(callback);

  // Keep callback ref up to date
  useEffect(() => {
    callbackRef.current = callback;
  }, [callback]);

  const startPolling = useCallback(() => {
    if (intervalRef.current) return; // Already running
    intervalRef.current = setInterval(() => {
      callbackRef.current();
    }, interval);
  }, [interval]);

  const stopPolling = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      stopPolling();
      return;
    }

    // Run immediately if requested
    if (runImmediately) {
      callbackRef.current();
    }

    // Start polling
    startPolling();

    // Handle visibility changes
    const handleVisibilityChange = () => {
      if (!pauseWhenHidden) return;

      if (document.hidden) {
        stopPolling();
      } else {
        // Run immediately when tab becomes visible again
        callbackRef.current();
        startPolling();
      }
    };

    if (pauseWhenHidden) {
      document.addEventListener('visibilitychange', handleVisibilityChange);
    }

    return () => {
      stopPolling();
      if (pauseWhenHidden) {
        document.removeEventListener('visibilitychange', handleVisibilityChange);
      }
    };
  }, [enabled, pauseWhenHidden, runImmediately, startPolling, stopPolling]);

  return { startPolling, stopPolling };
}

/**
 * A simpler hook that just returns whether the document is currently visible.
 * Useful for conditionally skipping expensive operations.
 */
export function useDocumentVisible(): boolean {
  const [visible, setVisible] = useState(() =>
    typeof document !== 'undefined' ? !document.hidden : true
  );

  useEffect(() => {
    const handleVisibilityChange = () => {
      setVisible(!document.hidden);
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => document.removeEventListener('visibilitychange', handleVisibilityChange);
  }, []);

  return visible;
}
