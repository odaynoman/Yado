import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { ICONS } from "./icons";
import { applyAppIcon } from "./appIcon";

const $ = (id: string): HTMLElement => document.getElementById(id)!;

export interface MediaInfo {
  title: string;
  artist: string;
  app: string;
  playing: boolean;
  positionSec: number;
  durationSec: number;
}

let info: MediaInfo | null = null;
let scrubbing = false;
/** Hidden while a focus session owns the bottom strip. */
let suppressed = false;
/** Smooth local playback clock between backend polls. */
let localPosition = 0;
let lastTick = 0;

export function mediaActive(): boolean {
  return info !== null;
}

export function mediaTitle(): string | null {
  return info?.title ?? null;
}

export function mediaPlaying(): boolean {
  return info?.playing ?? false;
}

/** Focus sessions take over the bottom strip; media resumes after. */
export function setMediaSuppressed(value: boolean): void {
  suppressed = value;
  render();
}

function fmt(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function render(): void {
  const card = $("media-card");
  card.hidden = suppressed || info === null;
  if (!info) return;

  $("media-title").textContent = info.title;
  $("media-artist").textContent = info.artist;
  applyAppIcon($("media-art"), `${info.app}.exe`, "&#x266B;");

  const toggle = $("media-toggle");
  toggle.innerHTML = info.playing ? ICONS.pause : ICONS.play;

  if (!scrubbing) {
    const slider = $("media-seek") as HTMLInputElement;
    slider.max = String(Math.floor(info.durationSec));
    slider.value = String(Math.floor(info.positionSec));
  }
  $("media-pos").textContent = fmt(info.positionSec);
  $("media-dur").textContent = fmt(info.durationSec);
}

export function setupMedia(onChange?: () => void): void {
  void listen<MediaInfo | null>("media://state", ({ payload }) => {
    info = payload;
    if (info) localPosition = info.positionSec;
    render();
    onChange?.();
  });

  $("media-toggle").addEventListener("click", () => {
    if (!info) return;
    void (info.playing ? api.mediaPause() : api.mediaPlay());
  });
  $("media-prev").addEventListener("click", () => void api.mediaPrev());
  $("media-next").addEventListener("click", () => void api.mediaNext());

  const slider = $("media-seek") as HTMLInputElement;
  slider.addEventListener("pointerdown", () => (scrubbing = true));
  slider.addEventListener("pointerup", () => (scrubbing = false));
  slider.addEventListener("change", () => {
    const sec = Number(slider.value);
    if (info) {
      info.positionSec = sec;
      localPosition = sec;
      lastTick = Date.now();
    }
    void api.mediaSeek(sec);
    scrubbing = false;
  });

  // Local playback clock keeps the seek bar moving between backend polls.
  setInterval(() => {
    if (!info?.playing || scrubbing) return;
    const now = Date.now();
    if (lastTick) {
      localPosition = Math.min(
        localPosition + (now - lastTick) / 1000,
        info.durationSec,
      );
      info.positionSec = localPosition;
      slider.value = String(Math.floor(info.positionSec));
      $("media-pos").textContent = fmt(info.positionSec);
    }
    lastTick = now;
  }, 500);
}
