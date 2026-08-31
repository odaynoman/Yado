//! Single source of truth for tasks.
//!
//! One DB fetch (`all`) fills one cached array; every view (dashboard
//! preview, tasks page) subscribes and re-renders from this store when it
//! changes. All mutations go through here, so a change made anywhere is
//! visible everywhere immediately.

import { api } from "./api";
import type { Task } from "./types";

let tasks: Task[] = [];
const listeners = new Set<() => void>();

function emit(): void {
  listeners.forEach((fn) => fn());
}

/** Subscribes a view to task changes. Returns an unsubscribe function. */
export function subscribeTasks(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

export function getTasks(): Task[] {
  return tasks;
}

export function openTasks(): Task[] {
  return tasks.filter((t) => !t.done);
}

export function doneTasks(): Task[] {
  return tasks.filter((t) => t.done);
}

export async function refreshTasks(): Promise<void> {
  tasks = await api.listTasks("all", null).catch(() => tasks);
  emit();
}

export async function addTask(input: {
  title: string;
  notes: string | null;
  due_date: string | null;
  duration_min: number | null;
}): Promise<void> {
  await api.addTask(input).catch(() => {});
  await refreshTasks();
}

export async function setTaskDone(id: number, done: boolean): Promise<void> {
  await api.setTaskDone(id, done).catch(() => {});
  await refreshTasks();
}

export async function setTaskDuration(
  id: number,
  duration_min: number | null,
): Promise<void> {
  await api.setTaskDuration(id, duration_min).catch(() => {});
  await refreshTasks();
}

/** Partial update: merges the patch with the stored task, then syncs. */
export async function updateTask(
  id: number,
  patch: Partial<Pick<Task, "title" | "notes" | "due_date" | "duration_min">>,
): Promise<void> {
  const t = tasks.find((x) => x.id === id);
  if (!t) return;
  const merged = {
    title: patch.title ?? t.title,
    notes: patch.notes !== undefined ? patch.notes : t.notes,
    due_date: patch.due_date !== undefined ? patch.due_date : t.due_date,
    duration_min:
      patch.duration_min !== undefined ? patch.duration_min : t.duration_min,
  };
  await api.updateTask(id, merged).catch(() => {});
  await refreshTasks();
}

export async function deleteTask(id: number): Promise<void> {
  await api.deleteTask(id).catch(() => {});
  await refreshTasks();
}
