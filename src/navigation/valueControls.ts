import type { Direction } from "./spatialNav";

type AdjustableInput = HTMLInputElement & { type: "range" | "number" };

function isAdjustableInput(el: Element | null): el is AdjustableInput {
  return el instanceof HTMLInputElement && (el.type === "range" || el.type === "number");
}

export function isAdjustable(el: Element | null): el is HTMLSelectElement | AdjustableInput {
  return el instanceof HTMLSelectElement || isAdjustableInput(el);
}

function stepSelect(select: HTMLSelectElement, delta: number, wrap: boolean): boolean {
  const count = select.options.length;
  if (count === 0) return false;
  let next = select.selectedIndex + delta;
  if (wrap) next = (next + count) % count;
  if (next < 0 || next >= count || next === select.selectedIndex) return false;
  select.selectedIndex = next;
  select.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}

function stepInput(input: AdjustableInput, delta: number): boolean {
  const before = input.value;
  try {
    if (delta > 0) input.stepUp();
    else input.stepDown();
  } catch {
    return false;
  }
  if (input.value === before) return false;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}

// Returns false at either end of the range so focus can still leave the control.
export function adjustValue(el: Element | null, direction: Direction): boolean {
  if (direction !== "left" && direction !== "right") return false;
  if (!isAdjustable(el) || el.disabled) return false;
  const delta = direction === "right" ? 1 : -1;
  return el instanceof HTMLSelectElement ? stepSelect(el, delta, false) : stepInput(el, delta);
}

export function cycleSelect(select: HTMLSelectElement): boolean {
  if (select.disabled) return false;
  return stepSelect(select, 1, true);
}
