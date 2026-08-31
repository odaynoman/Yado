import { invoke } from "@tauri-apps/api/core";
import type { AppCount, Stats, Task, TaskFilter, WidgetConfig } from "./types";

export interface AddTaskInput {
  title: string;
  notes: string | null;
  due_date: string | null;
  duration_min: number | null;
}

export interface SaveConfigInput {
  mode: string;
  apps: string[];
  watch_folders: string[] | null;
}

/** Typed wrappers around every backend command. */
export const api = {
  expandNow: () => invoke("expand_now"),
  collapseNow: () => invoke("collapse_now"),
  setPin: (pin: boolean) => invoke("set_pin", { pin }),
  setIslandSize: (width: number, height: number) =>
    invoke("set_island_size", { width, height }),

  getStats: () => invoke<Stats>("get_stats"),
  recentApps: () => invoke<AppCount[]>("recent_apps"),
  logEvent: (appName: string, eventType: string, detail: string | null) =>
    invoke<number>("log_event", { appName, eventType, detail }),

  getConfig: () => invoke<WidgetConfig>("get_config"),
  saveConfig: (args: SaveConfigInput) => invoke("save_config", { args }),

  addTask: (input: AddTaskInput) =>
    invoke<Task>("add_task", {
      title: input.title,
      notes: input.notes,
      dueDate: input.due_date,
      durationMin: input.duration_min,
    }),
  listTasks: (filter: TaskFilter, day: string | null) =>
    invoke<Task[]>("list_tasks", { filter, day }),
  setTaskDone: (id: number, done: boolean) =>
    invoke("set_task_done", { id, done }),
  setTaskDuration: (id: number, duration_min: number | null) =>
    invoke("set_task_duration", { id, duration_min }),
  updateTask: (
    id: number,
    input: {
      title: string;
      notes: string | null;
      due_date: string | null;
      duration_min: number | null;
    },
  ) =>
    invoke("update_task", {
      id,
      title: input.title,
      notes: input.notes,
      dueDate: input.due_date,
      durationMin: input.duration_min,
    }),
  deleteTask: (id: number) => invoke("delete_task", { id }),

  getAppIcon: (app: string) => invoke<string | null>("get_app_icon", { app }),
  notifyFocusDone: (title: string) => invoke("notify_focus_done", { title }),
  autostartEnabled: () => invoke<boolean>("autostart_enabled"),
  autostartSet: (enable: boolean) => invoke("autostart_set", { enable }),

  mediaPlay: () => invoke("media_play"),
  mediaPause: () => invoke("media_pause"),
  mediaNext: () => invoke("media_next"),
  mediaPrev: () => invoke("media_prev"),
  mediaSeek: (positionSec: number) => invoke("media_seek", { positionSec }),
};
