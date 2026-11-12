import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";

import logo from "../assets/logo.png";
import { pickQuote } from "../quotes";

/** The MIT license text, in the project's repository. */
export const LICENSE_URL = "https://github.com/TheComerator/Endless-Sky-Modman/blob/main/LICENSE";

/**
 * The logo, version number and license link. Shown when the app starts and again from the
 * "About" button. It stays until the user clicks anywhere (or presses Escape); there is no
 * timer, so it can't vanish before it's been read. The license link is the one exception: it
 * opens the browser and leaves the card alone.
 */
export function AboutCard({ onClose }: { onClose: () => void }) {
  // `null` while loading or if it can't be read; the card just leaves the version out then.
  const [version, setVersion] = useState<string | null>(null);
  // Chosen once per time the card opens (it is unmounted when closed), so it doesn't change while read.
  const [quote] = useState(pickQuote);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop splash-backdrop" onClick={onClose}>
      <div
        className="modal about-card"
        role="dialog"
        aria-label="About Endless Sky Mod Manager"
      >
        <img className="about-logo" src={logo} alt="" width={380} height={380} />
        <h2>Endless Sky Mod Manager</h2>
        {version && <p className="about-version">Version {version}</p>}
        <p className="muted">An unofficial tool for installing and managing Endless Sky plugins.</p>
        <figure className="about-quote">
          <blockquote>“{quote.text}”</blockquote>
          <figcaption>— {quote.credit}</figcaption>
        </figure>
        <p className="muted about-credit">
          Quotes from Endless Sky, © Michael Zahniser and contributors (GPL-3+).
        </p>
        <p className="about-license">
          <a
            href={LICENSE_URL}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation(); // opening the license shouldn't also close the card
              // A failure to open the browser isn't worth interrupting anyone over; the link text says where it goes.
              void openUrl(LICENSE_URL).catch(() => undefined);
            }}
          >
            MIT license
          </a>
        </p>
      </div>
    </div>
  );
}
