import type { Direction } from "./spatialNav";

export function isSteppable(el: Element | null): el is HTMLInputElement {
  return el instanceof HTMLInputElement && (el.type === "range" || el.type === "number") && !el.disabled;
}

export function stepValue(input: HTMLInputElement, direction: Direction): boolean {
  if (direction !== "left" && direction !== "right") return false;
  const before = input.value;
  try {
    if (direction === "right") input.stepUp();
    else input.stepDown();
  } catch {
    return false;
  }
  if (input.value === before) return false;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}

export function cycleSelect(select: HTMLSelectElement): boolean {
  const count = select.options.length;
  if (select.disabled || count < 2) return false;
  select.selectedIndex = (select.selectedIndex + 1) % count;
  select.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}
