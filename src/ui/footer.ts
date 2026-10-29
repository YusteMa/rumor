// Footer: Settings (phase 4), the shortcut, Quit. Also where a taken
// shortcut is mentioned (SPEC §4.5 "atajo ocupado").

import type { PanelState } from "../api";
import { t } from "../i18n";

export function footer(onQuit: () => void) {
  const settings = document.getElementById("settings") as HTMLButtonElement;
  const shortcut = document.getElementById("shortcut") as HTMLElement;
  (document.getElementById("quit") as HTMLButtonElement).addEventListener("click", onQuit);

  return {
    localise() {
      // The Settings window arrives in phase 4.
      settings.title = t("settingsSoon");
    },
    render(s: PanelState) {
      if (s.shortcut && s.shortcutTaken) {
        shortcut.textContent = t("shortcutTaken", { taken: s.shortcutTaken, shortcut: s.shortcut });
      } else if (s.shortcut) {
        shortcut.textContent = t("shortcutHint", { shortcut: s.shortcut });
      } else {
        shortcut.textContent = "";
      }
    },
  };
}
