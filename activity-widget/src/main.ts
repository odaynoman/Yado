import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { ICONS } from "./icons";
import { refreshTasksPage, setupTasksPage } from "./tasksPage";
import type { AppCount, Stats, Task, WidgetConfig } from "./types";
import "./tokens.css";
import "./styles.css";

const $ = (id: string): HTMLElement => document.getElementById(id)!;

const DONE_FLASH_MS = 5000;
const STATS_POLL_MS = 30_000;
const CLOCK_POLL_MS = 1000;

/** Island sizes per stage — mirrored in notch_daemon for hit-testing. */
const ISLAND_PILL: [number, number] = [340, 44];
const ISLAND_DASHBOARD: [number, number] = [474, 312];
const ISLAND_SETTINGS: [number, number] = [474, 424];
const ISLAND_TASKS: [number, number] = [560, 444];

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

/** Local, same-document notification for task mutations. */
function emitTasksChanged(): void {
  document.dispatchEvent(new CustomEvent("tasks-changed"));
}

/* ============================ heatmap ============================ */

const HEATMAP_DAYS = 12 * 7;

const heatCells: { el: HTMLDivElement; date: string }[] = (() => {
  const grid = $("heatmap");
  const today = new Date();
  const start = new Date(today);
  start.setDate(start.getDate() - (HEATMAP_DAYS - 1));

  const pad = (start.getDay() + 6) % 7;
  for (let i = 0; i < pad; i++) {
    const filler = document.createElement("div");
    filler.className = "cell filler";
    grid.appendChild(filler);
  }
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
    await api.setTaskDone(t.id, true).catch(() => {});
    emitTasksChanged();
  });

  const title = document.createElement("span");
  title.className = "task-title";
  title.textContent = t.title;

  const play = document.createElement("button");
  play.className = "task-play";
  play.title = "Start focus";
  play.innerHTML = ICONS.play;
  play.addEventListener("click", (e) => {
    e.stopPropagation();
    openFocusPop(t.title, t.duration_min);
  });

  row.addEventListener("click", () => openTasksPage());
  row.append(circle, title, play);
  return row;
}

async function refreshTaskPreview(): Promise<void> {
  try {
    const tasks = await api.listTasks("open", null);
    const list = $("task-preview");
    list.innerHTML = "";
    $("todo-count").textContent = String(tasks.length);
    if (!tasks.length) {
      list.innerHTML = '<span class="empty">No open tasks</span>';
      return;
    }
    for (const t of tasks.slice(0, 6)) list.appendChild(previewRow(t));
  } catch {
    // ignore
  }
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

const focus = {
  session: null as FocusSession | null,
  timer: undefined as ReturnType<typeof setInterval> | undefined,
  streak: 0,
  popTask: "",
  popMin: 25,
  popSec: 0,
  /** Pin reflects the focus-duration popover; the daemon keeps the island open. */
  pin: false,
};

function setPin(pin: boolean): void {
  focus.pin = pin;
  void api.setPin(pin);
}

function openFocusPop(title: string, defaultMin: number | null): void {
  focus.popTask = title;
  focus.popMin = defaultMin && defaultMin > 0 ? defaultMin : 25;
  focus.popSec = 0;
  renderStepper();
  $("pop-task").textContent = title;
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
  await notifyFocusDone(s.title);

  $("island").dataset.focus = "done";
  $("focus-bar").hidden = true;
  $("focus-line").hidden = true;
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

async function notifyFocusDone(title: string): Promise<void> {
  try {
    if (!(await isPermissionGranted())) {
      const perm = await requestPermission();
      if (perm !== "granted") return;
    }
    sendNotification({ title: "Focus session complete", body: title });
  } catch {
    // notifications unavailable — the island flash still signals completion
  }
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
    const total = focus.popMin * 60 + focus.popSec;
    closeFocusPop();
    if (total > 0) startSession(focus.popTask, total);
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

  void listen<FocusRequest>("focus-request", ({ payload }) => {
    openFocusPop(payload.title, payload.minutes);
  });
}

interface FocusRequest {
  title: string;
  minutes: number | null;
}

/* ============================ stage (from daemon) ============================ */

function islandSize(): [number, number] {
  if ($("island").dataset.stage !== "expanded") return ISLAND_PILL;
  if ($("island").dataset.page === "tasks") return ISLAND_TASKS;
  return $("settings-view").hidden ? ISLAND_DASHBOARD : ISLAND_SETTINGS;
}

/** Keeps data-view/data-page metadata and the daemon's island size in sync. */
function syncIslandSize(): void {
  const island = $("island");
  island.dataset.view = $("settings-view").hidden ? "main" : "settings";
  const [w, h] = islandSize();
  void api.setIslandSize(w, h);
}

/** The backend daemon owns the stage; we only render it. */
function setupStageListener(): void {
  void listen<{ stage: "compact" | "expanded" | "hidden" }>("notch://stage", ({ payload }) => {
    $("island").dataset.stage = payload.stage === "expanded" ? "expanded" : "compact";
    if (payload.stage !== "expanded") {
      // Leaving the expanded stage resets transient UI.
      closeFocusPop();
      closeSettings();
      closeTasksPage();
      void refreshStats();
      void refreshTaskPreview();
    }
    syncIslandSize();
  });

  $("pill").addEventListener("click", () => void api.expandNow());
  $("collapse-btn").addEventListener("click", () => void api.collapseNow());
  $("tasks-btn").addEventListener("click", () => openTasksPage());
}

/* ============================ tasks page ============================ */

function tasksPageOpen(): boolean {
  return $("island").dataset.page === "tasks";
}

function openTasksPage(): void {
  closeSettings();
  $("island").dataset.page = "tasks";
  syncIslandSize();
  void api.setPin(true);
  refreshTasksPage();
}

function closeTasksPage(): void {
  if (!tasksPageOpen()) return;
  $("island").dataset.page = "dashboard";
  syncIslandSize();
  setPin(false);
}

/* ============================ settings ============================ */

let cfg: WidgetConfig = { mode: "all", apps: [], watch_folders: null };

function renderModeSeg(): void {
  document
    .querySelectorAll<HTMLButtonElement>("#mode-seg .seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.mode === cfg.mode));
}

function renderAppChips(recent: AppCount[]): void {
  const wrap = $("app-chips");
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
    chip.classList.toggle(
      "on",
      cfg.mode === "all" || cfg.apps.some((a) => a.toLowerCase() === n),
    );
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
      renderAppChips(recent);
      renderModeSeg();
    });
    wrap.appendChild(chip);
  }
  if (!sorted.length) wrap.innerHTML = '<span class="empty">No apps seen yet</span>';
}

async function openSettings(): Promise<void> {
  $("main-view").hidden = true;
  $("settings-view").hidden = false;
  syncIslandSize();
  try {
    const [loaded, recent] = await Promise.all([api.getConfig(), api.recentApps()]);
    cfg = {
      mode: loaded.mode === "allowlist" ? "allowlist" : "all",
      apps: loaded.apps ?? [],
      watch_folders: loaded.watch_folders ?? null,
    };
    renderModeSeg();
    renderAppChips(recent);
    ($("folders-input") as HTMLInputElement).value = (cfg.watch_folders ?? []).join("; ");
  } catch {
    // ignore
  }
}

function closeSettings(): void {
  if ($("settings-view").hidden) return;
  $("settings-view").hidden = true;
  $("main-view").hidden = false;
  syncIslandSize();
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

function setupSettings(): void {
  $("settings-btn").addEventListener("click", () => {
    const hidden = $("settings-view").hidden;
    if (hidden) void openSettings();
    else closeSettings();
  });
  document.querySelectorAll<HTMLButtonElement>("#mode-seg .seg-btn").forEach((b) =>
    b.addEventListener("click", () => {
      cfg.mode = (b.dataset.mode as WidgetConfig["mode"]) ?? "all";
      if (cfg.mode === "all") cfg.apps = [];
      void openSettings();
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
    void openSettings();
  });
  $("save-btn").addEventListener("click", () => void saveSettings());
}

/* ============================ boot ============================ */

function main(): void {
  $("today-date").textContent = new Date().toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });

  setupStageListener();
  setupFocusUI();
  setupSettings();
  setupTasksPage({
    onChanged: () => void refreshTaskPreview(),
    onClose: () => closeTasksPage(),
  });
  syncIslandSize();

  document.addEventListener("tasks-changed", () => void refreshTaskPreview());

  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (!$("focus-overlay").hidden) {
      closeFocusPop();
    } else if (tasksPageOpen()) {
      closeTasksPage();
    } else {
      void api.collapseNow();
    }
  });

  void refreshStats();
  void refreshTaskPreview();
  renderPill();
  setInterval(renderPill, CLOCK_POLL_MS);
  setInterval(() => void refreshStats(), STATS_POLL_MS);
}

main();
