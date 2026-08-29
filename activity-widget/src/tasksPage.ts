import { api } from "./api";
import { ICONS } from "./icons";
import type { Task, TaskFilter } from "./types";

const $ = (id: string): HTMLElement => document.getElementById(id)!;

/** Duration presets cycled by clicking a task's duration chip (minutes). */
const DURATION_PRESETS: (number | null)[] = [null, 5, 15, 25, 45];

export interface TasksPageHooks {
  /** Called after any task mutation so the dashboard preview refreshes. */
  onChanged: () => void;
  /** Called when the user closes the page. */
  onClose: () => void;
}

const state = {
  viewMonth: new Date(),
  selectedDay: todayStr(),
  mode: "day" as "day" | "unscheduled",
};

function todayStr(): string {
  const d = new Date();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

function fmtDateLocal(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

function fmtDuration(min: number | null): string {
  return min === null ? "no est" : `${min}m`;
}

function fmtDayLabel(date: string): string {
  const d = new Date(`${date}T12:00:00`);
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Local, same-document notification for task mutations. */
function emitChanged(): void {
  document.dispatchEvent(new CustomEvent("tasks-changed"));
}

/* ============================ calendar ============================ */

function renderCalendar(): void {
  const grid = $("cal-days");
  grid.innerHTML = "";
  const { viewMonth, selectedDay } = state;

  $("cal-title").textContent = viewMonth.toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });

  const year = viewMonth.getFullYear();
  const month = viewMonth.getMonth();
  const first = new Date(year, month, 1);
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const pad = (first.getDay() + 6) % 7; // Monday-first offset
  const leadDays = pad === 0 ? 7 : pad;
  const total = Math.ceil((leadDays + daysInMonth) / 7) * 7;

  for (let i = 0; i < total; i++) {
    const d = new Date(year, month, 1 - leadDays + i);
    const cell = document.createElement("button");
    const date = fmtDateLocal(d);
    cell.className = "cal-day";
    if (d.getMonth() !== month) cell.classList.add("muted");
    if (date === todayStr()) cell.classList.add("today");
    if (date === selectedDay) cell.classList.add("selected");
    cell.textContent = String(d.getDate());
    cell.title = d.toLocaleDateString();
    cell.addEventListener("click", () => {
      state.selectedDay = date;
      state.viewMonth = new Date(d.getFullYear(), d.getMonth(), 1);
      renderCalendar();
      void refreshList();
    });
    grid.appendChild(cell);
  }
}

function setupCalendar(): void {
  $("cal-prev").addEventListener("click", () => {
    state.viewMonth = new Date(state.viewMonth.getFullYear(), state.viewMonth.getMonth() - 1, 1);
    renderCalendar();
  });
  $("cal-next").addEventListener("click", () => {
    state.viewMonth = new Date(state.viewMonth.getFullYear(), state.viewMonth.getMonth() + 1, 1);
    renderCalendar();
  });
  $("cal-today").addEventListener("click", () => {
    state.viewMonth = new Date();
    state.selectedDay = todayStr();
    renderCalendar();
    void refreshList();
  });
}

/* ============================ task list ============================ */

function taskRow(t: Task): HTMLElement {
  const row = document.createElement("div");
  row.className = "trow";

  const circle = document.createElement("button");
  circle.className = "task-circle";
  circle.title = "Complete";
  circle.addEventListener("click", () => {
    void api.setTaskDone(t.id, true).catch(() => {});
    emitChanged();
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

  const dueChip = document.createElement("span");
  dueChip.className = "chip";
  dueChip.textContent = t.due_date ? fmtDayLabel(t.due_date) : "no date";

  const durationChip = document.createElement("button");
  durationChip.className = "chip";
  durationChip.title = "Click to change estimate";
  durationChip.textContent = fmtDuration(t.duration_min);
  durationChip.addEventListener("click", () => {
    const idx = DURATION_PRESETS.indexOf(t.duration_min);
    const next = DURATION_PRESETS[(idx + 1) % DURATION_PRESETS.length];
    t.duration_min = next;
    durationChip.textContent = fmtDuration(next);
    void api.setTaskDuration(t.id, next).catch(() => {});
  });

  const play = document.createElement("button");
  play.className = "task-play";
  play.title = "Start focus";
  play.innerHTML = ICONS.play;
  play.addEventListener("click", () => {
    document.dispatchEvent(
      new CustomEvent<FocusRequestLike>("focus-request", {
        detail: { title: t.title, minutes: t.duration_min },
      }),
    );
  });

  const del = document.createElement("button");
  del.className = "task-del";
  del.title = "Delete";
  del.innerHTML = ICONS.trash;
  del.addEventListener("click", () => {
    void api.deleteTask(t.id).catch(() => {});
    emitChanged();
  });

  row.append(circle, body, dueChip, durationChip, del, play);
  return row;
}

export interface FocusRequestLike {
  title: string;
  minutes: number | null;
}

async function refreshList(): Promise<void> {
  const filter: TaskFilter = state.mode === "day" ? "day" : "unscheduled";
  try {
    const [tasks, open] = await Promise.all([
      api.listTasks(filter, state.selectedDay),
      api.listTasks("open", null),
    ]);
    $("t-open-count").textContent = `${open.length} open`;
    $("t-list-title").textContent =
      state.mode === "day" ? fmtDayLabel(state.selectedDay) : "Unscheduled";

    const list = $("t-list");
    list.innerHTML = "";
    if (!tasks.length) {
      list.innerHTML = `<span class="empty">${
        state.mode === "day" ? "Nothing scheduled this day" : "Nothing unscheduled"
      }</span>`;
      return;
    }
    for (const t of tasks) list.appendChild(taskRow(t));
  } catch {
    // ignore
  }
}

/* ============================ add form ============================ */

function resetAddForm(): void {
  ($("t-title") as HTMLInputElement).value = "";
  ($("t-notes") as HTMLInputElement).value = "";
  $("t-notes-row").hidden = true;
  ($("t-title") as HTMLInputElement).blur();
}

function setupAddForm(): void {
  const title = $("t-title") as HTMLInputElement;
  const notes = $("t-notes") as HTMLInputElement;

  title.addEventListener("focus", () => ($("t-notes-row").hidden = false));
  document.addEventListener("click", (e) => {
    if (!(e.target as HTMLElement).closest(".tadd")) $("t-notes-row").hidden = true;
  });

  const submit = (): void => {
    const t = title.value.trim();
    if (!t) return;
    void api
      .addTask({
        title: t,
        notes: notes.value.trim() || null,
        due_date: state.mode === "day" ? state.selectedDay : null,
        duration_min: null,
      })
      .then(() => {
        resetAddForm();
        emitChanged();
      })
      .catch(() => {});
  };
  title.addEventListener("keydown", (e) => e.key === "Enter" && submit());
  notes.addEventListener("keydown", (e) => e.key === "Enter" && submit());
  $("t-add").addEventListener("click", submit);
  $("t-cancel").addEventListener("click", resetAddForm);
}

/* ============================ setup ============================ */

export function refreshTasksPage(): void {
  void refreshList();
}

export function setupTasksPage(hooks: TasksPageHooks): void {
  setupCalendar();
  setupAddForm();

  $("t-close").addEventListener("click", hooks.onClose);

  document.querySelectorAll<HTMLButtonElement>("#t-mode .seg-btn").forEach((b) =>
    b.addEventListener("click", () => {
      const mode = (b.dataset.mode as "day" | "unscheduled") ?? "day";
      if (mode === state.mode) return;
      state.mode = mode;
      document
        .querySelectorAll<HTMLButtonElement>("#t-mode .seg-btn")
        .forEach((x) => x.classList.toggle("active", x === b));
      void refreshList();
    }),
  );

  document.addEventListener("tasks-changed", () => void refreshList());

  renderCalendar();
  void refreshList();
}
