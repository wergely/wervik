/** A tiny procedural chiptune loop, played with plain Web Audio oscillators
 * — no audio files, no network, nothing to bundle or license. Ten named
 * "tracks" are generated (see `PATTERN_NAMES`/`generateLeadPattern`), each
 * with its own waveform, register and note density so they're actually
 * distinguishable from one another; the loop auto-advances to the next one
 * every time the current one repeats, and `next()` lets the user skip to
 * another on demand. The bass plays a fixed i–VI–VII–v progression in A
 * minor underneath every track — every generated melody is drawn from the
 * A natural minor scale, so it always harmonizes with that progression no
 * matter which notes come out.
 *
 * Uses the standard "look-ahead" scheduling pattern (schedule a little
 * ahead of real time on a fast interval) instead of one `setTimeout` per
 * note, so playback stays tight even when the tab/window is throttled.
 */

const TEMPO_BPM = 128;
const STEP_SECONDS = 60 / TEMPO_BPM / 2; // eighth notes
const SCHEDULE_AHEAD_SECONDS = 0.1;
const SCHEDULE_INTERVAL_MS = 25;
const STEPS_PER_PATTERN = 16;
// Each 16-step pattern is only ~3.75s on its own — far too short to read as
// a "song" — so it repeats several times before the loop auto-advances to
// the next track. `next()` can still skip immediately at any point.
const LOOPS_PER_TRACK = 16;

const NOTE = {
  A2: 110.0,
  E3: 164.81,
  F3: 174.61,
  G3: 196.0,
} as const;

// A natural minor across three octaves — low enough to still sound
// chip-like at the bottom, bright without being shrill at the top.
const SCALE = [
  164.81, // E3
  174.61, // F3
  196.0, // G3
  220.0, // A3
  246.94, // B3
  261.63, // C4
  293.66, // D4
  329.63, // E4
  349.23, // F4
  392.0, // G4
  440.0, // A4
  493.88, // B4
  523.25, // C5
  587.33, // D5
  659.25, // E5
];

type Step = number | null;

interface PatternFlavor {
  name: string;
  waveform: OscillatorType;
  /** Index into `SCALE` the melody wanders around. */
  centerDegree: number;
  /** 0–1: higher means sparser, more rests. */
  restBias: number;
  seed: number;
}

// Each track gets its own waveform, register and density so switching
// tracks is actually audible, not just a different arrangement of the same
// timbre in the same octave.
const PATTERN_FLAVORS: PatternFlavor[] = [
  { name: "Byte Bounce", waveform: "square", centerDegree: 7, restBias: 0.3, seed: 1000 },
  { name: "Pixel Drift", waveform: "triangle", centerDegree: 10, restBias: 0.45, seed: 1097 },
  { name: "Turbo Sprint", waveform: "square", centerDegree: 4, restBias: 0.12, seed: 1194 },
  { name: "Neon Nocturne", waveform: "sawtooth", centerDegree: 8, restBias: 0.4, seed: 1291 },
  { name: "8-Bit Lullaby", waveform: "triangle", centerDegree: 2, restBias: 0.55, seed: 1388 },
  { name: "Dungeon Shuffle", waveform: "square", centerDegree: 5, restBias: 0.25, seed: 1485 },
  { name: "Coin Rush", waveform: "square", centerDegree: 11, restBias: 0.1, seed: 1582 },
  { name: "Moonlit Cartridge", waveform: "sawtooth", centerDegree: 3, restBias: 0.45, seed: 1679 },
  { name: "Save Point Serenade", waveform: "triangle", centerDegree: 6, restBias: 0.5, seed: 1776 },
  { name: "Overclocked", waveform: "sawtooth", centerDegree: 9, restBias: 0.08, seed: 1873 },
];

export const PATTERN_NAMES: readonly string[] = PATTERN_FLAVORS.map((flavor) => flavor.name);

/** Deterministic PRNG (mulberry32) — patterns stay the same across runs
 * instead of reshuffling on every launch. */
function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Generates one 16-step melody: a bounded random walk across `SCALE`
 * centered on the flavor's register (so it moves smoothly instead of
 * jumping around) with occasional rests, more likely on off-beats than on
 * the strong beat of each quarter note. */
function generateLeadPattern(flavor: PatternFlavor): Step[] {
  const random = mulberry32(flavor.seed);
  const steps: Step[] = [];
  let degree = flavor.centerDegree;
  for (let i = 0; i < STEPS_PER_PATTERN; i++) {
    const onStrongBeat = i % 4 === 0;
    if (random() < (onStrongBeat ? flavor.restBias * 0.15 : flavor.restBias)) {
      steps.push(null);
      continue;
    }
    const move = Math.floor(random() * 5) - 2; // -2..2 scale degrees
    degree = Math.min(SCALE.length - 1, Math.max(0, degree + move));
    steps.push(SCALE[degree]);
  }
  return steps;
}

const LEAD_PATTERNS: Step[][] = PATTERN_FLAVORS.map(generateLeadPattern);

// A minor i (Am) – VI (F) – VII (G) – v (Em), one held note per bar.
const BASS_PATTERN: Step[] = [
  NOTE.A2, null, null, null, NOTE.F3, null, null, null,
  NOTE.G3, null, null, null, NOTE.E3, null, null, null,
];

export interface ChiptunePlayer {
  start(): void;
  stop(): void;
  /** Skips to the next track immediately. */
  next(): void;
  setVolume(volume: number): void;
  dispose(): void;
}

/** `onTrackChange` fires with the new track's index into `PATTERN_NAMES`
 * whenever it changes — from `next()` or from the loop auto-advancing. */
export function createChiptunePlayer(
  initialVolume: number,
  onTrackChange?: (index: number) => void,
): ChiptunePlayer {
  let context: AudioContext | null = null;
  let masterGain: GainNode | null = null;
  let intervalId: number | null = null;
  let nextStepTime = 0;
  let stepIndex = 0;
  let patternIndex = 0;
  let loopCount = 0;
  let volume = initialVolume;
  let running = false;

  function ensureContext(): AudioContext {
    if (!context) {
      context = new AudioContext();
      masterGain = context.createGain();
      masterGain.gain.value = volume;
      masterGain.connect(context.destination);
    }
    return context;
  }

  function playNote(frequency: number, time: number, duration: number, type: OscillatorType, peak: number) {
    if (!context || !masterGain) return;
    const oscillator = context.createOscillator();
    oscillator.type = type;
    oscillator.frequency.setValueAtTime(frequency, time);

    const noteGain = context.createGain();
    noteGain.gain.setValueAtTime(0, time);
    noteGain.gain.linearRampToValueAtTime(peak, time + 0.01);
    noteGain.gain.exponentialRampToValueAtTime(0.001, time + duration);

    oscillator.connect(noteGain).connect(masterGain);
    oscillator.start(time);
    oscillator.stop(time + duration + 0.02);
  }

  function goToPattern(index: number) {
    patternIndex = index;
    stepIndex = 0;
    loopCount = 0;
    onTrackChange?.(patternIndex);
  }

  function scheduleStep(time: number) {
    const flavor = PATTERN_FLAVORS[patternIndex];
    const lead = LEAD_PATTERNS[patternIndex][stepIndex];
    if (lead != null) playNote(lead, time, STEP_SECONDS * 0.9, flavor.waveform, 0.05);
    const bass = BASS_PATTERN[stepIndex];
    if (bass != null) playNote(bass, time, STEP_SECONDS * 3.5, "triangle", 0.09);

    stepIndex += 1;
    if (stepIndex >= STEPS_PER_PATTERN) {
      stepIndex = 0;
      loopCount += 1;
      if (loopCount >= LOOPS_PER_TRACK) {
        goToPattern((patternIndex + 1) % LEAD_PATTERNS.length);
      }
    }
  }

  function tick() {
    if (!context) return;
    while (nextStepTime < context.currentTime + SCHEDULE_AHEAD_SECONDS) {
      scheduleStep(nextStepTime);
      nextStepTime += STEP_SECONDS;
    }
  }

  return {
    start() {
      const ctx = ensureContext();
      if (ctx.state === "suspended") ctx.resume().catch(() => {});
      if (running) return;
      running = true;
      nextStepTime = ctx.currentTime + 0.1;
      intervalId = window.setInterval(tick, SCHEDULE_INTERVAL_MS);
    },
    stop() {
      running = false;
      if (intervalId !== null) {
        window.clearInterval(intervalId);
        intervalId = null;
      }
      context?.suspend().catch(() => {});
    },
    next() {
      goToPattern((patternIndex + 1) % LEAD_PATTERNS.length);
    },
    setVolume(next: number) {
      volume = Math.min(1, Math.max(0, next));
      if (context && masterGain) {
        masterGain.gain.setTargetAtTime(volume, context.currentTime, 0.05);
      }
    },
    dispose() {
      this.stop();
      context?.close().catch(() => {});
      context = null;
      masterGain = null;
    },
  };
}
