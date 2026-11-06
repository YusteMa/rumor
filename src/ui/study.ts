// The study card (SPEC §4.3b): play/pause inside a thin ring that fills
// with the Pomodoro phase, the session time in large figures, today and this
// week in small ones, and finishing the session. With "Modo estudio" off it
// doesn't exist (SPEC §4.3c).

import type { StudyMode, StudyView } from "../api";
import { clock, t } from "../i18n";
import { dropdown } from "./dropdown";
import { radioGroup } from "./radio";

/** "3 h 10 min" or "10 min". */
export function duration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor(secs / 60) % 60;
  return h > 0 ? t("durHours", { h, m }) : t("durMinutes", { m });
}

export interface StudyActions {
  toggle(): void;
  finish(): void;
  setMode(mode: StudyMode): void;
  setTag(tag: number | null): void;
  resolveIdle(keep: boolean): void;
  openStats(): void;
}

export function study(actions: StudyActions) {
  const root = document.getElementById("study") as HTMLElement;
  const modes = document.getElementById("study-modes") as HTMLElement;
  const phase = document.getElementById("study-phase") as HTMLElement;
  const body = document.getElementById("study-body") as HTMLElement;
  const play = document.getElementById("study-play") as HTMLButtonElement;
  const ring = document.getElementById("study-ring") as unknown as SVGCircleElement;
  const time = document.getElementById("study-time") as HTMLElement;
  const totals = document.getElementById("study-totals") as HTMLElement;
  const foot = document.getElementById("study-foot") as HTMLElement;
  const finish = document.getElementById("study-finish") as HTMLButtonElement;
  const tagSlot = document.getElementById("study-tag") as HTMLElement;
  const idle = document.getElementById("study-idle") as HTMLElement;
  const idleText = document.getElementById("study-idle-text") as HTMLElement;

  const modeGroup = radioGroup(modes, (v) => actions.setMode(v as StudyMode));
  play.addEventListener("click", () => actions.toggle());
  finish.addEventListener("click", () => actions.finish());
  (document.getElementById("study-stats") as HTMLButtonElement).addEventListener("click", () => actions.openStats());
  (document.getElementById("idle-keep") as HTMLButtonElement).addEventListener("click", () =>
    actions.resolveIdle(true),
  );
  (document.getElementById("idle-discount") as HTMLButtonElement).addEventListener("click", () =>
    actions.resolveIdle(false),
  );

  // "" is no tag; ids travel as strings through the dropdown.
  const tags = dropdown<string>([], (v) => actions.setTag(v ? Number(v) : null));
  tags.node.classList.add("compact");
  tags.node.setAttribute("aria-label", t("studyTag"));
  tagSlot.append(tags.node);
  let tagKey = "";

  return {
    render(s: StudyView | null) {
      root.hidden = s === null;
      if (!s) return;
      const session = s.run !== "stopped";
      const running = s.run === "running";
      const isBreak = s.phase !== "work";
      root.classList.toggle("is-running", running);
      root.classList.toggle("is-break", session && isBreak);

      // Between sessions: the mode; during one: where it stands.
      modes.hidden = session;
      modeGroup.set(s.mode);
      let where = "";
      if (session && s.mode === "pomodoro") {
        const name = isBreak
          ? t(s.phase === "longBreak" ? "phaseLongBreak" : "phaseBreak")
          : t("phaseWork", { n: s.cycle, total: s.cycles });
        where = s.phaseLeftSecs !== null ? `${name} · ${clock(s.phaseLeftSecs)}` : name;
      }
      if (s.run === "paused") where = where ? `${t("studyPaused")} · ${where}` : t("studyPaused");
      phase.textContent = where;

      const progress =
        s.phaseSecs && s.phaseLeftSecs !== null ? 1 - s.phaseLeftSecs / s.phaseSecs : 0;
      ring.style.strokeDashoffset = String(100 * (1 - progress));
      root.classList.toggle("has-phase", s.phaseSecs !== null);
      play.setAttribute("aria-pressed", String(running));
      play.setAttribute("aria-label", t(running ? "studyPause" : session ? "studyResume" : "studyStart"));
      time.textContent = clock(s.workedSecs);
      totals.textContent = t("studyTotals", { today: duration(s.todaySecs), week: duration(s.weekSecs) });
      finish.disabled = !session;
      finish.setAttribute("aria-label", t("studyFinish"));
      finish.title = t("studyFinish");

      // Tags only appear once there is one.
      tagSlot.hidden = s.tags.length === 0;
      const key = s.tags.map((g) => `${g.id}:${g.name}:${g.color}`).join("|");
      if (key !== tagKey) {
        tagKey = key;
        tags.setOptions([["", t("tagNone")], ...s.tags.map((g): [string, string] => [String(g.id), g.name])]);
      }
      tags.setValue(s.tag === null ? "" : String(s.tag));
      const color = s.tags.find((g) => g.id === s.tag)?.color;
      tags.node.style.setProperty("--tag", color ? `var(--tag-${color})` : "transparent");
      tags.node.classList.toggle("no-tag", !color);

      // "¿Seguías estudiando?" waits here; it never opens the panel itself.
      const asking = s.idleMinutes !== null;
      idle.hidden = !asking;
      body.hidden = asking;
      foot.hidden = asking;
      if (asking) idleText.textContent = t("idleQuestion", { n: s.idleMinutes ?? 0 });
    },
  };
}
