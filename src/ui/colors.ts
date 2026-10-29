// Pink / brown / white, with the colour's slope next to the label.

import type { Color, PanelState } from "../api";
import { t } from "../i18n";
import { radioGroup } from "./radio";
import { SLOPES } from "./spectrum";

export function colors(onPick: (c: Color) => void) {
  const group = radioGroup(document.getElementById("colors") as HTMLElement, (v) => onPick(v as Color));
  const slope = document.getElementById("slope") as HTMLElement;

  return {
    render(s: PanelState) {
      group.set(s.color);
      const n = SLOPES[s.color];
      slope.textContent = t("slope", { n: n === 0 ? "0" : `−${Math.abs(n)}` });
    },
  };
}
