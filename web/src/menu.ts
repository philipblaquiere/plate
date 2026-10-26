/** A small context menu (right-click on the drawing, the explorer, tabs). */

export type MenuItem =
  { label: string; note?: string; checked?: boolean; disabled?: boolean; action: () => void } | "-";

let menuEl: HTMLDivElement | null = null;

export function closeMenu(): void {
  menuEl?.remove();
  menuEl = null;
}

export function openMenu(title: string, items: MenuItem[], x: number, y: number): void {
  closeMenu();
  const menu = document.createElement("div");
  menu.className = "ctx";
  menu.setAttribute("role", "menu");
  if (title) {
    const head = document.createElement("div");
    head.className = "ctx-title";
    head.textContent = title;
    menu.append(head);
  }
  for (const item of items) {
    if (item === "-") {
      menu.append(document.createElement("hr"));
      continue;
    }
    const b = document.createElement("button");
    b.setAttribute("role", item.checked === undefined ? "menuitem" : "menuitemradio");
    if (item.checked !== undefined) b.setAttribute("aria-checked", String(item.checked));
    b.innerHTML = `<span></span>`;
    b.querySelector("span")!.textContent = item.label;
    b.disabled = !!item.disabled;
    if (item.note) {
      const n = document.createElement("span");
      n.className = "ctx-note";
      n.textContent = item.note;
      b.append(n);
    }
    b.addEventListener("click", () => {
      closeMenu();
      item.action();
    });
    menu.append(b);
  }
  document.body.append(menu);
  menuEl = menu;
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(x, window.innerWidth - r.width - 8)}px`;
  menu.style.top = `${Math.min(y, window.innerHeight - r.height - 8)}px`;
  menu.querySelector<HTMLElement>("button")?.focus();
  menu.addEventListener("keydown", (e) => {
    const buttons = [...menu.querySelectorAll<HTMLElement>("button")];
    const i = buttons.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Escape") closeMenu();
    if (e.key === "ArrowDown") (e.preventDefault(), buttons[(i + 1) % buttons.length]?.focus());
    if (e.key === "ArrowUp")
      (e.preventDefault(), buttons[(i - 1 + buttons.length) % buttons.length]?.focus());
  });
}

document.addEventListener("pointerdown", (e) => {
  if (menuEl && !menuEl.contains(e.target as Node)) closeMenu();
});
