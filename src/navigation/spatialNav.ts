export type Direction = "up" | "down" | "left" | "right";

export const NAV_ITEM_SELECTOR = "[data-nav-item]";

/** Finds the best next focus target in `container` relative to `current`,
 * using rectangle geometry — a simple directional nearest-neighbor search
 * that works across mixed layouts (rows + grids) without manual indices. */
export function findNextFocusTarget(
  current: Element,
  direction: Direction,
  container: ParentNode = document,
): HTMLElement | null {
  const currentRect = current.getBoundingClientRect();
  const candidates = Array.from(container.querySelectorAll<HTMLElement>(NAV_ITEM_SELECTOR)).filter(
    (el) => el !== current && isVisible(el),
  );

  let best: HTMLElement | null = null;
  let bestScore = Infinity;

  for (const candidate of candidates) {
    const rect = candidate.getBoundingClientRect();
    const score = directionalScore(currentRect, rect, direction);
    if (score !== null && score < bestScore) {
      bestScore = score;
      best = candidate;
    }
  }

  return best;
}

export function findNearestFocusTarget(
  rect: DOMRect | null,
  container: ParentNode = document,
): HTMLElement | null {
  const candidates = Array.from(container.querySelectorAll<HTMLElement>(NAV_ITEM_SELECTOR)).filter(isVisible);
  if (!rect) return candidates[0] ?? null;
  const cx = rect.left + rect.width / 2;
  const cy = rect.top + rect.height / 2;
  let best: HTMLElement | null = null;
  let bestDistance = Infinity;
  for (const candidate of candidates) {
    const r = candidate.getBoundingClientRect();
    const distance = Math.hypot(r.left + r.width / 2 - cx, r.top + r.height / 2 - cy);
    if (distance < bestDistance) {
      bestDistance = distance;
      best = candidate;
    }
  }
  return best;
}

function isVisible(el: HTMLElement): boolean {
  return el.offsetParent !== null || el.getClientRects().length > 0;
}

function directionalScore(
  from: DOMRect,
  to: DOMRect,
  direction: Direction,
): number | null {
  const fromCenter = { x: from.left + from.width / 2, y: from.top + from.height / 2 };
  const toCenter = { x: to.left + to.width / 2, y: to.top + to.height / 2 };
  const dx = toCenter.x - fromCenter.x;
  const dy = toCenter.y - fromCenter.y;

  // Primary-axis displacement must point the right way; a small tolerance
  // lets slightly-misaligned rows still count as "forward".
  const tolerance = 4;
  switch (direction) {
    case "up":
      if (dy > -tolerance) return null;
      break;
    case "down":
      if (dy < tolerance) return null;
      break;
    case "left":
      if (dx > -tolerance) return null;
      break;
    case "right":
      if (dx < tolerance) return null;
      break;
  }

  const primary = direction === "up" || direction === "down" ? Math.abs(dy) : Math.abs(dx);
  const secondary = direction === "up" || direction === "down" ? Math.abs(dx) : Math.abs(dy);

  // Weight lateral drift heavily so movement prefers the nearest item in the
  // intended row/column over one that's merely closer in a straight line.
  return primary + secondary * 3;
}
