/** Short procedural UI sound effects (plain Web Audio oscillators — no
 * audio files, no network) for browsing feedback: moving the selection,
 * opening a game, and pressing Play. Every exported sound is triggered
 * directly from inside a user gesture's own event handler (a keydown, a
 * click), so the lazily-created `AudioContext` never hits autoplay
 * restrictions the way an unprompted background player would. */

let context: AudioContext | null = null;

function ensureContext(): AudioContext {
  if (!context) context = new AudioContext();
  if (context.state === "suspended") context.resume().catch(() => {});
  return context;
}

function beep(frequency: number, startOffset: number, duration: number, type: OscillatorType, peak: number) {
  const ctx = ensureContext();
  const time = ctx.currentTime + startOffset;

  const oscillator = ctx.createOscillator();
  oscillator.type = type;
  oscillator.frequency.setValueAtTime(frequency, time);

  const gain = ctx.createGain();
  gain.gain.setValueAtTime(0, time);
  gain.gain.linearRampToValueAtTime(peak, time + 0.005);
  gain.gain.exponentialRampToValueAtTime(0.0001, time + duration);

  oscillator.connect(gain).connect(ctx.destination);
  oscillator.start(time);
  oscillator.stop(time + duration + 0.02);
}

/** A quiet, short tick for moving the selection between games. */
export function playMoveSound(): void {
  beep(720, 0, 0.045, "square", 0.045);
}

/** A brighter two-note blip for opening a game's detail screen. */
export function playSelectSound(): void {
  beep(660, 0, 0.06, "triangle", 0.08);
  beep(990, 0.05, 0.08, "triangle", 0.08);
}

/** An ascending three-note chime for pressing Play. */
export function playConfirmSound(): void {
  beep(523.25, 0, 0.09, "square", 0.07);
  beep(659.25, 0.07, 0.09, "square", 0.07);
  beep(783.99, 0.14, 0.16, "square", 0.07);
}
