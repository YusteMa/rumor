// Sleep timer: ∞ / 25 / 50 / 90 min, with the countdown next to the label.

import type { PanelState } from "../api";
import { clock, t } from "../i18n";
import { radioGroup } from "./radio";

export function timer(onPick: (minutes: number) => void) {
  const root = document.getElementById("timers") as HTMLElement;
  const out = document.getElementById("timer-out") as HTMLElement;
  const group = radioGroup(root, (v) => onPick(Number(v)));

  const label = (s: Pick<PanelState, "timerMinutes" | "timerRemaining">) => {
    if (s.timerRemaining !== null) return t("timerLeft", { time: clock(s.timerRemaining) });
    if (s.timerMinutes > 0) return t("timerArmed", { n: s.timerMinutes });
    return t("timerNever");
  };

  return {
    /** Option labels depend on the language. */
    localise() {
      root.querySelectorAll<HTMLButtonElement>('[data-value]:not([data-value="0"])').forEach((b) => {
        b.textContent = t("timerOption", { n: Number(b.dataset.value) });
      });
    },
    render(s: PanelState) {
      group.set(String(s.timerMinutes));
      out.textContent = label(s);
    },
  };
}
