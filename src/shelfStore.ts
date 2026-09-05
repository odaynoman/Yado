//! Single source of truth for the file shelf: a temporary stash.
//!
//! Items are references to files/folders on disk (dropping never moves
//! anything). Nothing is persisted — the shelf lives for the current
//! session only, and an item leaves it once its drag-out is delivered.
//! Views subscribe and re-render from the store.

export interface ShelfItem {
  path: string;
  name: string;
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

export function shelfAdd(path: string): void {
  if (items.some((i) => i.path.toLowerCase() === path.toLowerCase())) return;
  items = [
    {
      path,
      name: path.split(/[\\/]/).pop() ?? path,
    },
    ...items,
  ];
  emit();
}

export function shelfRemove(path: string): void {
  items = items.filter((i) => i.path !== path);
  emit();
}
