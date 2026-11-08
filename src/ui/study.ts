// The study card (SPEC §4.3b): the sphere with the time inside, and beside
// it where the session stands and its buttons; today and this week below,
// with Statistics. With "Modo estudio" off it doesn't exist (SPEC §4.3c).

import type { StudyMode, StudyView } from "../api";
import { clock, t } from "../i18n";
import { dial, type DialView } from "./dial";
import { dropdown } from "./dropdown";
import { radioGroup } from "./radio";

/** "3 h 10 min" or "10 min". */
export function duration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor(secs / 60) % 60;
  return h > 0 ? t("durHours", { h, m }) : t("durMinutes", { m });
}

/** "15:12" in the user's locale. */
function wallClock(ms: number): string {
  return new Intl.DateTimeFormat(document.documentElement.lang || undefined, { hour: "2-digit", minute: "2-digit" }).format(ms);
}

/** What the sphere and its column say, from the study state. Shared with
 *  the mini clock, so both always agree. */
export function face(s: StudyView): DialView & { time: string; sub: string; title: string; detail: string } {
  const session = s.run !== "stopped";
  const paused = s.run === "paused";
  const isBreak = session && s.phase !== "work";
  const breakName = t(s.phase === "longBreak" ? "phaseLongBreak" : "phaseBreak");
  const work = t("phaseWorkOf", { n: s.cycle, total: s.cycles });
  const since = s.startedAt !== null ? t("studySince", { time: wallClock(s.startedAt) }) : "";
  const look = !session ? "idle" : paused ? "paused" : isBreak ? "break" : "running";

  if (!session) {
    const pomodoro = s.mode === "pomodoro";
    return {
      look,
      progress: null,
      trail: null,
      time: clock(pomodoro ? s.workSecs : 0),
      sub: t(pomodoro ? "modePomodoro" : "modeStopwatch"),
      title: "",
      detail: "",
    };
  }
  if (s.phaseLeftSecs !== null && s.phaseSecs) {
    // Pomodoro: the sphere is the phase.
    const next = s.phase === "longBreak" ? 1 : s.cycle + 1;
    return {
      look,
      progress: 1 - s.phaseLeftSecs / s.phaseSecs,
      trail: null,
      time: clock(s.phaseLeftSecs),
      sub: paused ? t("studyPaused") : isBreak ? breakName : t("dialOf", { total: clock(s.phaseSecs) }),
      title: paused ? t("studyPaused") : isBreak ? breakName : work,
      detail: paused
        ? isBreak
          ? breakName
          : work
        : isBreak
          ? t("studyNext", { n: next, total: s.cycles })
          : t("studySession", { time: clock(s.workedSecs) }),
    };
  }
  // Stopwatch: the trail goes round once a minute.
  return {
    look,
    progress: null,
    trail: s.workedSecs,
    time: clock(s.workedSecs),
    sub: paused ? t("studyPaused") : t("modeStopwatch"),
    title: paused ? t("studyPaused") : t("studyStudying"),
    detail: since,
  };
}

export interface StudyActions {
  toggle(): void;
  finish(): void;
  setMode(mode: StudyMode): void;
  setTag(tag: number | null): void;
  resolveIdle(keep: boolean): void;
  openStats(): void;
  /** Noise and stopwatch together or apart, for this session. */
  setLinked(linked: boolean): void;
}

export function study(actions: StudyActions) {
  const $ = (id: string) => document.getElementById(id) as HTMLElement;
  const root = $("study");
  const modes = $("study-modes");
  const body = $("study-body");
  const sphere = $("study-dial");
  const time = $("study-time");
  const sub = $("study-sub");
  const phase = $("study-phase");
  const title = $("study-title");
  const detail = $("study-detail");
  const play = $("study-play") as HTMLButtonElement;
  const playText = $("study-play-text");
  const finish = $("study-finish") as HTMLButtonElement;
  const totals = $("study-totals");
  const foot = $("study-foot");
  const tagSlot = $("study-tag");
  const idle = $("study-idle");
  const idleText = $("study-idle-text");
  const link = $("study-link") as HTMLButtonElement;

  const ring = dial(sphere, "panel");
  link.addEventListener("click", () => actions.setLinked(link.getAttribute("aria-pressed") !== "true"));
  const modeGroup = radioGroup(modes, (v) => actions.setMode(v as StudyMode));
  play.addEventListener("click", () => actions.toggle());
  finish.addEventListener("click", () => actions.finish());
  $("study-stats").addEventListener("click", () => actions.openStats());
  $("idle-keep").addEventListener("click", () => actions.resolveIdle(true));
  $("idle-discount").addEventListener("click", () => actions.resolveIdle(false));

  // "" is no tag; ids travel as strings through the dropdown.
  const tags = dropdown<string>([], (v) => actions.setTag(v ? Number(v) : null));
  tags.node.classList.add("compact");
  tags.node.setAttribute("aria-label", t("studyTag"));
  tagSlot.append(tags.node);
  let tagKey = "";

  return {
    render(s: StudyView | null, linked: boolean) {
      root.hidden = s === null;
      if (!s) return;
      // The noise above and the stopwatch here: one link between them.
      link.setAttribute("aria-pressed", String(linked));
      link.setAttribute("aria-label", t("studyLinkLabel"));
      link.title = t(linked ? "studyLinkedTip" : "studyApartTip");
      const session = s.run !== "stopped";
      const running = s.run === "running";
      root.classList.toggle("is-running", running);

      const f = face(s);
      ring.render(f);
      time.textContent = f.time;
      time.classList.toggle("has-hours", f.time.split(":").length > 2);
      sub.textContent = f.sub;
      sphere.setAttribute("aria-label", `${f.time} · ${f.sub}`);

      // Before a session: the mode; during one: where it stands.
      modes.hidden = session;
      modeGroup.set(s.mode);
      phase.hidden = !session;
      title.textContent = f.title;
      detail.textContent = f.detail;

      playText.textContent = t(running ? "studyHold" : session ? "studyGoOn" : "studyGo");
      play.setAttribute("aria-label", t(running ? "studyPause" : session ? "studyResume" : "studyStart"));
      finish.hidden = !session;
      finish.setAttribute("aria-label", t("studyFinish"));
      finish.title = t("studyFinish");
      totals.textContent = t("studyTotals", { today: duration(s.todaySecs), week: duration(s.weekSecs) });

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
