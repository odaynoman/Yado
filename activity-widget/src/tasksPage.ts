import { ICONS } from "./icons";
import {
  deleteTask,
  getTasks,
  refreshTasks,
  setTaskDone,
  subscribeTasks,
  updateTask,
} from "./tasksStore";
import type { Task } from "./types";
import "./tokens.css";
import "./styles.css";

const $ = (id: string): HTMLElement => document.getElementById(id)!;

export interface TasksPageHooks {
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
      renderList();
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
    renderList();
  });
}

/* ============================ task list ============================ */

function taskRow(t: Task): HTMLElement {
  const row = document.createElement("div");
  row.className = "trow";

  const circle = document.createElement("button");
  circle.className = "task-circle";
  circle.title = "Complete";
  circle.addEventListener("click", () => void setTaskDone(t.id, true));

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
  durationChip.title = "Click to set any duration";
  durationChip.textContent = fmtDuration(t.duration_min);
  durationChip.addEventListener("click", () => {
    document.dispatchEvent(
      new CustomEvent("duration-request", {
        detail: { id: t.id, title: t.title, minutes: t.duration_min },
      }),
    );
  });

  const play = document.createElement("button");
  play.className = "task-play";
  play.title = "Start focus";
  play.innerHTML = ICONS.play;
  play.addEventListener("click", () => {
    document.dispatchEvent(
      new CustomEvent("focus-request", {
        detail: { title: t.title, minutes: t.duration_min },
      }),
    );
  });

  const edit = document.createElement("button");
  edit.className = "task-del";
  edit.title = "Edit";
  edit.innerHTML = ICONS.edit;
  edit.addEventListener("click", () => renderEdit(row, t));

  const del = document.createElement("button");
  del.className = "task-del";
  del.title = "Delete";
  del.innerHTML = ICONS.trash;
  del.addEventListener("click", () => void deleteTask(t.id));

  row.append(circle, body, dueChip, durationChip, edit, del, play);
  return row;
}

function doneRow(t: Task): HTMLElement {
  const row = document.createElement("div");
  row.className = "trow done";

  const circle = document.createElement("button");
  circle.className = "task-circle filled";
  circle.title = "Reopen";
  circle.addEventListener("click", () => void setTaskDone(t.id, false));

  const body = document.createElement("div");
  body.className = "task-body";
  const title = document.createElement("span");
  title.className = "task-title";
  title.textContent = t.title;
  body.appendChild(title);

  const del = document.createElement("button");
  del.className = "task-del";
  del.title = "Delete";
  del.innerHTML = ICONS.trash;
  del.addEventListener("click", () => void deleteTask(t.id));

  row.append(circle, body, del);
  return row;
}

/** Re-renders the list purely from the store — no fetching here. */
function renderList(): void {
  const all = getTasks();
  const visible =
    state.mode === "day"
      ? all.filter((t) => !t.done && t.due_date === state.selectedDay)
      : all.filter((t) => !t.done && t.due_date === null);
  const done = all.filter((t) => t.done);

  $("t-open-count").textContent = `${all.filter((t) => !t.done).length} open`;
  $("t-list-title").textContent =
    state.mode === "day" ? fmtDayLabel(state.selectedDay) : "Unscheduled";

  const list = $("t-list");
  list.innerHTML = "";
  if (!visible.length && !done.length) {
    list.innerHTML = `<span class="empty">${
      state.mode === "day" ? "Nothing scheduled this day" : "Nothing unscheduled"
    }</span>`;
    return;
  }
  for (const t of visible) list.appendChild(taskRow(t));

  if (done.length) {
    const header = document.createElement("span");
    header.className = "done-header";
    header.textContent = "Done";
    list.appendChild(header);
    for (const t of done) list.appendChild(doneRow(t));
  }
}

function renderEdit(row: HTMLElement, t: Task): void {
  row.classList.add("editing");
  row.innerHTML = "";

  const fields = document.createElement("div");
  fields.className = "edit-fields";

  const titleInput = document.createElement("input");
  titleInput.value = t.title;
  titleInput.placeholder = "Task title";

  const notesInput = document.createElement("input");
  notesInput.value = t.notes ?? "";
  notesInput.placeholder = "Notes (optional)";

  const actions = document.createElement("div");
  actions.className = "edit-actions";

  const save = document.createElement("button");
  save.className = "mini-btn primary";
  save.textContent = "Save";

  const cancel = document.createElement("button");
  cancel.className = "mini-btn";
  cancel.textContent = "Cancel";

  const submit = (): void => {
    const title = titleInput.value.trim();
    if (!title) return;
    void updateTask(t.id, title, notesInput.value.trim() || null);
  };

  save.addEventListener("click", submit);
  cancel.addEventListener("click", () => renderList());
  titleInput.addEventListener("keydown", (e) => e.key === "Enter" && submit());
  notesInput.addEventListener("keydown", (e) => e.key === "Enter" && submit());
  titleInput.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      renderList();
    }
  });

  actions.append(save, cancel);
  fields.append(titleInput, notesInput, actions);
  row.append(fields);
  titleInput.focus();
  titleInput.select();
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
    void import("./tasksStore").then(({ addTask }) =>
      addTask({
        title: t,
        notes: notes.value.trim() || null,
        due_date: state.mode === "day" ? state.selectedDay : null,
        duration_min: null,
      }),
    );
    resetAddForm();
  };
  title.addEventListener("keydown", (e) => e.key === "Enter" && submit());
  notes.addEventListener("keydown", (e) => e.key === "Enter" && submit());
  $("t-add").addEventListener("click", submit);
  $("t-cancel").addEventListener("click", resetAddForm);
}

/* ============================ setup ============================ */

export function focusAddInput(): void {
  ($("t-title") as HTMLInputElement).focus();
}

export function refreshTasksPage(): void {
  void refreshTasks();
}

export function setupTasksPage(hooks: TasksPageHooks): void {
  setupCalendar();
  setupAddForm();

  $("t-close").addEventListener("click", () => hooks.onClose());
  $("t-focus").addEventListener("click", () => {
    document.dispatchEvent(
      new CustomEvent("focus-request", {
        detail: { title: "Focus session", minutes: 25 },
      }),
    );
  });

  document.querySelectorAll<HTMLButtonElement>("#t-mode .seg-btn").forEach((b) =>
    b.addEventListener("click", () => {
      const mode = (b.dataset.mode as "day" | "unscheduled") ?? "day";
      if (mode === state.mode) return;
      state.mode = mode;
      document
        .querySelectorAll<HTMLButtonElement>("#t-mode .seg-btn")
        .forEach((x) => x.classList.toggle("active", x === b));
      renderList();
    }),
  );

  // The store is the single source of truth: re-render on any change,
  // wherever it came from (this page, the dashboard preview, ...).
  subscribeTasks(renderList);

  renderCalendar();
  renderList();
}

export interface TasksPageHooks {
  onClose: () => void;
}
