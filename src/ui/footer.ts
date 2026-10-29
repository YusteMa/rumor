// Footer: Settings, the shortcut, Quit. Also where a taken
// shortcut is mentioned (SPEC §4.5 "atajo ocupado").

import type { PanelState } from "../api";
import { t } from "../i18n";

export function footer(onSettings: () => void, onQuit: () => void) {
  const shortcut = document.getElementById("shortcut") as HTMLElement;
  (document.getElementById("settings") as HTMLButtonElement).addEventListener("click", onSettings);
  (document.getElementById("quit") as HTMLButtonElement).addEventListener("click", onQuit);

  return {
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
