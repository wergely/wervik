import type { Direction } from "./spatialNav";

type EditableControl = HTMLInputElement | HTMLSelectElement;

interface EditSession {
  el: EditableControl;
  initialValue: string;
}

const FAST_STEP_AFTER_REPEATS = 8;
const FASTER_STEP_AFTER_REPEATS = 24;

let session: EditSession | null = null;

export function isEditable(el: Element | null): el is EditableControl {
  if (el instanceof HTMLSelectElement) return !el.disabled && el.options.length > 1;
  return el instanceof HTMLInputElement && (el.type === "range" || el.type === "number") && !el.disabled;
}

export function editingControl(): EditableControl | null {
  return session?.el ?? null;
}

export function startEditing(el: EditableControl) {
  endEditing("commit");
  session = { el, initialValue: el.value };
  el.setAttribute("data-editing", "");
}

function notify(el: EditableControl) {
  if (el instanceof HTMLInputElement) el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
}

// Goes through the prototype setter so React's value tracker still sees a change.
function setNativeValue(el: EditableControl, value: string) {
  const prototype = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(el, value);
}

// Selects only fire `change` on commit so browsing options doesn't save each one.
export function endEditing(outcome: "commit" | "cancel") {
  if (!session) return;
  const { el, initialValue } = session;
  session = null;
  el.removeAttribute("data-editing");
  if (!el.isConnected || el.value === initialValue) return;
  if (outcome === "cancel") setNativeValue(el, initialValue);
  if (outcome === "cancel" || el instanceof HTMLSelectElement) notify(el);
}

function stepCount(el: EditableControl, repeats: number): number {
  if (!(el instanceof HTMLInputElement) || el.type !== "number") return 1;
  if (repeats >= FASTER_STEP_AFTER_REPEATS) return 100;
  if (repeats >= FAST_STEP_AFTER_REPEATS) return 10;
  return 1;
}

export function stepEditing(direction: Direction, repeats: number): boolean {
  const el = session?.el;
  if (!el || (direction !== "left" && direction !== "right")) return false;
  const delta = direction === "right" ? 1 : -1;
  if (el instanceof HTMLSelectElement) {
    const next = el.selectedIndex + delta;
    if (next < 0 || next >= el.options.length) return false;
    el.selectedIndex = next;
    return true;
  }
  const before = el.value;
  try {
    if (delta > 0) el.stepUp(stepCount(el, repeats));
    else el.stepDown(stepCount(el, repeats));
  } catch {
    return false;
  }
  if (el.value === before) return false;
  notify(el);
  return true;
}
