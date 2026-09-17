import { siBattledotnet, siEa, siSteam } from "simple-icons";
import type { GameSource } from "../types";

// Eagerly bundles every retro console icon, keyed by the same snake_case
// key `Platform::key()` sends over the Tauri boundary (see
// `retro::platform::Platform::key` in the Rust backend) — so looking one up
// is a plain object lookup, no separate name-mapping table to keep in sync.
// Source: github.com/KyleBing/retro-game-console-icons (GPL-3.0 — see
// CREDITS.md at the repo root).
const iconModules = import.meta.glob("../assets/platform-icons/*.png", { eager: true, import: "default" }) as Record<
  string,
  string
>;
const retroIcons: Record<string, string> = {};
for (const path in iconModules) {
  const key = path.split("/").pop()?.replace(/\.png$/, "");
  if (key) retroIcons[key] = iconModules[path];
}

// Platform-page artwork can be supplied independently from the compact icon
// used in badges and achievement rows. This keeps a cinematic card image from
// being squeezed into a tiny source badge.
const artModules = import.meta.glob("../assets/platform-art/*.webp", { eager: true, import: "default" }) as Record<
  string,
  string
>;
const artThumbModules = import.meta.glob("../assets/platform-art/thumbs/*.webp", {
  eager: true,
  import: "default",
}) as Record<string, string>;
const retroArt: Record<string, string> = {};
for (const path in artModules) {
  const key = path.split("/").pop()?.replace(/\.webp$/, "");
  if (key) retroArt[key] = artModules[path];
}
const retroArtThumb: Record<string, string> = {};
for (const path in artThumbModules) {
  const key = path.split("/").pop()?.replace(/\.webp$/, "");
  if (key) retroArtThumb[key] = artThumbModules[path];
}

export function platformArtworkUrl(
  platformKey?: string | null,
  variant: "full" | "thumb" = "full",
): string | undefined {
  if (!platformKey) return undefined;
  return variant === "thumb" ? retroArtThumb[platformKey] ?? retroArt[platformKey] : retroArt[platformKey];
}

// Simple Icons has no Xbox entry (removed at the trademark holder's
// request), so it falls back to the generic glyph below like an
// unrecognized retro platform (e.g. Atari Jaguar, missing from the console
// pack) would.
const BRAND_ICONS: Partial<Record<GameSource, { title: string; path: string; hex: string }>> = {
  steam: siSteam,
  battlenet: siBattledotnet,
  ea: siEa,
};

interface PlatformIconProps {
  source: GameSource;
  /** `Game.platformKey` — only meaningful (and only looked up) when
   * `source === "retro"`. */
  platformKey?: string | null;
  /** Use platform-page artwork instead of the compact console icon when available. */
  art?: boolean;
  /** Selects the lighter card thumbnail or the larger detail-view artwork. */
  artVariant?: "full" | "thumb";
  size?: number;
  className?: string;
}

/** The one place that decides how to visually represent "where does this
 * game come from": a real brand mark for Steam/Battle.net/EA, a real
 * console logo for a recognized retro platform, or a plain generic
 * controller glyph when neither is available. */
export function PlatformIcon({
  source,
  platformKey,
  art = false,
  artVariant = "full",
  size = 20,
  className,
}: PlatformIconProps) {
  if (source === "retro") {
    const imageUrl = art ? platformArtworkUrl(platformKey, artVariant) : undefined;
    const iconUrl = platformKey ? retroIcons[platformKey] : undefined;
    if (imageUrl || iconUrl) {
      return (
        <img
          src={imageUrl ?? iconUrl}
          alt=""
          draggable={false}
          className={className}
          loading={imageUrl ? "lazy" : undefined}
          decoding="async"
          style={{
            width: imageUrl ? "100%" : size,
            height: imageUrl ? "100%" : size,
            objectFit: imageUrl ? "cover" : "contain",
          }}
        />
      );
    }
    return <GenericIcon size={size} className={className} />;
  }

  const brand = BRAND_ICONS[source];
  if (brand) {
    return (
      <svg
        viewBox="0 0 24 24"
        width={size}
        height={size}
        className={className}
        role="img"
        aria-label={brand.title}
        style={{ fill: `#${brand.hex}`, flexShrink: 0 }}
      >
        <path d={brand.path} />
      </svg>
    );
  }

  return <GenericIcon size={size} className={className} />;
}

function GenericIcon({ size, className }: { size: number; className?: string }) {
  return (
    <span
      className={className}
      style={{ fontSize: size * 0.85, lineHeight: 1, display: "inline-flex", flexShrink: 0 }}
      aria-hidden="true"
    >
      🎮
    </span>
  );
}
