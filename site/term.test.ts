import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { Catalog, cssColor, DARK, type Frame, frameText, indexed, keyCode, LIGHT, MOD, paint } from './term';

const frame = (lines: Frame['lines'], cols = 10): Frame => ({ cols, rows: lines.length, lines });

describe('paint', () => {
  it('puts widget text in as text, never as markup', () => {
    const el = document.createElement('pre');
    paint(el, frame([[['<img src=x onerror=alert(1)>', null, null, 0, 26]]], 26), DARK);
    expect(el.querySelector('img')).toBeNull();
    expect(el.textContent).toBe('<img src=x onerror=alert(1)>');
  });

  it('gives a non-ASCII glyph a fixed box of its cell width', () => {
    const el = document.createElement('pre');
    paint(
      el,
      frame([
        [
          ['ab', 2, null, 0, 2],
          ['한', null, null, 0, 2],
          ['█', 1, null, 0, 1],
        ],
      ]),
      DARK,
    );
    const spans = el.querySelectorAll('span');
    expect(spans[0].className).toBe('');
    expect(spans[1].className).toContain('g');
    expect(spans[1].style.width).toBe('2ch');
    expect(spans[2].style.width).toBe('1ch');
  });

  it('maps named colours through the terminal palette', () => {
    const el = document.createElement('pre');
    paint(el, frame([[['ok', 2, null, 0, 2]]]), LIGHT);
    expect(el.querySelector('span')!.style.color).toBe(LIGHT.ansi[2]);
  });

  it('swaps colours for reversed cells, defaulting to the terminal pair', () => {
    const el = document.createElement('pre');
    paint(el, frame([[['sel', null, null, MOD.REVERSED, 3]]]), DARK);
    const s = el.querySelector('span')!;
    expect(s.style.color).toBe(DARK.bg);
    expect(s.style.backgroundColor).toBe(DARK.fg);
  });

  it('marks bold, dim, italic, underline and crossed out', () => {
    const el = document.createElement('pre');
    const all = MOD.BOLD | MOD.DIM | MOD.ITALIC | MOD.UNDERLINED | MOD.CROSSED_OUT;
    paint(el, frame([[['x', null, null, all, 1]]]), DARK);
    expect(el.querySelector('span')!.className.split(' ').sort()).toEqual(['b', 'd', 'i', 'u', 'x']);
  });
});

describe('colours', () => {
  it('resolves the xterm cube and greys', () => {
    expect(indexed(16, DARK)).toBe('rgb(0,0,0)');
    expect(indexed(231, DARK)).toBe('rgb(255,255,255)');
    expect(indexed(232, DARK)).toBe('rgb(8,8,8)');
  });

  it('rejects anything that is not a colour', () => {
    expect(cssColor('#12ab9f', DARK)).toBe('#12ab9f');
    expect(cssColor('red;background:url(x)', DARK)).toBeNull();
    expect(cssColor(null, DARK)).toBeNull();
  });
});

describe('keys', () => {
  it('maps browser keys to the wasm codes', () => {
    expect(keyCode('ArrowDown')).toEqual([10, 0]);
    expect(keyCode('Tab', true)).toEqual([6, 0]);
    expect(keyCode('a')).toEqual([0, 97]);
    expect(keyCode('한')).toEqual([0, '한'.codePointAt(0)]);
    expect(keyCode('Shift')).toBeNull();
    expect(keyCode('F5')).toBeNull();
  });
});

/** The real module: every demo of every widget renders, at its size and narrow, in every theme. */
const wasm = resolve(__dirname, 'catalog.wasm');
describe.skipIf(!existsSync(wasm))('catalog.wasm', () => {
  it('renders every demo the catalog lists', async () => {
    const cat = await Catalog.load(readFileSync(wasm));
    const doc = cat.json<{ components: { slug: string; demos: { id: string; cols: number; rows: number }[] }[] }>();
    expect(doc.components.length).toBeGreaterThan(0);
    let n = 0;
    for (const c of doc.components) {
      for (const d of c.demos) {
        const h = cat.open(`${c.slug}/${d.id}`);
        expect(h, `${c.slug}/${d.id}`).toBeGreaterThan(0);
        for (const theme of [0, 1, 2, 3] as const) {
          for (const cols of [d.cols, 40]) {
            const f = cat.render(h, cols, d.rows, 1500, theme, false);
            expect(f, `${c.slug}/${d.id}`).not.toBeNull();
            expect(f!.lines.length).toBe(d.rows);
            n++;
          }
        }
        expect(frameText(cat.render(h, d.cols, d.rows, 0, 0, true)!).trim().length).toBeGreaterThan(0);
        cat.close(h);
      }
    }
    expect(n).toBeGreaterThan(0);
    expect(cat.open('nope/nope')).toBe(0);
  });
});
