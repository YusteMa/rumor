// The protagonist: play/pause, what is playing, and any notice about why it
// stopped (SPEC §4.5 states: playing, paused, headphones disconnected, no
// audio device).

import type { PanelState } from "../api";
import { t } from "../i18n";

const TITLES = { pink: "titlePink", brown: "titleBrown", white: "titleWhite" } as const;
const NOTICES = {
  headphonesDisconnected: "pausedHeadphones",
  outputChanged: "pausedOutputChanged",
  noOutput: "pausedNoOutput",
} as const;

export function player(onToggle: () => void) {
  const play = document.getElementById("play") as HTMLButtonElement;
  const title = document.getElementById("title") as HTMLElement;
  const subtitle = document.getElementById("subtitle") as HTMLElement;
  const status = document.getElementById("status-text") as HTMLElement;
  play.addEventListener("click", onToggle);

  return {
    render(s: PanelState) {
      document.body.classList.toggle("is-playing", s.playing);
      play.setAttribute("aria-pressed", String(s.playing));
      play.setAttribute("aria-label", t(s.playing ? "actionPause" : "actionPlay"));
      play.disabled = !s.output && !s.playing;
      title.textContent = t(TITLES[s.color]);
      status.textContent = t(s.playing ? "statusPlaying" : "statusPaused");

      const reason = !s.output ? "noOutput" : s.playing ? null : s.pausedFor;
      // Sleep and lock pauses are expected: no notice for them.
      const notice = reason && reason !== "suspended" && reason !== "locked" ? NOTICES[reason] : null;
      subtitle.textContent = notice ? t(notice) : t("subtitleLive");
      subtitle.classList.toggle("is-notice", notice !== null);
    },
  };
}
