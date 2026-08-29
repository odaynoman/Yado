import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

const WEEKS = 20;
const DAYS = WEEKS * 7;

interface DayCount {
  date: string;
  count: number;
}
interface AppCount {
  app: string;
  count: number;
}
interface Stats {
  today: number;
  total: number;
  streak: number;
  heatmap: DayCount[];
  top_apps: AppCount[];
}
interface WidgetConfig {
  mode: "all" | "allowlist";
  apps: string[];
  watch_folders: string[] | null;
}

function fmtDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function renderDate(): void {
  const el = document.getElementById("today-date");
  if (el) {
    el.textContent = new Date().toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
    });
  }
}

function buildCells(): { cell: HTMLDivElement; date: string }[] {
  const grid = document.getElementById("heatmap")!;
  grid.innerHTML = "";
  const today = new Date();
  const start = new Date(today);
  start.setDate(start.getDate() - (DAYS - 1));

  const pad = (start.getDay() + 6) % 7;
  for (let i = 0; i < pad; i++) {
    const filler = document.createElement("div");
    filler.className = "cell filler";
    grid.appendChild(filler);
  }
  const cells: { cell: HTMLDivElement; date: string }[] = [];
  for (let i = 0; i < DAYS; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    const cell = document.createElement("div");
    cell.className = "cell";
    cell.dataset.level = "0";
    grid.appendChild(cell);
    cells.push({ cell, date: fmtDate(d) });
  }
  return cells;
}

const cells = buildCells();

function levelFor(count: number, max: number): number {
  if (count <= 0) return 0;
  const scale = Math.max(4, max);
  return Math.min(4, Math.ceil((count * 4) / scale));
}

function renderApps(apps: AppCount[]): void {
  const list = document.getElementById("apps-list")!;
  list.innerHTML = "";
  if (!apps.length) {
    list.innerHTML = '<span class="empty">No activity tracked yet</span>';
    return;
  }
  const max = Math.max(...apps.map((a) => a.count));
  for (const a of apps) {
    const row = document.createElement("div");
    row.className = "app-row";
    const name = document.createElement("span");
    name.className = "app-name";
    name.textContent = a.app.replace(/\.exe$/i, "");
    const barWrap = document.createElement("div");
    barWrap.className = "app-bar";
    const bar = document.createElement("div");
    bar.className = "app-bar-fill";
    bar.style.width = `${Math.max(6, (a.count / max) * 100)}%`;
    barWrap.appendChild(bar);
    const count = document.createElement("span");
    count.className = "app-count";
    count.textContent = String(a.count);
    row.append(name, barWrap, count);
    list.appendChild(row);
  }
}

async function refresh(): Promise<void> {
  try {
    const s = await invoke<Stats>("get_stats");
    const set = (id: string, v: string) => {
      const el = document.getElementById(id);
      if (el) el.textContent = v;
    };
    set("stat-today", String(s.today));
    set("stat-total", String(s.total));
    set("stat-streak", `${s.streak}d`);
    set("pill-count", `${s.today} today`);

    const counts = new Map(s.heatmap.map((d) => [d.date, d.count]));
    const max = Math.max(0, ...s.heatmap.map((d) => d.count));
    for (const { cell, date } of cells) {
      const count = counts.get(date) ?? 0;
      cell.dataset.level = String(levelFor(count, max));
      const d = new Date(`${date}T12:00:00`);
      cell.title =
        count > 0
          ? `${d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })} · ${count} event${count === 1 ? "" : "s"}`
          : `${d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })} · no events`;
    }
    renderApps(s.top_apps);
  } catch {
    // backend not ready yet
  }
}

let expanded = false;
let collapseTimer: ReturnType<typeof setTimeout> | undefined;

function applyState(): void {
  const widget = document.getElementById("widget");
  if (!widget) return;
  widget.dataset.state = expanded ? "open" : "closed";
  if (expanded) void refresh();
  invoke("set_widget_state", { expanded }).catch(() => {});
}

function openWidget(): void {
  if (collapseTimer) {
    clearTimeout(collapseTimer);
    collapseTimer = undefined;
  }
  if (!expanded) {
    expanded = true;
    applyState();
  }
}

function scheduleCollapse(): void {
  if (!expanded) return;
  if (collapseTimer) clearTimeout(collapseTimer);
  collapseTimer = setTimeout(() => {
    expanded = false;
    collapseTimer = undefined;
    applyState();
  }, 600);
}

/* ---------------- settings ---------------- */

let cfg: WidgetConfig = { mode: "all", apps: [], watch_folders: null };

function renderModeSeg(): void {
  document
    .querySelectorAll<HTMLButtonElement>("#mode-seg .seg-btn")
    .forEach((b) => {
      b.classList.toggle("active", b.dataset.mode === cfg.mode);
    });
}

function renderChips(recent: AppCount[]): void {
  const wrap = document.getElementById("app-chips")!;
  wrap.innerHTML = "";
  const names = new Set<string>([
    ...cfg.apps.map((a) => a.toLowerCase()),
    ...recent.map((a) => a.app.toLowerCase()),
  ]);
  const countOf = (n: string): number =>
    recent.find((r) => r.app.toLowerCase() === n)?.count ?? 0;
  const sorted = [...names].sort(
    (a, b) => countOf(b) - countOf(a) || a.localeCompare(b),
  );
  for (const n of sorted) {
    const chip = document.createElement("button");
    chip.className = "chip";
    const enabled = cfg.mode === "all" || cfg.apps.some((a) => a.toLowerCase() === n);
    chip.classList.toggle("on", enabled);
    chip.textContent = `${n.replace(/\.exe$/i, "")} · ${countOf(n)}`;
    chip.addEventListener("click", () => {
      if (cfg.mode === "all") {
        cfg.mode = "allowlist";
        cfg.apps = [...names];
      }
      const has = cfg.apps.some((a) => a.toLowerCase() === n);
      cfg.apps = has
        ? cfg.apps.filter((a) => a.toLowerCase() !== n)
        : [...cfg.apps, n];
      renderChips(recent);
      renderModeSeg();
    });
    wrap.appendChild(chip);
  }
  if (!sorted.length) {
    wrap.innerHTML = '<span class="empty">No apps seen yet</span>';
  }
}

async function openSettings(): Promise<void> {
  const settings = document.getElementById("settings-view")!;
  const main = document.getElementById("main-view")!;
  main.hidden = true;
  settings.hidden = false;
  try {
    const [loaded, recent] = await Promise.all([
      invoke<WidgetConfig>("get_config"),
      invoke<AppCount[]>("recent_apps"),
    ]);
    cfg = {
      mode: loaded.mode === "allowlist" ? "allowlist" : "all",
      apps: loaded.apps ?? [],
      watch_folders: loaded.watch_folders ?? null,
    };
    renderModeSeg();
    renderChips(recent);
    const folders = document.getElementById("folders-input") as HTMLInputElement;
    folders.value = (cfg.watch_folders ?? []).join("; ");
  } catch {
    // ignore
  }
}

function closeSettings(): void {
  document.getElementById("settings-view")!.hidden = true;
  document.getElementById("main-view")!.hidden = false;
}

async function saveSettings(): Promise<void> {
  const foldersRaw = (document.getElementById("folders-input") as HTMLInputElement)
    .value.trim();
  const watch_folders = foldersRaw
    ? foldersRaw
        .split(/[;\n]/)
        .map((s) => s.trim())
        .filter(Boolean)
    : null;
  await invoke("save_config", {
    args: {
      mode: cfg.mode,
      apps: cfg.apps.map((a) => (a.includes(".") ? a : `${a}.exe`)),
      watch_folders,
    },
  });
  const hint = document.getElementById("save-hint")!;
  hint.hidden = false;
  setTimeout(() => (hint.hidden = true), 3000);
}

function setupUI(): void {
  const widget = document.getElementById("widget")!;

  document.getElementById("pill")?.addEventListener("mouseenter", openWidget);

  widget.addEventListener("mouseenter", () => {
    if (collapseTimer) {
      clearTimeout(collapseTimer);
      collapseTimer = undefined;
    }
  });
  widget.addEventListener("mouseleave", scheduleCollapse);

  document.getElementById("collapse-btn")?.addEventListener("click", (e) => {
    e.stopPropagation();
    expanded = false;
    applyState();
  });
  document.getElementById("settings-btn")?.addEventListener("click", (e) => {
    e.stopPropagation();
    const hidden = document.getElementById("settings-view")!.hidden;
    if (hidden) void openSettings();
    else closeSettings();
  });
  document
    .querySelectorAll<HTMLButtonElement>("#mode-seg .seg-btn")
    .forEach((b) =>
      b.addEventListener("click", () => {
        cfg.mode = (b.dataset.mode as WidgetConfig["mode"]) ?? "all";
        if (cfg.mode === "all") cfg.apps = [];
        void openSettings();
      }),
    );
  document.getElementById("app-add")?.addEventListener("click", () => {
    const input = document.getElementById("app-input") as HTMLInputElement;
    const v = input.value.trim().toLowerCase();
    if (!v) return;
    if (!cfg.apps.some((a) => a.toLowerCase() === v)) {
      cfg.apps = [...cfg.apps, v];
      if (cfg.mode === "all") cfg.mode = "allowlist";
    }
    input.value = "";
    void openSettings();
  });
  document.getElementById("save-btn")?.addEventListener("click", () => {
    void saveSettings();
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && expanded) {
      expanded = false;
      applyState();
    }
  });
}

renderDate();
setupUI();
void refresh();
setInterval(() => void refresh(), 30_000);
