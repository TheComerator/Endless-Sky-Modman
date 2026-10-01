import { useCallback, useRef, useState } from "react";

export type ToastKind = "info" | "success" | "warn" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

export type Notify = (kind: ToastKind, text: string) => void;

const LIFETIME_MS: Record<ToastKind, number> = {
  info: 5000,
  success: 5000,
  warn: 9000,
  error: 12000,
};

export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((all) => all.filter((t) => t.id !== id));
  }, []);

  const notify: Notify = useCallback(
    (kind, text) => {
      const id = nextId.current++;
      setToasts((all) => [...all, { id, kind, text }]);
      window.setTimeout(() => dismiss(id), LIFETIME_MS[kind]);
    },
    [dismiss],
  );

  return { toasts, notify, dismiss };
}
