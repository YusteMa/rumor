// Ambients (SPEC §3.5): real recordings, downloaded the first time they are
// picked. One radio group, like the colours; the word under each name says
// where it was recorded, or what a click will do (download, retry) and how
// far a download has got.

import type { AmbientView, PanelState } from "../api";
import { t } from "../i18n";
import { radioGroup } from "./radio";

const TAGS = {
  "rain-soft": "tagRainSoft",
  rain: "tagRain",
  storm: "tagStorm",
  downpour: "tagDownpour",
} as const;

const HINTS = {
  "rain-soft": "hintRainSoft",
  rain: "hintRain",
  storm: "hintStorm",
  downpour: "hintDownpour",
} as const;

/** What a ready ambient sounds like: the player's subtitle and a tooltip. */
export function ambientHint(id: string): string {
  return t(HINTS[id as keyof typeof HINTS] ?? "hintRain");
}

/** The word under the name: what it is once ready, else what a click does. */
function note(v: AmbientView): string {
  switch (v.status) {
    case "missing":
      return t("ambientSize", { mb: Math.round(v.bytes / 1e6) });
    case "downloading":
      return t("ambientProgress", { pct: Math.floor(v.progress * 100) });
    case "failed":
      return t("ambientRetry");
    case "unavailable":
      return t("ambientSoon");
    case "ready":
      return t(TAGS[v.id as keyof typeof TAGS] ?? "tagRain");
  }
}

export function ambients(onPick: (id: string) => void) {
  const root = document.getElementById("ambients") as HTMLElement;
  const group = radioGroup(root, onPick);
  const buttons = new Map(
    [...root.querySelectorAll<HTMLButtonElement>("[data-value]")].map((b) => [b.dataset.value!, b]),
  );

  return {
    render(s: PanelState) {
      group.set(s.ambient ?? "");
      for (const v of s.ambients) {
        const b = buttons.get(v.id);
        if (!b) continue;
        b.dataset.status = v.status;
        b.disabled = v.status === "unavailable";
        b.style.setProperty("--progress", String(v.progress));
        b.querySelector(".hint")!.textContent = note(v);
        b.title =
          v.status === "missing"
            ? t("ambientDownload", { mb: Math.round(v.bytes / 1e6) })
            : ambientHint(v.id);
      }
    },
  };
}
