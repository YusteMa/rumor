// Volume slider. Sends at most once per frame while dragging and ignores
// echoes from Rust until released, so the thumb never fights the hand.

import type { PanelState } from "../api";
import { t } from "../i18n";

export function volume(onChange: (v: number) => void) {
  const input = document.getElementById("volume") as HTMLInputElement;
  const out = document.getElementById("volume-out") as HTMLOutputElement;
  let dragging = false;
  let pending: number | null = null;
  let raf = 0;

  const paint = (percent: number) => {
    input.style.setProperty("--fill", `${percent}%`);
    out.textContent = t("percent", { n: percent });
  };

  input.addEventListener("input", () => {
    paint(input.valueAsNumber);
    pending = input.valueAsNumber / 100;
    raf ||= requestAnimationFrame(() => {
      raf = 0;
      if (pending !== null) onChange(pending);
      pending = null;
    });
  });
  input.addEventListener("pointerdown", () => (dragging = true));
  for (const end of ["pointerup", "pointercancel", "change"]) {
    input.addEventListener(end, () => (dragging = false));
  }

  return {
    render(s: PanelState) {
      if (dragging) return;
      const percent = Math.round(s.volume * 100);
      input.value = String(percent);
      paint(percent);
    },
  };
}
