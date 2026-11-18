// Noise | Ambients (SPEC §3.5): one block for the sound, two tabs. A tab only
// changes what the panel shows, never what plays; picking a colour or an
// ambient does that. The panel opens on whatever is playing, and a dot on the
// other tab says the sound is over there.

import type { PanelState } from "../api";
import { t } from "../i18n";

type Tab = "noise" | "ambients";

export function soundTabs() {
  const list = document.getElementById("sound-tabs") as HTMLElement;
  const tabs = [...list.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
  const panels: Record<Tab, HTMLElement> = {
    noise: document.getElementById("colors") as HTMLElement,
    ambients: document.getElementById("ambients") as HTMLElement,
  };
  let shown: Tab = "noise";
  let playing: Tab | null = null;

  const show = (tab: Tab) => {
    shown = tab;
    for (const b of tabs) {
      const on = b.dataset.tab === tab;
      b.setAttribute("aria-selected", String(on));
      b.tabIndex = on ? 0 : -1;
      b.toggleAttribute("data-live", b.dataset.tab === playing && !on);
    }
    for (const [key, panel] of Object.entries(panels)) panel.hidden = key !== tab;
    list.closest(".field")?.setAttribute("data-tab", tab);
  };

  list.addEventListener("click", (e) => {
    const b = (e.target as Element).closest<HTMLButtonElement>('[role="tab"]');
    if (b?.dataset.tab) show(b.dataset.tab as Tab);
  });

  list.addEventListener("keydown", (e) => {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const i = tabs.findIndex((b) => b.dataset.tab === shown);
    const next = tabs[(i + step + tabs.length) % tabs.length];
    next.focus();
    show(next.dataset.tab as Tab);
  });

  show(shown);

  return {
    render(s: PanelState) {
      list.setAttribute("aria-label", t("labelSound"));
      const now: Tab = s.ambient ? "ambients" : "noise";
      // Follow the sound when it moves (a pick here, the tray, a restore);
      // otherwise leave the tab the user is looking at alone.
      if (now !== playing) {
        playing = now;
        show(now);
      } else {
        show(shown);
      }
    },
  };
}
