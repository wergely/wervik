import { useEffect, useRef } from "react";
import { playMoveSound } from "../utils/sfx";
import { findNextFocusTarget, type Direction } from "./spatialNav";

const STICK_DEADZONE = 0.5;
const REPEAT_DELAY_MS = 320;
const REPEAT_RATE_MS = 140;

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

function move(direction: Direction, scope: ParentNode) {
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

/** Keyboard + gamepad spatial navigation over any `[data-nav-item]` grid. */
export function useControllerNavigation({ onBack, disabled, scope }: Options = {}) {
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;
  const scopeRef = useRef(scope);
  scopeRef.current = scope;

  useEffect(() => {
    if (disabled) return;

    function handleKeyDown(e: KeyboardEvent) {
      const direction = KEY_TO_DIRECTION[e.key];
      if (direction) {
        if (isTextEntry(document.activeElement) && (direction === "left" || direction === "right")) {
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
    const prevButtons = new Map<number, boolean[]>();

    function poll() {
      const pads = navigator.getGamepads();
      const now = performance.now();

      for (const pad of pads) {
        if (!pad) continue;

        const prev = prevButtons.get(pad.index) ?? [];
        const pressed = pad.buttons.map((b) => b.pressed);

        // A / Cross: activate focused element.
        if (pressed[0] && !prev[0]) {
          const el = document.activeElement;
          if (el instanceof HTMLElement) el.click();
        }
        // B / Circle: back.
        if (pressed[1] && !prev[1]) {
          onBackRef.current?.();
        }

        prevButtons.set(pad.index, pressed);

        const dpad: Direction | null = pressed[12]
          ? "up"
          : pressed[13]
            ? "down"
            : pressed[14]
              ? "left"
              : pressed[15]
                ? "right"
                : null;

        const [x, y] = pad.axes;
        const stick: Direction | null =
          y < -STICK_DEADZONE
            ? "up"
            : y > STICK_DEADZONE
              ? "down"
              : x < -STICK_DEADZONE
                ? "left"
                : x > STICK_DEADZONE
                  ? "right"
                  : null;

        const direction = dpad ?? stick;

        if (direction) {
          const activeScope = scopeRef.current ?? document;
          if (direction !== activeDirection) {
            activeDirection = direction;
            move(direction, activeScope);
            nextRepeatAt = now + REPEAT_DELAY_MS;
          } else if (now >= nextRepeatAt) {
            move(direction, activeScope);
            nextRepeatAt = now + REPEAT_RATE_MS;
          }
        } else {
          activeDirection = null;
        }
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
