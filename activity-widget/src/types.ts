export interface DayCount {
  date: string;
  count: number;
}

export interface AppCount {
  app: string;
  count: number;
}

export interface Stats {
  today: number;
  total: number;
  streak: number;
  heatmap: DayCount[];
  top_apps: AppCount[];
}

export interface Task {
  id: number;
  title: string;
  notes: string | null;
  due_date: string | null;
  duration_min: number | null;
  done: boolean;
  created_ts: number;
}

export type TaskFilter = "day" | "unscheduled" | "open" | "recent";

export interface WidgetConfig {
  mode: "all" | "allowlist";
  apps: string[];
  watch_folders: string[] | null;
}

/** Payload emitted when a task's play button requests a focus session. */
export interface FocusRequest {
  title: string;
  minutes: number | null;
}
