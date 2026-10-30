// Panel entry point: wires Rust events to the UI and UI actions to commands.
// The panel never holds state of its own (CLAUDE.md rule 3).

import { backend, isTauri, type PanelState } from "./api";
import { applyStatic, setLang } from "./i18n";
import { applyAccent } from "./ui/accent";
import { colors } from "./ui/colors";
import { footer } from "./ui/footer";
import { player } from "./ui/player";
import { Spectrum } from "./ui/spectrum";
import { study } from "./ui/study";
import { timer } from "./ui/timer";
import { volume } from "./ui/volume";

const api = await backend;
const panel = document.getElementById("panel") as HTMLElement;
const spectrum = new Spectrum(document.getElementById("spectrum-canvas") as HTMLCanvasElement);
const spectrumCard = document.querySelector(".spectrum") as HTMLElement;

const ui = {
  player: player(() => api.toggle()),
  colors: colors((c) => api.setColor(c)),
  volume: volume((v) => api.setVolume(v)),
  timer: timer((m) => api.setTimer(m)),
  study: study({
    toggle: () => void api.studyToggle(),
    finish: () => void api.studyFinish(),
    setMode: (mode) => void api.studySetMode(mode),
    setTag: (tag) => void api.studySetTag(tag),
    resolveIdle: (keep) => void api.studyResolveIdle(keep),
  }),
  footer: footer(
    () => api.openSettings(),
    () => api.quit(),
  ),
};

let state: PanelState | null = null;
let lang: string | null = null;
let mica: boolean | null = null;

function render(s: PanelState): void {
  state = s;
  if (s.lang !== lang) {
    lang = s.lang;
    setLang(s.lang);
    applyStatic();
  }
  if (s.mica !== mica) {
    mica = s.mica;
    document.documentElement.toggleAttribute("data-mica", s.mica);
    spectrum.refreshPalette();
  }
  // The accent follows the noise (tokens.css), or Windows' own; text stays neutral.
  document.documentElement.dataset.noise = s.color;
  applyAccent(s.appearance.accent === "system" ? s.systemAccent : null);
  spectrumCard.hidden = !s.appearance.spectrum;
  spectrum.setEnabled(s.appearance.spectrum);
  ui.player.render(s);
  ui.colors.render(s);
  ui.volume.render(s);
  ui.timer.render(s);
  ui.study.render(s.study);
  ui.footer.render(s);
  spectrum.setColor(s.color);
  spectrum.setPlaying(s.playing);
}

api.onState(render);
api.onSpectrum((bands) => spectrum.push(bands));
api.onTimerTick((remaining) => {
  if (state) ui.timer.render({ ...state, timerRemaining: remaining });
});
api.onStudyTick((view) => {
  if (state?.study) ui.study.render((state = { ...state, study: view }).study);
});
api.onShown(() => {
  panel.classList.remove("is-closing");
  panel.classList.add("is-open");
  spectrum.start();
  void api.getState().then((s) => s && render(s));
  // Two frames: the first paints, the second runs once it is on screen.
  requestAnimationFrame(() => requestAnimationFrame(() => void api.painted()));
});
api.onHiding(() => {
  panel.classList.remove("is-open");
  panel.classList.add("is-closing");
  spectrum.stop();
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    void api.hide();
  } else if (e.code === "Space" && !(e.target instanceof HTMLButtonElement)) {
    // Buttons handle Space themselves; anywhere else it plays or pauses.
    e.preventDefault();
    void api.toggle();
  }
});

if (isTauri) {
  // A native flyout, not a web page.
  document.addEventListener("contextmenu", (e) => e.preventDefault());
}

const initial = await api.getState();
if (initial) render(initial);
void api.ready();
