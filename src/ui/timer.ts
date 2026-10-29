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
    render(s: PanelState) {
      // The quick choices come from Settings → Timer.
      const chips = root.querySelectorAll<HTMLButtonElement>('[role="radio"]:not([data-value="0"])');
      chips.forEach((b, i) => {
        const n = s.timerPresets[i];
        b.dataset.value = String(n);
        b.textContent = t("timerOption", { n });
      });
      group.set(String(s.timerMinutes));
      out.textContent = label(s);
    },
  };
}
