import { useEffect, useRef } from "react";
import { playMoveSound } from "../utils/sfx";
import { findNearestFocusTarget, findNextFocusTarget, type Direction } from "./spatialNav";
import { cycleSelect, isSteppable, stepValue } from "./valueControls";

const STICK_DEADZONE = 0.5;
const REPEAT_DELAY_MS = 320;
const REPEAT_RATE_MS = 140;

const BUTTON_A = 0;
const BUTTON_B = 1;
const DPAD: [number, Direction][] = [
  [12, "up"],
  [13, "down"],
  [14, "left"],
  [15, "right"],
];

interface Options {
  /** Called on Escape / gamepad B. */
  onBack?: () => void;
  /** Disables all handling, e.g. while a modal/dialog owns input. */
  disabled?: boolean;
  /** Restricts spatial navigation to this subtree — set while a drawer or
   * dialog is open so arrow keys/gamepad can't jump to elements behind it.
   * Defaults to the whole document. */
  scope?: ParentNode | null;
}

const KEY_TO_DIRECTION: Record<string, Direction> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
};

let lastFocusRect: DOMRect | null = null;
// A slider or number field only takes left/right after A puts it in edit mode.
let editing: HTMLInputElement | null = null;

function setEditing(el: HTMLInputElement | null) {
  editing?.removeAttribute("data-editing");
  editing = el;
  el?.setAttribute("data-editing", "");
}

function focusIsLost(): boolean {
  const el = document.activeElement;
  return !el || el === document.body || !el.isConnected;
}

// The focused element can vanish (a filtered-out card), dropping focus to body.
function recoverFocus(scope: ParentNode): boolean {
  if (!focusIsLost()) return false;
  findNearestFocusTarget(lastFocusRect, scope)?.focus();
  return true;
}

function move(direction: Direction, scope: ParentNode) {
  if (recoverFocus(scope)) return;
  const current = document.activeElement;
  if (!current) return;
  const next = findNextFocusTarget(current, direction, scope);
  if (!next) return;
  next.focus();
  playMoveSound();
}

function isTextEntry(el: Element | null): boolean {
  return el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement;
}

// Gamepad focus doesn't count as keyboard for :focus-visible, so CSS keys off this.
function setInputMode(mode: "gamepad" | "keyboard" | "pointer") {
  if (document.documentElement.dataset.input !== mode) {
    document.documentElement.dataset.input = mode;
  }
}

function readPads(): Gamepad[] {
  const pads = navigator.getGamepads().filter((pad): pad is Gamepad => pad !== null);
  // Non-standard pads use arbitrary button indices; only use them as a fallback.
  const standard = pads.filter((pad) => pad.mapping === "standard");
  return standard.length > 0 ? standard : pads;
}

function stickDirection(pad: Gamepad): Direction | null {
  const [x = 0, y = 0] = pad.axes;
  if (y < -STICK_DEADZONE) return "up";
  if (y > STICK_DEADZONE) return "down";
  if (x < -STICK_DEADZONE) return "left";
  if (x > STICK_DEADZONE) return "right";
  return null;
}

/** Keyboard + gamepad spatial navigation over any `[data-nav-item]` grid. */
export function useControllerNavigation({ onBack, disabled, scope }: Options = {}) {
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;
  const scopeRef = useRef(scope);
  scopeRef.current = scope;

  useEffect(() => {
    function handleFocusIn(e: FocusEvent) {
      if (editing && e.target !== editing) setEditing(null);
      if (e.target instanceof HTMLElement && e.target.matches("[data-nav-item]")) {
        lastFocusRect = e.target.getBoundingClientRect();
      }
    }
    function handlePointerDown() {
      setInputMode("pointer");
    }
    document.addEventListener("focusin", handleFocusIn);
    window.addEventListener("pointerdown", handlePointerDown, true);
    return () => {
      document.removeEventListener("focusin", handleFocusIn);
      window.removeEventListener("pointerdown", handlePointerDown, true);
    };
  }, []);

  useEffect(() => {
    if (disabled) return;

    function handleKeyDown(e: KeyboardEvent) {
      if (e.isTrusted) setInputMode("keyboard");
      const direction = KEY_TO_DIRECTION[e.key];
      if (direction) {
        const active = document.activeElement;
        if (isTextEntry(active) && (direction === "left" || direction === "right")) {
          return;
        }
        e.preventDefault();
        move(direction, scopeRef.current ?? document);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        onBackRef.current?.();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [disabled]);

  useEffect(() => {
    if (disabled) return;

    let rafId: number | null = null;
    let nextRepeatAt = 0;
    let activeDirection: Direction | null = null;
    let prevPressed: boolean[] = [];

    function pressA(scope: ParentNode) {
      if (recoverFocus(scope)) return;
      const el = document.activeElement;
      if (el instanceof HTMLSelectElement) {
        if (cycleSelect(el)) playMoveSound();
      } else if (isSteppable(el)) {
        setEditing(editing === el ? null : el);
      } else if (el instanceof HTMLElement) {
        el.click();
      }
    }

    // A synthetic Escape lets popovers that close on Escape handle B before onBack.
    function pressB() {
      if (editing) {
        setEditing(null);
        return;
      }
      const target = document.activeElement ?? document.body;
      target.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    }

    function step(direction: Direction, scope: ParentNode) {
      if (editing && editing === document.activeElement && (direction === "left" || direction === "right")) {
        if (stepValue(editing, direction)) playMoveSound();
        return;
      }
      setEditing(null);
      move(direction, scope);
    }

    function poll() {
      const pads = readPads();
      const now = performance.now();
      const activeScope = scopeRef.current ?? document;

      // Steam Input/DS4Windows add a virtual pad beside the real one, so merge them.
      const pressed: boolean[] = [];
      let dpad: Direction | null = null;
      let stick: Direction | null = null;
      for (const pad of pads) {
        pad.buttons.forEach((button, i) => {
          if (button.pressed) pressed[i] = true;
        });
        dpad ??= DPAD.find(([index]) => pad.buttons[index]?.pressed)?.[1] ?? null;
        stick ??= stickDirection(pad);
      }
      const direction = dpad ?? stick;

      const justPressed = (i: number) => pressed[i] && !prevPressed[i];
      if (justPressed(BUTTON_A) || justPressed(BUTTON_B) || direction) setInputMode("gamepad");
      if (justPressed(BUTTON_A)) pressA(activeScope);
      if (justPressed(BUTTON_B)) pressB();
      prevPressed = pressed;

      if (direction) {
        if (direction !== activeDirection) {
          activeDirection = direction;
          step(direction, activeScope);
          nextRepeatAt = now + REPEAT_DELAY_MS;
        } else if (now >= nextRepeatAt) {
          step(direction, activeScope);
          nextRepeatAt = now + REPEAT_RATE_MS;
        }
      } else {
        activeDirection = null;
      }

      rafId = requestAnimationFrame(poll);
    }

    function start() {
      if (rafId == null) rafId = requestAnimationFrame(poll);
    }
    function stop() {
      if (rafId != null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    }

    // `navigator.getGamepads()` has nothing to poll for while Wervik's own
    // window isn't the one with focus — e.g. while a native emulator's own
    // window has focus during actual gameplay — and there's real cost to
    // polling it forever regardless: WebKitGTK's own memory-footprint
    // watchdog killed Wervik's WebView process after several minutes of a
    // native session with a real controller connected, its footprint
    // having grown to 16+ GB while this was the only thing still running
    // continuously in the background the whole time. Pausing it while
    // unfocused isn't just a leak workaround — there's genuinely no UI to
    // navigate meanwhile either way.
    if (document.hasFocus()) start();
    window.addEventListener("focus", start);
    window.addEventListener("blur", stop);

    return () => {
      stop();
      window.removeEventListener("focus", start);
      window.removeEventListener("blur", stop);
    };
  }, [disabled]);
}
