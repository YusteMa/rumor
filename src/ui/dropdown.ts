// A dropdown drawn with the tokens instead of the native <select>, whose list
// WebView2 paints with system colours that don't follow the theme or Mica.
// ARIA "select-only combobox": focus stays on the button, the list is
// announced through aria-activedescendant.
//   Closed: Enter / Space / ↓ / ↑ / Alt+↓ open it; Home / End open it at the
//           ends; typing a letter jumps to the matching option.
//   Open:   ↑ ↓ PgUp PgDn Home End move; Enter / Space / Tab choose;
//           Esc closes without changing; a click outside closes.

const CHEVRON =
  '<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const PAGE = 5;
const TYPEAHEAD_MS = 600;

let uid = 0;

export interface Dropdown<T extends string> {
  /** The button; the list is attached to <body> while open. */
  node: HTMLButtonElement;
  setOptions(options: [T, string][]): void;
  setValue(value: T): void;
}

export function dropdown<T extends string>(options: [T, string][], onPick: (value: T) => void): Dropdown<T> {
  const id = `dd-${++uid}`;
  const node = document.createElement("button");
  node.type = "button";
  node.className = "select";
  node.setAttribute("role", "combobox");
  node.setAttribute("aria-haspopup", "listbox");
  node.setAttribute("aria-expanded", "false");
  node.setAttribute("aria-controls", `${id}-list`);
  const label = document.createElement("span");
  label.className = "select-label";
  node.append(label);
  node.insertAdjacentHTML("beforeend", CHEVRON);

  const list = document.createElement("ul");
  list.id = `${id}-list`;
  list.className = "select-list";
  list.setAttribute("role", "listbox");
  list.tabIndex = -1;

  let items: [T, string][] = [];
  let value: T | null = null;
  let active = -1;
  let typed = "";
  let typedAt = 0;

  const isOpen = () => node.getAttribute("aria-expanded") === "true";
  const indexOf = (v: T | null) => items.findIndex(([x]) => x === v);

  function paintValue(): void {
    const text = items[indexOf(value)]?.[1] ?? "";
    label.textContent = text;
    node.title = text;
  }

  function paintActive(scroll: boolean): void {
    list.querySelectorAll<HTMLElement>("[role=option]").forEach((li, i) => li.classList.toggle("is-active", i === active));
    const li = list.children[active] as HTMLElement | undefined;
    if (li) {
      node.setAttribute("aria-activedescendant", li.id);
      if (scroll) li.scrollIntoView({ block: "nearest" });
    } else node.removeAttribute("aria-activedescendant");
  }

  function fillList(): void {
    list.replaceChildren(
      ...items.map(([v, text], i) => {
        const li = document.createElement("li");
        li.id = `${id}-${i}`;
        li.setAttribute("role", "option");
        li.setAttribute("aria-selected", String(v === value));
        li.textContent = text;
        li.title = text;
        return li;
      }),
    );
  }

  /** Below the button, or above it when it doesn't fit; never off-window. */
  function place(): void {
    const r = node.getBoundingClientRect();
    const gap = parseFloat(getComputedStyle(list).getPropertyValue("--popup-gap")) || 0;
    list.style.minWidth = `${r.width}px`;
    list.style.left = "0px";
    list.style.top = "0px";
    const w = list.offsetWidth;
    const h = list.offsetHeight;
    const vw = document.documentElement.clientWidth;
    const vh = document.documentElement.clientHeight;
    const below = vh - r.bottom - gap * 2;
    const up = h > below && r.top - gap * 2 > below;
    list.classList.toggle("is-up", up);
    list.style.left = `${Math.max(gap, Math.min(r.left, vw - w - gap))}px`;
    list.style.top = `${up ? r.top - gap - h : r.bottom + gap}px`;
  }

  function open(at = indexOf(value)): void {
    if (isOpen() || items.length === 0) return;
    fillList();
    document.body.append(list);
    node.setAttribute("aria-expanded", "true");
    place();
    active = Math.max(0, at);
    paintActive(true);
    list.classList.add("is-open");
    document.addEventListener("pointerdown", onOutside, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    document.addEventListener("scroll", close, true);
  }

  function close(): void {
    if (!isOpen()) return;
    node.setAttribute("aria-expanded", "false");
    node.removeAttribute("aria-activedescendant");
    list.classList.remove("is-open");
    list.remove();
    document.removeEventListener("pointerdown", onOutside, true);
    window.removeEventListener("resize", close);
    window.removeEventListener("blur", close);
    document.removeEventListener("scroll", close, true);
  }

  function choose(i: number): void {
    close();
    const picked = items[i]?.[0];
    if (picked === undefined || picked === value) return;
    value = picked;
    paintValue();
    onPick(picked);
  }

  function move(to: number): void {
    active = Math.max(0, Math.min(items.length - 1, to));
    paintActive(true);
  }

  /** Jumps to the next option starting with what has been typed. */
  function typeahead(key: string): number {
    const now = performance.now();
    typed = now - typedAt > TYPEAHEAD_MS ? key : typed + key;
    typedAt = now;
    const norm = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase();
    // The same letter again cycles through the options that start with it.
    const repeated = [...typed].every((c) => c === typed[0]);
    const want = norm(repeated ? typed[0] : typed);
    // A new letter looks past the current option; a word being typed may stay.
    const from = (isOpen() ? active : indexOf(value)) + (repeated ? 1 : 0);
    const n = items.length;
    for (let k = 0; k < n; k++) {
      const i = (((from + k) % n) + n) % n;
      if (norm(items[i][1]).startsWith(want)) return i;
    }
    return -1;
  }

  function onOutside(e: PointerEvent): void {
    const t = e.target as Node;
    if (!list.contains(t) && !node.contains(t)) close();
  }

  node.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault(); // keeps the focus ring for the keyboard only
    node.focus({ preventScroll: true });
    if (isOpen()) close();
    else open();
  });
  // A click on the row's label lands here: focus, don't open (like <select>).
  node.addEventListener("click", () => node.focus());

  node.addEventListener("keydown", (e) => {
    const k = e.key;
    if (!isOpen()) {
      if (k === "Enter" || k === " " || k === "ArrowDown" || k === "ArrowUp") open();
      else if (k === "Home") open(0);
      else if (k === "End") open(items.length - 1);
      else if (k.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
        const i = typeahead(k);
        if (i >= 0) open(i);
      } else return;
      e.preventDefault();
      return;
    }
    if (k === "ArrowDown") move(active + 1);
    else if (k === "ArrowUp" && e.altKey) choose(active);
    else if (k === "ArrowUp") move(active - 1);
    else if (k === "PageDown") move(active + PAGE);
    else if (k === "PageUp") move(active - PAGE);
    else if (k === "Home") move(0);
    else if (k === "End") move(items.length - 1);
    else if (k === "Enter" || k === " ") choose(active);
    else if (k === "Escape") close();
    else if (k === "Tab") {
      choose(active);
      return; // focus moves on as usual
    } else if (k.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
      const i = typeahead(k);
      if (i >= 0) move(i);
    } else return;
    e.preventDefault();
    e.stopPropagation();
  });

  list.addEventListener("pointermove", (e) => {
    const li = (e.target as HTMLElement).closest<HTMLElement>("[role=option]");
    const i = li ? Array.prototype.indexOf.call(list.children, li) : -1;
    if (i >= 0 && i !== active) {
      active = i;
      paintActive(false);
    }
  });
  // The list never takes focus away from the button.
  list.addEventListener("pointerdown", (e) => e.preventDefault());
  list.addEventListener("click", (e) => {
    const li = (e.target as HTMLElement).closest<HTMLElement>("[role=option]");
    if (li) choose(Array.prototype.indexOf.call(list.children, li));
  });

  const api: Dropdown<T> = {
    node,
    setOptions(next) {
      items = next;
      paintValue();
      if (isOpen()) close();
    },
    setValue(next) {
      if (next === value) return;
      value = next;
      paintValue();
      if (isOpen()) {
        fillList();
        paintActive(false);
      }
    },
  };
  api.setOptions(options);
  return api;
}
