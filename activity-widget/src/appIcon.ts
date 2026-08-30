import { api } from "./api";

/** Resolves (and caches) an app's extracted icon as a data URL. */
const cache = new Map<string, Promise<string | null>>();

export function appIconUrl(app: string): Promise<string | null> {
  const key = app.toLowerCase();
  let promise = cache.get(key);
  if (!promise) {
    promise = (async () => {
      const direct = await api.getAppIcon(key).catch(() => null);
      if (direct) return direct;
      return api.getAppIcon(`${key}.exe`).catch(() => null);
    })();
    cache.set(key, promise);
  }
  return promise;
}

/** Fills an <img>-like element with the app icon, falling back to a glyph. */
export function applyAppIcon(
  target: HTMLElement,
  app: string,
  glyph = "&#x266B;",
): void {
  target.classList.add("icon-loading");
  void appIconUrl(app).then((url) => {
    if (url) {
      const img = document.createElement("img");
      img.src = url;
      img.alt = "";
      target.replaceChildren(img);
      target.classList.add("has-img");
    } else {
      target.innerHTML = glyph;
    }
    target.classList.remove("icon-loading");
  });
}
