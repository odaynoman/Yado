import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api } from "./api";
import { ICONS } from "./icons";
import { setMediaSuppressed, setupMedia } from "./mediaCard";
import { applyAppIcon } from "./appIcon";
import { setupTasksPage } from "./tasksPage";
import {
  addTask as storeAddTask,
  getTasks,
  refreshTasks as storeRefreshTasks,
  setTaskDone as storeSetTaskDone,
  subscribeTasks,
  updateTask as storeUpdateTask,
} from "./tasksStore";
import {
  getShelf,
  shelfAdd,
  shelfRemove,
  subscribeShelf,
  type ShelfItem,
} from "./shelfStore";
import type { AppCount, Stats, Task, WidgetConfig } from "./types";
import "./tokens.css";
import "./styles.css";

const $ = (id: string): HTMLElement => document.getElementById(id)!;

const DONE_FLASH_MS = 5000;
const STATS_POLL_MS = 30_000;
const CLOCK_POLL_MS = 1000;

/** Island sizes per stage — mirrored in notch_daemon for hit-testing. */
const ISLAND_PILL: [number, number] = [340, 44];
const ISLAND_DASHBOARD: [number, number] = [600, 340];
const ISLAND_PAGE: [number, number] = [600, 460];

/* ============================ helpers ============================ */

function fmtDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function fmtClock(totalSec: number): string {
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/* ============================ heatmap (rolling window) ============================ */

/** 12 columns x 3 rows = 36 days ending today. Every cell is a real day —
 *  days from the previous month fill the start naturally, so the grid is
 *  always full and the last cell is always today. */
const HEATMAP_COLS = 12;
const HEATMAP_DAYS = HEATMAP_COLS * 3;

const heatCells: { el: HTMLDivElement; date: string }[] = (() => {
  const grid = $("heatmap");
  const today = new Date();
  const start = new Date(today);
  start.setDate(start.getDate() - (HEATMAP_DAYS - 1));

  const cells: { el: HTMLDivElement; date: string }[] = [];
  for (let i = 0; i < HEATMAP_DAYS; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    const el = document.createElement("div");
    el.className = "cell";
    el.dataset.level = "0";
    grid.appendChild(el);
    cells.push({ el, date: fmtDate(d) });
  }
  return cells;
})();

function levelFor(count: number, max: number): number {
  if (count <= 0) return 0;
  const scale = Math.max(4, max);
  return Math.min(4, Math.ceil((count * 4) / scale));
}

async function refreshStats(): Promise<void> {
  try {
    const s: Stats = await api.getStats();
    $("stat-today").textContent = String(s.today);
    $("stat-total").textContent = String(s.total);
    $("stat-streak").textContent = `${s.streak}d`;
    $("header-streak").innerHTML = `&#x1F525; ${s.streak}d`;
    $("pill-streak").textContent = String(s.streak);
    focus.streak = s.streak;

    const counts = new Map(s.heatmap.map((d) => [d.date, d.count]));
    const max = Math.max(0, ...s.heatmap.map((d) => d.count));
    for (const { el, date } of heatCells) {
      const count = counts.get(date) ?? 0;
      el.dataset.level = String(levelFor(count, max));
      const d = new Date(`${date}T12:00:00`);
       el.title = `${d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })} · ${count} event${count === 1 ? "" : "s"}`;
    }
    renderPill();
  } catch {
    // backend not ready
  }
}

/* ============================ task preview ============================ */

function previewRow(t: Task): HTMLElement {
  const row = document.createElement("div");
  row.className = "task-row";

  const circle = document.createElement("button");
  circle.className = "task-circle";
  circle.title = "Complete";
  circle.addEventListener("click", async (e) => {
    e.stopPropagation();
    await storeSetTaskDone(t.id, true);
  });

  const body = document.createElement("div");
  body.className = "task-body";
  const title = document.createElement("span");
  title.className = "task-title";
  title.textContent = t.title;
  body.appendChild(title);
  if (t.notes?.trim()) {
    const notes = document.createElement("span");
    notes.className = "task-notes";
    notes.textContent = t.notes;
    body.appendChild(notes);
  }

  const play = document.createElement("button");
  play.className = "task-play";
  play.title = "Start focus";
  play.innerHTML = ICONS.play;
  play.addEventListener("click", (e) => {
    e.stopPropagation();
    openFocusPop(t.title, t.duration_min);
  });

  row.addEventListener("click", () => openTasksPage());
  row.append(circle, body, play);
  return row;
}

/** Renders the preview purely from the store — no fetching here. */
function renderTaskPreview(): void {
  const tasks = getTasks().filter((t) => !t.done);
  const list = $("task-preview");
  list.innerHTML = "";
  $("todo-count").textContent = String(tasks.length);
  if (!tasks.length) {
    list.innerHTML = '<span class="empty">No open tasks</span>';
    return;
  }
  for (const t of tasks.slice(0, 6)) list.appendChild(previewRow(t));
}

/* ============================ pill ============================ */

function renderPill(): void {
  const clockEl = $("pill-clock");
  const taskEl = $("pill-task");

  if (focus.session) {
    const s = focus.session;
    clockEl.textContent = s.paused
      ? `II ${fmtClock(s.remainingSec)}`
      : fmtClock(s.remainingSec);
    taskEl.textContent = s.done ? "Focus complete" : s.title;
  } else {
    clockEl.textContent = new Date().toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
    taskEl.textContent = "";
  }
}

/* ============================ focus sessions ============================ */

interface FocusSession {
  title: string;
  totalSec: number;
  remainingSec: number;
  /** Wall-clock deadline keeps the countdown accurate despite timer drift. */
  deadline: number;
  paused: boolean;
  done: boolean;
}

interface FocusRequest {
  title: string;
  minutes: number | null;
}

const focus = {
  session: null as FocusSession | null,
  timer: undefined as ReturnType<typeof setInterval> | undefined,
  streak: 0,
  popTask: "",
  popMin: 25,
  popSec: 0,
  /** "start" runs a session on apply; "estimate" saves it on the task. */
  popMode: "start" as "start" | "estimate",
  estimateId: 0,
  /** Pin reflects the focus-duration popover; the daemon keeps the island open. */
  pin: false,
};

function setPin(pin: boolean): void {
  focus.pin = pin;
  void api.setPin(pin);
}

function openFocusPop(
  title: string,
  defaultMin: number | null,
  mode: "start" | "estimate" = "start",
  estimateId = 0,
): void {
  focus.popTask = title;
  focus.popMin = defaultMin && defaultMin > 0 ? defaultMin : 25;
  focus.popSec = 0;
  focus.popMode = mode;
  focus.estimateId = estimateId;
  renderStepper();
  $("pop-task").textContent = title;
  $("focus-start").textContent = mode === "estimate" ? "Apply" : "Start";
  $("focus-overlay").hidden = false;
  setPin(true);
}

function closeFocusPop(): void {
  if ($("focus-overlay").hidden) return;
  $("focus-overlay").hidden = true;
  setPin(false);
}

function renderStepper(): void {
  $("min-val").textContent = String(focus.popMin);
  $("sec-val").textContent = String(focus.popSec).padStart(2, "0");
}

function startSession(title: string, totalSec: number): void {
  stopSessionTimer();
  focus.session = {
    title,
    totalSec,
    remainingSec: totalSec,
    deadline: Date.now() + totalSec * 1000,
    paused: false,
    done: false,
  };
  $("island").dataset.focus = "running";
  $("focus-task").textContent = title;
  $("focus-bar").hidden = false;
  $("focus-line").hidden = false;
  setMediaSuppressed(true);
  syncIslandSize();
  focus.timer = setInterval(tickSession, 500);
  tickSession();
}

function tickSession(): void {
  const s = focus.session;
  if (!s || s.paused || s.done) return;
  s.remainingSec = Math.max(0, Math.ceil((s.deadline - Date.now()) / 1000));
  updateFocusUI();
  if (s.remainingSec === 0) void completeSession();
}

async function completeSession(): Promise<void> {
  const s = focus.session;
  if (!s) return;
  s.done = true;
  stopSessionTimer();

  await api.logEvent("Focus", "focus_session", s.title).catch(() => {});
  await api.notifyFocusDone(s.title).catch(() => {});

  $("island").dataset.focus = "done";
  $("focus-bar").hidden = true;
  $("focus-line").hidden = true;
  setMediaSuppressed(false);
  syncIslandSize();
  setTimeout(() => {
    if (focus.session?.done) {
      focus.session = null;
      $("island").dataset.focus = "";
      renderPill();
    }
  }, DONE_FLASH_MS);
  renderPill();
  await refreshStats();
}

function stopSession(): void {
  stopSessionTimer();
  focus.session = null;
  $("island").dataset.focus = "";
  $("focus-bar").hidden = true;
  $("focus-line").hidden = true;
  setMediaSuppressed(false);
  syncIslandSize();
  renderPill();
}

function stopSessionTimer(): void {
  if (focus.timer) {
    clearInterval(focus.timer);
    focus.timer = undefined;
  }
}

function updateFocusUI(): void {
  const s = focus.session;
  if (!s) return;
  $("focus-count").textContent = fmtClock(s.remainingSec);
  const pct = Math.round(((s.totalSec - s.remainingSec) / s.totalSec) * 100);
  $("focus-line-fill").style.width = `${pct}%`;
  const pauseIc = $("focus-pause-ic") as HTMLElement;
  const playIc = $("focus-play-ic") as HTMLElement;
  pauseIc.style.display = s.paused ? "none" : "block";
  playIc.style.display = s.paused ? "block" : "none";
  renderPill();
}

function setupFocusUI(): void {
  const clamp = (v: number, max: number) => Math.max(0, Math.min(max, v));
  $("min-up").addEventListener("click", () => {
    focus.popMin = clamp(focus.popMin + 1, 180);
    renderStepper();
  });
  $("min-down").addEventListener("click", () => {
    focus.popMin = clamp(focus.popMin - 1, 180);
    renderStepper();
  });
  $("sec-up").addEventListener("click", () => {
    focus.popSec = clamp(focus.popSec + 5, 55);
    renderStepper();
  });
  $("sec-down").addEventListener("click", () => {
    focus.popSec = clamp(focus.popSec - 5, 55);
    renderStepper();
  });
  $("focus-start").addEventListener("click", () => {
    const totalSec = focus.popMin * 60 + focus.popSec;
    if (focus.popMode === "estimate") {
      const minutes = totalSec === 0 ? null : Math.max(1, Math.round(totalSec / 60));
      void storeUpdateTask(focus.estimateId, { duration_min: minutes });
    } else if (totalSec > 0) {
      startSession(focus.popTask, totalSec);
    }
    closeFocusPop();
  });
  $("focus-pop-cancel").addEventListener("click", closeFocusPop);
  $("focus-overlay").addEventListener("click", (e) => {
    if (e.target === $("focus-overlay")) closeFocusPop();
  });

  $("focus-toggle").addEventListener("click", () => {
    const s = focus.session;
    if (!s || s.done) return;
    if (s.paused) {
      s.deadline = Date.now() + s.remainingSec * 1000;
      s.paused = false;
    } else {
      s.paused = true;
    }
    updateFocusUI();
  });
  $("focus-stop").addEventListener("click", () => stopSession());

  // Play buttons on the tasks page request sessions through a DOM event.
  document.addEventListener("focus-request", (e) => {
    const { title, minutes } = (e as CustomEvent<FocusRequest>).detail;
    openFocusPop(title, minutes);
  });

  // Duration chips on the tasks page open the same popover in estimate mode.
  document.addEventListener("duration-request", (e) => {
    const { id, title, minutes } = (e as CustomEvent<FocusRequest & { id: number }>).detail;
    openFocusPop(title, minutes, "estimate", id);
  });

  // Clicking outside the island while transient UI is pinned dismisses it.
  void listen("notch://outside", () => {
    if (!$("focus-overlay").hidden) closeFocusPop();
    else if (!$("schedule-overlay").hidden) closeSchedulePop();
    else void api.collapseNow();
  });
}

/* ============================ stage (from daemon) ============================ */

function islandSize(): [number, number] {
  if ($("island").dataset.stage !== "expanded") return ISLAND_PILL;
  if ($("island").dataset.page !== "dashboard") return ISLAND_PAGE;
  // Dynamic strips reserve their height only while actually shown.
  const mediaBarShown = !$("media-card").hidden && $("focus-bar").hidden;
  const shelfShown = !$("shelf-card").hidden;
  let h = mediaBarShown ? ISLAND_DASHBOARD[1] : 286;
  if (shelfShown) h += 78;
  return [ISLAND_DASHBOARD[0], h];
}

/** Publishes the live island size so the daemon's hit-testing matches. */
function syncIslandSize(): void {
  const island = $("island");
  const [w, h] = islandSize();
  if (island.dataset.stage === "expanded" && island.dataset.page === "dashboard") {
    island.style.height = `${h}px`;
  } else {
    island.style.height = "";
  }
  void api.setIslandSize(w, h);
}

/** The backend daemon owns the stage; we only render it. */
function setupStageListener(): void {
  void listen<{ stage: "compact" | "expanded" | "hidden" }>("notch://stage", ({ payload }) => {
    $("island").dataset.stage = payload.stage === "expanded" ? "expanded" : "compact";
    if (payload.stage !== "expanded") {
      // Leaving the expanded stage resets transient UI.
      closeFocusPop();
      closeSettingsPage();
      closeTasksPage();
      void refreshStats();
    }
    syncIslandSize();
  });

  $("pill").addEventListener("click", () => void api.expandNow());
  $("collapse-btn").addEventListener("click", () => void api.collapseNow());
  $("tasks-btn").addEventListener("click", () => openTasksPage());
  $("settings-btn").addEventListener("click", () => openSettingsPage());
}

/* ============================ pages ============================ */

function tasksPageOpen(): boolean {
  return $("island").dataset.page === "tasks";
}

function settingsPageOpen(): boolean {
  return $("island").dataset.page === "settings";
}

function openTasksPage(): void {
  closeSettingsPage();
  $("island").dataset.page = "tasks";
  syncIslandSize();
  void api.setPin(true);
  void storeRefreshTasks();
}

function closeTasksPage(): void {
  if (!tasksPageOpen()) return;
  $("island").dataset.page = "dashboard";
  syncIslandSize();
  setPin(false);
}

function openSettingsPage(): void {
  closeTasksPage();
  $("island").dataset.page = "settings";
  syncIslandSize();
  void api.setPin(true);
  void loadSettings();
}

function closeSettingsPage(): void {
  if (!settingsPageOpen()) return;
  $("island").dataset.page = "dashboard";
  syncIslandSize();
  setPin(false);
}

/* ============================ settings page content ============================ */

let cfg: WidgetConfig = { mode: "all", apps: [], watch_folders: null };

function renderModeSeg(): void {
  document
    .querySelectorAll<HTMLButtonElement>("#mode-seg .seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.mode === cfg.mode));
}

function appSwitch(enabled: boolean, onToggle: () => void): HTMLButtonElement {
  const sw = document.createElement("button");
  sw.className = "switch" + (enabled ? " on" : "");
  sw.setAttribute("role", "switch");
  sw.addEventListener("click", (e) => {
    e.stopPropagation();
    onToggle();
  });
  return sw;
}

/** Tracking list: icon · name · count · per-app switch. */
function renderAppList(recent: AppCount[]): void {
  const wrap = $("app-list");
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
    const row = document.createElement("div");
    row.className = "app-item";

    const ico = document.createElement("span");
    ico.className = "usage-ico";
    applyAppIcon(ico, n, "&#x25A1;");

    const name = document.createElement("span");
    name.className = "app-item-name";
    name.textContent = n.replace(/\.exe$/i, "");

    const count = document.createElement("span");
    count.className = "app-item-count";
    count.textContent = countOf(n) ? `${countOf(n)}/wk` : "";

    const enabled = cfg.mode === "all" || cfg.apps.some((a) => a.toLowerCase() === n);
    const sw = appSwitch(enabled, () => {
      if (cfg.mode === "all") {
        cfg.mode = "allowlist";
        cfg.apps = [...names];
      }
      const has = cfg.apps.some((a) => a.toLowerCase() === n);
      cfg.apps = has
        ? cfg.apps.filter((a) => a.toLowerCase() !== n)
        : [...cfg.apps, n];
      renderAppList(recent);
      renderModeSeg();
    });

    row.append(ico, name, count, sw);
    wrap.appendChild(row);
  }
  if (!sorted.length) {
    wrap.innerHTML = '<span class="empty">No apps seen yet</span>';
  }
}

function renderUsage(recent: AppCount[]): void {
  const wrap = $("usage-list");
  wrap.innerHTML = "";
  if (!recent.length) {
    wrap.innerHTML = '<span class="empty">No usage recorded yet</span>';
    return;
  }
  const total = recent.reduce((sum, r) => sum + r.count, 0) || 1;
  const max = Math.max(...recent.map((r) => r.count));

  for (const r of recent) {
    const row = document.createElement("div");
    row.className = "usage-row";

    const ico = document.createElement("span");
    ico.className = "usage-ico";
    applyAppIcon(ico, r.app, "&#x25A1;");

    const name = document.createElement("span");
    name.className = "usage-name";
    name.textContent = r.app.replace(/\.exe$/i, "");

    const bar = document.createElement("div");
    bar.className = "usage-bar";
    const fill = document.createElement("div");
    fill.className = "usage-bar-fill";
    fill.style.width = `${Math.max(4, (r.count / max) * 100)}%`;
    bar.appendChild(fill);

    const count = document.createElement("span");
    count.className = "usage-count";
    count.textContent = String(r.count);

    const share = document.createElement("span");
    share.className = "usage-share";
    share.textContent = `${Math.round((r.count / total) * 100)}%`;

    row.append(ico, name, bar, count, share);
    wrap.appendChild(row);
  }
}

async function loadSettings(): Promise<void> {
  try {
    const [loaded, recent] = await Promise.all([api.getConfig(), api.recentApps()]);
    cfg = {
      mode: loaded.mode === "allowlist" ? "allowlist" : "all",
      apps: loaded.apps ?? [],
      watch_folders: loaded.watch_folders ?? null,
    };
    renderModeSeg();
    renderAppList(recent);
    renderUsage(recent);
    void loadAutostart();
    ($("folders-input") as HTMLInputElement).value = (cfg.watch_folders ?? []).join("; ");
  } catch {
    // ignore
  }
}

async function saveSettings(): Promise<void> {
  const foldersRaw = ($("folders-input") as HTMLInputElement).value.trim();
  const watch_folders = foldersRaw
    ? foldersRaw.split(/[;\n]/).map((s) => s.trim()).filter(Boolean)
    : null;
  await api
    .saveConfig({
      mode: cfg.mode,
      apps: cfg.apps.map((a) => (a.includes(".") ? a : `${a}.exe`)),
      watch_folders,
    })
    .catch(() => {});
  $("save-hint").hidden = false;
  setTimeout(() => ($("save-hint").hidden = true), 3000);
}

function setupSettingsPage(): void {
  document.querySelectorAll<HTMLButtonElement>("#mode-seg .seg-btn").forEach((b) =>
    b.addEventListener("click", () => {
      cfg.mode = (b.dataset.mode as WidgetConfig["mode"]) ?? "all";
      if (cfg.mode === "all") cfg.apps = [];
      void loadSettings();
    }),
  );
  $("app-add").addEventListener("click", () => {
    const input = $("app-input") as HTMLInputElement;
    const v = input.value.trim().toLowerCase();
    if (!v) return;
    if (!cfg.apps.some((a) => a.toLowerCase() === v)) {
      cfg.apps = [...cfg.apps, v];
      if (cfg.mode === "all") cfg.mode = "allowlist";
    }
    input.value = "";
    void loadSettings();
  });
  $("save-btn").addEventListener("click", () => void saveSettings());
  $("settings-close").addEventListener("click", () => closeSettingsPage());
}

/* ============================ autostart ============================ */

async function loadAutostart(): Promise<void> {
  const sw = $("autostart-switch");
  const enabled = await api.autostartEnabled().catch(() => false);
  sw.classList.toggle("on", enabled);
}

function setupAutostart(): void {
  $("autostart-switch").addEventListener("click", () => {
    const enable = !$("autostart-switch").classList.contains("on");
    $("autostart-switch").classList.toggle("on", enable);
    void api.autostartSet(enable).catch(() => {});
  });
}

/* ============================ dashboard add ============================ */

function setupDashboardAdd(): void {
  const input = $("todo-add-input") as HTMLInputElement;
  const submit = (): void => {
    const t = input.value.trim();
    if (!t) return;
    input.value = "";
    // Tasks captured on the dashboard are planned for today by default.
    void storeAddTask({
      title: t,
      notes: null,
      due_date: fmtDate(new Date()),
      duration_min: null,
    });
  };
  $("todo-add-btn").addEventListener("click", submit);
  input.addEventListener("keydown", (e) => e.key === "Enter" && submit());
}

/* ============================ scheduling popover ============================ */

const schedule = {
  taskId: 0,
  title: "",
  date: "",
};

function openSchedulePop(id: number, title: string, due: string | null): void {
  schedule.taskId = id;
  schedule.title = title;
  schedule.date = due ?? fmtDate(new Date());
  ($("schedule-date") as HTMLInputElement).value = schedule.date;
  $("schedule-task").textContent = title;
  $("schedule-overlay").hidden = false;
  setPin(true);
}

function closeSchedulePop(): void {
  if ($("schedule-overlay").hidden) return;
  $("schedule-overlay").hidden = true;
  setPin(false);
}

function setupScheduleUI(): void {
  const dateInput = $("schedule-date") as HTMLInputElement;
  $("schedule-today").addEventListener("click", () => {
    schedule.date = fmtDate(new Date());
    dateInput.value = schedule.date;
  });
  $("schedule-tomorrow").addEventListener("click", () => {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    schedule.date = fmtDate(d);
    dateInput.value = schedule.date;
  });
  $("schedule-none").addEventListener("click", () => {
    schedule.date = "";
    dateInput.value = "";
  });
  dateInput.addEventListener("change", () => {
    schedule.date = dateInput.value;
  });
  $("schedule-apply").addEventListener("click", () => {
    const due = schedule.date || null;
    $("schedule-overlay").hidden = true;
    void storeUpdateTaskDue(schedule.taskId, due);
    setPin(false);
  });
  $("schedule-cancel").addEventListener("click", closeSchedulePop);
  $("schedule-overlay").addEventListener("click", (e) => {
    if (e.target === $("schedule-overlay")) closeSchedulePop();
  });

  // Due-date chips on the tasks page open this popover.
  document.addEventListener("schedule-request", (e) => {
    const { id, title, due_date } = (e as CustomEvent<ScheduleRequest>).detail;
    openSchedulePop(id, title, due_date);
  });
}

interface ScheduleRequest {
  id: number;
  title: string;
  due_date: string | null;
}

/** Due-date changes bypass the generic patch: the popover owns the field. */
function storeUpdateTaskDue(id: number, due_date: string | null): Promise<void> {
  return storeUpdateTask(id, { due_date });
}

/* ============================ file shelf ============================ */

let dragOverShelf = false;

function renderShelf(): void {
  const items = getShelf();
  const card = $("shelf-card");
  card.hidden = items.length === 0 && !dragOverShelf;
  $("shelf-count").textContent = String(items.length);

  const wrap = $("shelf-items");
  wrap.innerHTML = "";
  if (!items.length) {
    wrap.innerHTML =
      '<span class="empty">Drop files on the notch to stash them here</span>';
  } else {
    for (const it of items) wrap.appendChild(shelfTile(it));
  }
  syncIslandSize();
}

function shelfTile(it: ShelfItem): HTMLElement {
  const tile = document.createElement("div");
  tile.className = "shelf-tile";
  tile.title = it.path;

  const art = document.createElement("div");
  art.className = "shelf-art";
  art.textContent = shelfGlyph(it.name);

  const name = document.createElement("span");
  name.className = "shelf-name";
  name.textContent = it.name;

  const del = document.createElement("button");
  del.className = "task-del";
  del.title = "Remove from shelf";
  del.innerHTML = ICONS.trash;
  del.addEventListener("click", (e) => {
    e.stopPropagation();
    shelfRemove(it.path);
  });

  // Drag-out gesture: press + move beyond 8px starts a native OS drag
  // carrying the file path. Pointer capture keeps the move stream alive
  // even if hover/click-through flickers mid-gesture.
  tile.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    tile.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startY = e.clientY;
    let started = false;
    const onMove = (ev: PointerEvent): void => {
      if (started) return;
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) > 8) {
        started = true;
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        void beginShelfDrag(it);
      }
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  });

  tile.addEventListener("dblclick", () => void api.revealPath(it.path));
  tile.append(art, name, del);
  return tile;
}

function shelfGlyph(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"].includes(ext)) return "🖼";
  if (["zip", "rar", "7z", "tar", "gz"].includes(ext)) return "🗜";
  if (["pdf"].includes(ext)) return "📕";
  if (["doc", "docx", "txt", "md", "rtf"].includes(ext)) return "📝";
  if (["mp4", "mkv", "mov", "avi"].includes(ext)) return "🎬";
  if (["mp3", "wav", "flac", "aac"].includes(ext)) return "🎵";
  return "📎";
}

/** Begins a native OS drag carrying the shelf item (blocks until drop). */
async function beginShelfDrag(it: ShelfItem): Promise<void> {
  setPin(true);
  try {
    const result = await api.startFileDrag([it.path]);
    if (result === "dropped") shelfRemove(it.path);
  } catch (err) {
    // Real failures (never user-cancel) must be visible, not swallowed.
    console.error("[shelf] drag-out failed:", err);
  }
  setPin(false);
}

function setupShelf(): void {
  subscribeShelf(renderShelf);

  // Drag-in: the OS reports drops on the window; add every path.
  void getCurrentWebviewWindow().onDragDropEvent((event) => {
    const type = event.payload.type;
    if (type === "enter" || type === "over") {
      dragOverShelf = true;
      if ($("island").dataset.stage !== "expanded") void api.expandNow();
      renderShelf();
      $("island").dataset.drop = "1";
    } else if (type === "leave") {
      dragOverShelf = false;
      $("island").dataset.drop = "";
      renderShelf();
    } else if (type === "drop") {
      dragOverShelf = false;
      $("island").dataset.drop = "";
      for (const p of event.payload.paths) shelfAdd(p);
      renderShelf();
    }
  });
}

/* ============================ boot ============================ */

function main(): void {
  $("today-date").textContent = new Date().toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });

  setupStageListener();
  setupFocusUI();
  setupScheduleUI();
  setupSettingsPage();
  setupAutostart();
  setupDashboardAdd();
  setupTasksPage({
    onClose: () => closeTasksPage(),
  });
  subscribeTasks(renderTaskPreview);
  setupShelf();
  setupMedia(syncIslandSize);
  syncIslandSize();

  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (!$("focus-overlay").hidden) {
      closeFocusPop();
    } else if (!$("schedule-overlay").hidden) {
      closeSchedulePop();
    } else if (settingsPageOpen()) {
      closeSettingsPage();
    } else if (tasksPageOpen()) {
      closeTasksPage();
    } else {
      void api.collapseNow();
    }
  });

  void refreshStats();
  void storeRefreshTasks();
  renderPill();
  setInterval(renderPill, CLOCK_POLL_MS);
  setInterval(() => void refreshStats(), STATS_POLL_MS);
}

main();
