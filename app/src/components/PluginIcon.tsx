import { useEffect, useRef, useState } from "react";

import { api } from "../api";

// One request per icon URL for the app's lifetime; the backend also caches on disk forever
// (icon URLs embed the plugin version, decision I).
const cache = new Map<string, Promise<string | null>>();

function loadIcon(url: string): Promise<string | null> {
  let pending = cache.get(url);
  if (!pending) {
    pending = api.getIcon(url).catch(() => null);
    cache.set(url, pending);
  }
  return pending;
}

/** Catalog icon, fetched only once it scrolls into view; a generic planet otherwise. */
export function PluginIcon({ url, size = 48 }: { url: string | null | undefined; size?: number }) {
  const [src, setSrc] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setSrc(null);
    const el = ref.current;
    if (!url || !el) return;
    let alive = true;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        observer.disconnect();
        void loadIcon(url).then((data) => alive && setSrc(data));
      }
    });
    observer.observe(el);
    return () => {
      alive = false;
      observer.disconnect();
    };
  }, [url]);

  return (
    <div className="plugin-icon" ref={ref} style={{ width: size, height: size }}>
      {src ? (
        <img src={src} alt="" width={size} height={size} />
      ) : (
        <svg viewBox="0 0 48 48" width={size} height={size} aria-hidden="true">
          <circle cx="24" cy="24" r="11" className="planet" />
          <ellipse cx="24" cy="24" rx="20" ry="6" className="ring" transform="rotate(-20 24 24)" />
        </svg>
      )}
    </div>
  );
}
