/** Whether the frontend is running inside macOS's WebView. Kept here rather
 * than spread through components so platform-specific UI stays consistent in
 * both the packaged app and the Vite development server. */
export const IS_MACOS =
  typeof navigator !== "undefined" &&
  (/Macintosh|Mac OS X/.test(navigator.userAgent) || navigator.platform.toUpperCase().includes("MAC"));
