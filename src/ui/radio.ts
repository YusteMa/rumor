// A segmented control that behaves as a radio group: one tab stop, arrow
// keys move and pick, the checked option is announced.

export interface RadioGroup {
  set(value: string): void;
}

export function radioGroup(root: HTMLElement, onPick: (value: string) => void): RadioGroup {
  const options = [...root.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
  const pick = (b: HTMLButtonElement | undefined) => {
    if (b?.dataset.value !== undefined) onPick(b.dataset.value);
  };

  root.addEventListener("click", (e) => {
    pick((e.target as Element).closest<HTMLButtonElement>('[role="radio"]') ?? undefined);
  });

  root.addEventListener("keydown", (e) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const current = options.indexOf(document.activeElement as HTMLButtonElement);
    const next = options[(current + step + options.length) % options.length];
    next.focus();
    pick(next);
  });

  return {
    set(value) {
      for (const b of options) {
        const on = b.dataset.value === value;
        b.setAttribute("aria-checked", String(on));
        b.tabIndex = on ? 0 : -1;
      }
    },
  };
}
