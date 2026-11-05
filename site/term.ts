/**
 * Paints a frame the wasm renderer exported (`snapshot::runs_json`) into a cell grid, and owns the wasm module.
 *
 * A run is `[text, fg, bg, mods, width]`. ASCII runs are plain text in a monospace line. A non-ASCII run is one glyph
 * with its cell width: it gets a fixed `width`-ch box, so box drawing, braille and CJK stay on the grid whatever the
 * browser's fallback font measures. Text goes in through `textContent` only — nothing a widget draws is parsed as HTML.
 */

export type Color = number | string | null;
export type Run = [text: string, fg: Color, bg: Color, mods: number, width: number];
export interface Frame {
  cols: number;
  rows: number;
  lines: Run[][];
}

/** ratatui `Modifier` bits. */
export const MOD = {
  BOLD: 1,
  DIM: 2,
  ITALIC: 4,
  UNDERLINED: 8,
  SLOW_BLINK: 16,
  RAPID_BLINK: 32,
  REVERSED: 64,
  HIDDEN: 128,
  CROSSED_OUT: 256,
} as const;

export interface TermPalette {
  name: 'dark' | 'light';
  fg: string;
  bg: string;
  /** The 16 named colours, in ANSI order (black, red, green, yellow, blue, magenta, cyan, gray, then the bright 8). */
  ansi: string[];
}

/** A calm dark terminal: the 2lab dashboard's dark surface with readable named colours. */
export const DARK: TermPalette = {
  name: 'dark',
  fg: '#e6e6e9',
  bg: '#141416',
  ansi: [
    '#2a2a2e',
    '#f87171',
    '#4ade80',
    '#facc15',
    '#60a5fa',
    '#c084fc',
    '#22d3ee',
    '#c4c4cc',
    '#6b6b75',
    '#fca5a5',
    '#86efac',
    '#fde68a',
    '#93c5fd',
    '#d8b4fe',
    '#67e8f9',
    '#ffffff',
  ],
};

/** A light terminal: named colours dark enough to read on white. */
export const LIGHT: TermPalette = {
  name: 'light',
  fg: '#18181b',
  bg: '#fbfbfa',
  ansi: [
    '#18181b',
    '#c81e1e',
    '#13803d',
    '#a16207',
    '#1d4ed8',
    '#9333ea',
    '#0e7490',
    '#52525b',
    '#a1a1aa',
    '#dc2626',
    '#16a34a',
    '#ca8a04',
    '#2563eb',
    '#a855f7',
    '#0891b2',
    '#3f3f46',
  ],
};

/** xterm's 256-colour table past the 16 named colours: a 6×6×6 cube, then 24 greys. */
export function indexed(i: number, pal: TermPalette): string {
  if (i < 16) return pal.ansi[i];
  if (i >= 232) {
    const v = 8 + (i - 232) * 10;
    return `rgb(${v},${v},${v})`;
  }
  const n = i - 16;
  const step = (c: number) => (c === 0 ? 0 : 55 + c * 40);
  return `rgb(${step(Math.floor(n / 36))},${step(Math.floor(n / 6) % 6)},${step(n % 6)})`;
}

export function cssColor(c: Color, pal: TermPalette): string | null {
  if (c === null) return null;
  if (typeof c === 'number') return indexed(c, pal);
  return /^#[0-9a-f]{6}$/i.test(c) ? c : null;
}

/** Paint `frame` into `el`, replacing what was there. */
export function paint(el: HTMLElement, frame: Frame, pal: TermPalette): void {
  const doc = el.ownerDocument;
  const out = doc.createDocumentFragment();
  for (const line of frame.lines) {
    const ln = doc.createElement('div');
    ln.className = 'ln';
    for (const [text, fg, bg, mods, width] of line) {
      const s = doc.createElement('span');
      s.textContent = text;
      let f = cssColor(fg, pal);
      let b = cssColor(bg, pal);
      if (mods & MOD.REVERSED) {
        const ff = f ?? pal.fg;
        f = b ?? pal.bg;
        b = ff;
      }
      if (mods & MOD.HIDDEN) f = b ?? pal.bg;
      if (f) s.style.color = f;
      if (b) s.style.backgroundColor = b;
      const cls: string[] = [];
      if (mods & MOD.BOLD) cls.push('b');
      if (mods & MOD.DIM) cls.push('d');
      if (mods & MOD.ITALIC) cls.push('i');
      if (mods & MOD.UNDERLINED) cls.push('u');
      if (mods & MOD.CROSSED_OUT) cls.push('x');
      // One non-ASCII glyph: a fixed box of its cell width.
      if (!/^[\x20-\x7e]*$/.test(text)) {
        cls.push('g');
        s.style.width = `${width}ch`;
      }
      if (cls.length) s.className = cls.join(' ');
      ln.append(s);
    }
    out.append(ln);
  }
  el.replaceChildren(out);
}

/** Plain text of a frame (for tests and for the copy button). */
export function frameText(frame: Frame): string {
  return frame.lines
    .map((l) =>
      l
        .map((r) => r[0])
        .join('')
        .replace(/\s+$/, ''),
    )
    .join('\n');
}

// ── the wasm module ──

interface Exports {
  memory: WebAssembly.Memory;
  tui_alloc(len: number): number;
  tui_free(ptr: number, len: number): void;
  tui_out_ptr(): number;
  tui_out_len(): number;
  tui_catalog(): void;
  tui_panic(): void;
  tui_open(ptr: number, len: number): number;
  tui_close(handle: number): void;
  tui_render(handle: number, cols: number, rows: number, tMs: number, palette: number, reduced: number): number;
  tui_key(handle: number, code: number, ch: number, mods: number): number;
}

/** `palette` for `tui_render`: 0 ansi, 1 dark true colour, 2 light true colour, 3 mono. */
export type ThemeCode = 0 | 1 | 2 | 3;

export class Catalog {
  private constructor(private x: Exports) {}

  static async load(bytes: BufferSource | Response | Promise<Response>): Promise<Catalog> {
    const src = await bytes;
    const { instance } =
      src instanceof Response
        ? await WebAssembly.instantiate(await src.arrayBuffer(), {})
        : await WebAssembly.instantiate(src, {});
    return new Catalog(instance.exports as unknown as Exports);
  }

  private out(): string {
    const p = this.x.tui_out_ptr();
    const n = this.x.tui_out_len();
    return new TextDecoder().decode(new Uint8Array(this.x.memory.buffer, p, n));
  }

  json<T>(): T {
    this.x.tui_catalog();
    return JSON.parse(this.out()) as T;
  }

  /** A demo handle for `slug/demo`, or 0 when it does not exist. */
  open(path: string): number {
    const bytes = new TextEncoder().encode(path);
    const p = this.x.tui_alloc(bytes.length);
    new Uint8Array(this.x.memory.buffer, p, bytes.length).set(bytes);
    const h = this.x.tui_open(p, bytes.length);
    this.x.tui_free(p, bytes.length);
    return h;
  }

  close(h: number): void {
    this.x.tui_close(h);
  }

  /** A frame, or null for an unknown handle. Throws with the Rust panic message if the demo panicked. */
  render(h: number, cols: number, rows: number, tMs: number, theme: ThemeCode, reduced: boolean): Frame | null {
    let ok: number;
    try {
      ok = this.x.tui_render(h, cols, rows, tMs, theme, reduced ? 1 : 0);
    } catch (e) {
      throw new Error(this.panicMessage() || String(e));
    }
    if (!ok) return null;
    return JSON.parse(this.out()) as Frame;
  }

  /** The last Rust panic message, or '' if none. */
  panicMessage(): string {
    try {
      this.x.tui_panic();
      return this.out();
    } catch {
      return '';
    }
  }

  /** Returns whether the demo changed. */
  key(h: number, e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'altKey' | 'shiftKey'>): boolean {
    const k = keyCode(e.key, e.shiftKey);
    if (!k) return false;
    const mods = (e.ctrlKey ? 1 : 0) | (e.altKey ? 2 : 0) | (e.shiftKey ? 4 : 0);
    return this.x.tui_key(h, k[0], k[1], mods) === 1;
  }
}

/** A browser key name → the wasm side's `(code, char)`. `null` for keys a demo never takes. */
export function keyCode(key: string, shift = false): [number, number] | null {
  const named: Record<string, number> = {
    Enter: 1,
    Escape: 2,
    Backspace: 3,
    Delete: 4,
    Tab: shift ? 6 : 5,
    ArrowLeft: 7,
    ArrowRight: 8,
    ArrowUp: 9,
    ArrowDown: 10,
    Home: 11,
    End: 12,
    PageUp: 13,
    PageDown: 14,
  };
  if (key in named) return [named[key], 0];
  // One printable character (a grapheme the IME committed counts too).
  const cps = Array.from(key);
  if (cps.length === 1) return [0, cps[0].codePointAt(0)!];
  return null;
}
