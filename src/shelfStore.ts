//! Single source of truth for the file shelf.
//!
//! Items are references to files/folders on disk (dropping never moves
//! anything). The list persists to shelf.json in app data and survives
//! restarts. Views subscribe and re-render from the store.

import { invoke } from "@tauri-apps/api/core";

export interface ShelfItem {
  path: string;
  name: string;
  added_at: number;
}

let items: ShelfItem[] = [];
const listeners = new Set<() => void>();

function emit(): void {
  listeners.forEach((fn) => fn());
}

export function subscribeShelf(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

export function getShelf(): ShelfItem[] {
  return items;
}

export async function loadShelf(): Promise<void> {
  items = await invoke<ShelfItem[]>("shelf_load").catch(() => items);
  emit();
}

export function shelfAdd(path: string): void {
  if (items.some((i) => i.path.toLowerCase() === path.toLowerCase())) return;
  items = [
    {
      path,
      name: path.split(/[\\/]/).pop() ?? path,
      added_at: Date.now(),
    },
    ...items,
  ];
  void invoke("shelf_save", { items }).catch(() => {});
  emit();
}

export function shelfRemove(path: string): void {
  items = items.filter((i) => i.path !== path);
  void invoke("shelf_save", { items }).catch(() => {});
  emit();
}
