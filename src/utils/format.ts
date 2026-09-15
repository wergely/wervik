import type { Game } from "../types";

export function sourceLabel(game: Game): string {
  if (game.source === "retro") return game.platform ?? "Retro";
  if (game.source === "xbox") return "Xbox";
  return game.source === "battlenet" ? "Battle.net" : "Steam";
}

export function formatPlaytime(minutes: number | null): string | null {
  if (!minutes || minutes <= 0) return null;
  const hours = minutes / 60;
  if (hours < 1) return `${minutes} min`;
  if (hours < 10) return `${hours.toFixed(1)} hrs`;
  return `${Math.round(hours)} hrs`;
}

export function formatLastPlayed(unixSeconds: number | null): string | null {
  if (!unixSeconds) return null;
  const then = unixSeconds * 1000;
  const diffMs = Date.now() - then;
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffDays <= 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  if (diffDays < 7) return `${diffDays} days ago`;
  if (diffDays < 30) {
    const weeks = Math.floor(diffDays / 7);
    return weeks === 1 ? "1 week ago" : `${weeks} weeks ago`;
  }
  if (diffDays < 365) {
    const months = Math.floor(diffDays / 30);
    return months === 1 ? "1 month ago" : `${months} months ago`;
  }
  const years = Math.floor(diffDays / 365);
  return years === 1 ? "1 year ago" : `${years} years ago`;
}

export function formatSize(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  if (gb < 1) {
    const mb = bytes / 1024 ** 2;
    return `${mb.toFixed(0)} MB`;
  }
  return `${gb.toFixed(1)} GB`;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
