import { useEffect, useRef } from "react";

// Calls `handler` when the window becomes active again, for example after a
// system permission prompt or System Settings closes.
export function useOnWindowFocus(handler: () => void) {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    const onFocus = () => latest.current();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);
}
