// Footer: Settings, the shortcut, Quit. Also where a taken
// shortcut is mentioned (SPEC §4.5 "atajo ocupado"), and where a newer
// version offers itself, in the shortcut's place.

import type { PanelState } from "../api";
import { t } from "../i18n";

export function footer(onSettings: () => void, onQuit: () => void, onUpdate: () => void) {
  const shortcut = document.getElementById("shortcut") as HTMLElement;
  const update = document.getElementById("update") as HTMLButtonElement;
  const updateText = document.getElementById("update-text") as HTMLElement;
  (document.getElementById("settings") as HTMLButtonElement).addEventListener("click", onSettings);
  (document.getElementById("quit") as HTMLButtonElement).addEventListener("click", onQuit);
  update.addEventListener("click", onUpdate);

  return {
    render(s: PanelState) {
      const u = s.update;
      update.hidden = !u;
      shortcut.hidden = !!u;
      if (u) {
        updateText.textContent = t(
          u.phase === "installing" ? "updateInstalling" : u.phase === "failed" ? "updateRetry" : "updateNow",
        );
        update.disabled = u.phase === "installing";
        update.title = t(u.phase === "failed" ? "updateFailed" : "updateHint", { v: u.version });
        return;
      }
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
