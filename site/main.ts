import { marked } from 'marked';
import research from '../docs/research.md?raw';
import notice from '../NOTICE?raw';
import wasmUrl from './catalog.wasm?url';
import { Catalog, DARK, type Frame, LIGHT, paint, type TermPalette, type ThemeCode } from './term';
import './style.css';

interface PropDoc {
  name: string;
  type: string;
  default: string | null;
  description: string;
}
interface DemoDoc {
  id: string;
  title: string;
  description: string;
  cols: number;
  rows: number;
  animated: boolean;
  interactive: boolean;
}
interface CompDoc {
  slug: string;
  name: string;
  type: string;
  summary: string;
  description: string;
  usedIn: string;
  group: 'Agent' | 'Data' | 'Input' | 'Primitives';
  usage: string;
  source: { project: string; url: string; license: string; how: string };
  props: PropDoc[];
  keyboard: [string, string][];
  a11y: string[];
  demos: DemoDoc[];
}
interface CatalogDoc {
  version: string;
  components: CompDoc[];
}

const app = document.getElementById('app')!;
const esc = (s: unknown) =>
  String(s ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');

/** The site lives under Vite's `base` (`/tui/`). Every internal link goes through `at()`. */
const BASE = import.meta.env.BASE_URL.endsWith('/') ? import.meta.env.BASE_URL : `${import.meta.env.BASE_URL}/`;
const at = (path: string): string => `${BASE}${path.replace(/^\/+/, '')}`;
function sitePath(pathname: string): string {
  const root = BASE.replace(/\/+$/, '');
  const rest = root && (pathname === root || pathname.startsWith(`${root}/`)) ? pathname.slice(root.length) : pathname;
  return rest.replace(/\/+$/, '') || '/';
}

const GROUPS: CompDoc['group'][] = ['Agent', 'Data', 'Input', 'Primitives'];
const GROUP_NOTE: Record<CompDoc['group'], string> = {
  Agent: 'what an agent or a request is doing now',
  Data: 'quotas, charts, tables, records',
  Input: 'what the person types, picks or presses',
  Primitives: 'the pieces the others share',
};
const PROJECT: Record<string, string> = { llmux: 'llmux', codex: 'OpenAI Codex', fx: 'Vercel fx', '2lab': '2lab' };

// ── view state (kept across pages) ──

type TermBg = 'dark' | 'light';
type Colour = 'ansi' | 'true' | 'mono';
const view = {
  bg: (localStorage.getItem('2lab-tui-term') as TermBg) || 'dark',
  colour: (localStorage.getItem('2lab-tui-colour') as Colour) || 'ansi',
  reduced: matchMedia('(prefers-reduced-motion: reduce)').matches,
  width: 'fit' as 'narrow' | 'fit' | 'wide',
};
const palette = (): TermPalette => (view.bg === 'dark' ? DARK : LIGHT);
function themeCode(): ThemeCode {
  if (view.colour === 'mono') return 3;
  if (view.colour === 'true') return view.bg === 'dark' ? 1 : 2;
  return 0;
}

function siteTheme(): 'light' | 'dark' {
  return document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light';
}

// ── terminals on the page ──

interface Term {
  el: HTMLElement;
  handle: number;
  cols: number;
  rows: number;
  start: number;
  animated: boolean;
  /** ms between frames: the stage runs smooth, tiles run calm. */
  every: number;
  last: number;
  visible: boolean;
}

/** Widest a home-page tile renders a demo, in cells. */
const THUMB_COLS = 56;

let cat: Catalog;
let doc: CatalogDoc;
let terms: Term[] = [];
let tileObserver: IntersectionObserver | null = null;

function drop(): void {
  for (const t of terms) cat.close(t.handle);
  terms = [];
  tileObserver?.disconnect();
  tileObserver = null;
}

function draw(t: Term, now: number): void {
  let frame: Frame | null;
  try {
    frame = cat.render(t.handle, t.cols, t.rows, now - t.start, themeCode(), view.reduced);
  } catch (e) {
    // A panicking demo stops animating and shows why, instead of taking the page down.
    t.animated = false;
    t.el.textContent = `This demo stopped: ${(e as Error).message}`;
    console.error(e);
    return;
  }
  if (!frame) return;
  paint(t.el, frame, palette());
  t.last = now;
}

function mount(
  el: HTMLElement,
  path: string,
  cols: number,
  rows: number,
  animated: boolean,
  every: number,
): Term | null {
  const handle = cat.open(path);
  if (!handle) {
    el.textContent = `unknown demo ${path}`;
    return null;
  }
  const t: Term = { el, handle, cols, rows, start: performance.now(), animated, every, last: 0, visible: true };
  el.style.setProperty('--cols', String(cols));
  el.style.setProperty('--rows', String(rows));
  applyTermColours(el);
  terms.push(t);
  draw(t, performance.now());
  return t;
}

function applyTermColours(el: HTMLElement): void {
  const p = palette();
  el.style.setProperty('--t-fg', p.fg);
  el.style.setProperty('--t-bg', p.bg);
}

function redrawAll(): void {
  const now = performance.now();
  for (const t of terms) {
    applyTermColours(t.el);
    draw(t, now);
  }
}

function loop(now: number): void {
  for (const t of terms) if (t.animated && t.visible && now - t.last >= t.every) draw(t, now);
  requestAnimationFrame(loop);
}

// ── pages ──

function shell(active: string, body: string): string {
  const link = (href: string, label: string, extra = '') =>
    `<a href="${at(href)}" data-link${active === href ? ' aria-current="page"' : ''}>${label}${extra}</a>`;
  const groups = GROUPS.map((g) => {
    const items = doc.components.filter((c) => c.group === g);
    if (!items.length) return '';
    return `<h4>${g}</h4>${items.map((c) => link(`/components/${c.slug}`, esc(c.name), `<small>${esc(c.source.project)}</small>`)).join('')}`;
  }).join('');
  return `<header class="top">
  <button class="icon-btn menu-btn" id="menu" aria-label="Menu">☰</button>
  <a class="brand" href="${at('/')}" data-link><img src="${at('favicon.svg')}" alt="" />2lab<span>/tui</span></a>
  <nav><a href="${at('components')}" data-link${active.startsWith('/components') ? ' aria-current="page"' : ''}>Components</a><a href="${at('docs/installation')}" data-link${active.startsWith('/docs/') ? ' aria-current="page"' : ''}>Docs</a><a href="https://2lab.ai/ui">ui ↗</a></nav>
  <div class="grow"></div>
  <span class="ver">v${esc(doc.version)}</span>
  <a class="icon-btn" href="https://github.com/2lab-ai/tui" aria-label="Source on GitHub">GH</a>
  <button class="icon-btn" id="theme" aria-label="Toggle site theme">${siteTheme() === 'dark' ? '☀' : '☾'}</button>
</header>
<div class="layout">
  <aside class="side" aria-label="Sections">
    <h4>Getting started</h4>
    ${link('/', 'Introduction')}${link('/docs/installation', 'Installation')}${link('/docs/theming', 'Theming')}
    <h4>Background</h4>
    ${link('/docs/research', 'Research', '<small>report</small>')}${link('/docs/licences', 'Sources and licences')}
    ${groups}
  </aside>
  <main id="main" tabindex="-1"><div class="page">${body}
    <div class="foot">twolab-tui v${esc(doc.version)} · ratatui 0.30 widgets · every demo on this site is the Rust crate itself, compiled to WebAssembly and drawn into a cell grid · widgets from <a href="https://github.com/2lab-ai/llmux">llmux</a>, <a href="https://github.com/openai/codex">OpenAI Codex</a> (Apache-2.0) and <a href="https://github.com/vercel-labs/fx">Vercel fx</a> (Apache-2.0) — see <a href="${at('docs/licences')}" data-link>sources and licences</a></div>
  </div></main>
</div>`;
}

function codeBlock(code: string, id: string): string {
  return `<div class="code-wrap"><pre class="code" id="${id}">${esc(code)}</pre><button class="copy" data-copy="${id}">Copy</button></div>`;
}

function table(head: string[], rows: string[][]): string {
  if (!rows.length) return '<p class="desc">None.</p>';
  return `<div class="tbl"><table><thead><tr>${head.map((h) => `<th>${h}</th>`).join('')}</tr></thead><tbody>${rows
    .map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join('')}</tr>`)
    .join('')}</tbody></table></div>`;
}

function cards(list: CompDoc[]): string {
  return list
    .map(
      (c) => `<a class="card" href="${at(`components/${c.slug}`)}" data-link>
      <div class="thumb term-thumb" data-thumb="${esc(c.slug)}"><pre class="term" aria-hidden="true"></pre></div>
      <div class="meta"><em>${esc(c.group)} · ${esc(PROJECT[c.source.project] ?? c.source.project)}</em><b>${esc(c.name)}</b><span>${esc(c.summary)}</span></div></a>`,
    )
    .join('');
}

function homePage(): string {
  const n = doc.components.length;
  const by = (p: string) => doc.components.filter((c) => c.source.project === p).length;
  return `<section class="hero">
    <div class="crumb">twolab-tui</div>
    <h1>Terminal widgets that show what the agent is doing — in the cells you have.</h1>
    <p class="lede">${n} ratatui widgets for agent and dashboard TUIs: quotas, live requests, key hints, pickers, pagers. ${by('llmux')} come from llmux, ${by('codex')} from OpenAI Codex, ${by('fx')} from Vercel fx. Data in, a buffer out. 16-colour, true colour or no colour. Reduced motion. CJK-safe widths.</p>
    <div class="cta"><a class="btn primary" href="${at('components')}" data-link>Browse widgets</a><a class="btn" href="${at('docs/installation')}" data-link>Install</a><a class="btn" href="${at('docs/research')}" data-link>Read the research</a></div>
  </section>
  ${GROUPS.map((g) => {
    const list = doc.components.filter((c) => c.group === g);
    return list.length
      ? `<h2 id="${g.toLowerCase()}">${g} <span class="chip">${list.length}</span></h2><p class="desc">${esc(GROUP_NOTE[g])}</p><div class="grid">${cards(list)}</div>`
      : '';
  }).join('')}`;
}

function componentsPage(): string {
  return homePage().replace(
    /<section class="hero">[\s\S]*?<\/section>/,
    '<div class="crumb">Components</div><h1>Widgets</h1><p class="lede">Every widget is a ratatui <code>Widget</code> (or returns <code>Line</code>s you place yourself). Pass data and a <code>Theme</code>; get cells.</p>',
  );
}

function componentPage(c: CompDoc): string {
  const demos = c.demos
    .map((m, i) => `<button role="tab" data-demo="${esc(m.id)}" aria-selected="${i === 0}">${esc(m.title)}</button>`)
    .join('');
  const seg = (name: string, key: string, opts: [string, string][], cur: string) =>
    `<div class="seg" aria-label="${name}">${opts.map(([v, l]) => `<button data-${key}="${v}" aria-pressed="${cur === v}">${l}</button>`).join('')}</div>`;
  const props = table(
    ['Builder', 'Type', 'Default', 'What it sets'],
    c.props.map((p) => [
      `<code>${esc(p.name)}</code>`,
      `<code class="type">${esc(p.type)}</code>`,
      p.default ? `<code>${esc(p.default)}</code>` : '—',
      esc(p.description),
    ]),
  );
  const keys = table(
    ['Keys', 'Action'],
    c.keyboard.map(([k, a]) => [
      k
        .split(/\s*\/\s*/)
        .map((x) => `<kbd>${esc(x)}</kbd>`)
        .join(' '),
      esc(a),
    ]),
  );
  const how = c.source.how === 'ported' ? 'Ported from' : c.source.how === 'studied' ? 'Re-written after' : 'New in';
  return `<div class="crumb"><a href="${at('components')}" data-link>${esc(c.group)}</a> / ${esc(c.name)}</div>
  <h1>${esc(c.name)}</h1>
  <p class="lede">${esc(c.summary)}</p>
  <div class="chips"><span class="chip"><code>${esc(c.type)}</code></span><span class="chip">ratatui 0.30</span><span class="chip">16 · true colour · no colour</span>
    <a class="chip link" href="${esc(c.source.url)}" target="_blank" rel="noopener">${how} ${esc(PROJECT[c.source.project] ?? c.source.project)} · ${esc(c.source.license)} ↗</a></div>
  <section class="preview" aria-label="Live preview">
    <div class="pv-bar">
      <div class="seg pv-tabs" role="tablist" aria-label="Examples">${demos}</div>
      <div class="pv-tools">
        ${seg(
          'Width',
          'width',
          [
            ['narrow', '40 cols'],
            ['fit', 'Fit'],
            ['wide', '120 cols'],
          ],
          view.width,
        )}
        ${seg(
          'Terminal',
          'bg',
          [
            ['dark', 'Dark'],
            ['light', 'Light'],
          ],
          view.bg,
        )}
        ${seg(
          'Colour',
          'colour',
          [
            ['ansi', '16'],
            ['true', 'True colour'],
            ['mono', 'No colour'],
          ],
          view.colour,
        )}
        <button class="icon-btn${view.reduced ? ' on' : ''}" id="motion" aria-pressed="${view.reduced}" aria-label="Reduced motion" title="Reduced motion">◐</button>
        <button class="icon-btn" id="replay" aria-label="Replay example" title="Replay">↺</button>
      </div>
    </div>
    <div class="pv-stage term-stage" id="stage" data-bg="${view.bg}">
      <div class="term-win"><div class="term-title"><i></i><i></i><i></i><span id="term-label"></span></div>
      <pre class="term" id="term" tabindex="0" aria-label="Live terminal preview"></pre></div>
    </div>
    <div class="pv-note" id="note"></div>
  </section>
  <p class="desc" style="margin-top:22px">${esc(c.description)}</p>
  <h2>Usage</h2>
  ${codeBlock(`# Cargo.toml\ntwolab-tui = { git = "https://github.com/2lab-ai/tui" }`, 'inst')}
  <div style="height:12px"></div>
  ${codeBlock(c.usage, 'usage')}
  <h2>Builder</h2>${props}
  <h2>Keyboard</h2>${keys}
  <h2>Accessibility</h2><ul class="plain">${c.a11y.map((a) => `<li>${esc(a)}</li>`).join('')}</ul>
  <h2>In 2lab apps</h2><p class="desc">${esc(c.usedIn)}</p>
  <h2>Source</h2><p class="desc">${how} <a href="${esc(c.source.url)}" target="_blank" rel="noopener">${esc(PROJECT[c.source.project] ?? c.source.project)}</a> under ${esc(c.source.license)}. ${c.source.license === 'Apache-2.0' ? 'The file keeps its Apache-2.0 notice and is listed in NOTICE.' : ''}</p>`;
}

function installPage(): string {
  return `<div class="crumb">Getting started</div><h1>Installation</h1>
  <p class="lede">A git dependency on <a href="https://github.com/2lab-ai/tui">2lab-ai/tui</a>. The crate needs only ratatui 0.30 — no terminal backend — so it builds for any target, the browser included.</p>
  <h2>Cargo</h2>
  ${codeBlock(`[dependencies]\ntwolab-tui = { git = "https://github.com/2lab-ai/tui" }\n# KeyInput::from_crossterm for crossterm apps:\n# twolab-tui = { git = "https://github.com/2lab-ai/tui", features = ["crossterm"] }`, 'cargo')}
  <h2>A frame</h2>
  ${codeBlock(`use twolab_tui::{Theme, spinner::{Spinner, SpinnerKind}};\n\nlet theme = Theme::from_env(); // NO_COLOR → mono\nterminal.draw(|f| {\n    let span = Spinner::new(SpinnerKind::Braille).span_at(started.elapsed(), &theme);\n    f.render_widget(ratatui::text::Line::from(vec![span, " working".into()]), area);\n})?;`, 'frame')}
  <h2>Rules every widget keeps</h2>
  <ul class="plain"><li>Data in, cells out. A widget never reads a clock or the terminal: time comes in as elapsed time or a shared tick, size as the area.</li><li>Colour comes from a <code>Theme</code>. With no colour (<code>NO_COLOR</code>) every cell keeps the terminal default and meaning moves to weight and glyphs — the catalog's tests check every demo.</li><li>Width is counted in terminal cells by grapheme, the unit ratatui renders, so Korean, Japanese and emoji line up.</li><li>Animated widgets hold one still frame under reduced motion.</li></ul>
  <h2>Features</h2>
  ${table(
    ['Feature', 'What it adds'],
    [
      ['<code>crossterm</code>', 'Converts crossterm key events into the widgets’ <code>KeyInput</code>.'],
      ['<code>catalog</code>', 'The docs and demos this site renders. Apps leave it off.'],
    ],
  )}`;
}

function themingPage(): string {
  const tokens: [string, string][] = [
    ['text', 'Body text'],
    ['text_secondary', 'Labels, secondary facts'],
    ['muted', 'Hints, separators, empty parts of a bar'],
    ['border', 'Frame lines'],
    ['accent', 'The one thing to look at'],
    ['run · wait · done · fail', 'The four task states, as in @2lab.ai/ui'],
    ['ok · warn · crit', 'How far into the red a value is'],
  ];
  return `<div class="crumb">Getting started</div><h1>Theming</h1>
  <p class="lede">One <code>Theme</code> carries the colours and the motion setting. The token names match <a href="https://2lab.ai/ui/docs/theming">@2lab.ai/ui</a>, so a dashboard and its terminal sibling say the same thing in the same colour.</p>
  <h2>Four palettes</h2>
  ${table(
    ['Constructor', 'When'],
    [
      [
        '<code>Theme::ansi()</code>',
        'Default. The 16 named colours, so the user’s terminal palette decides the hues and light or dark both work.',
      ],
      ['<code>Theme::dark()</code> / <code>Theme::light()</code>', 'True colour, when you know the background.'],
      [
        '<code>Theme::mono()</code>',
        'No colour at all — what <code>NO_COLOR</code> asks for. Meaning moves to bold, dim and reversed.',
      ],
      [
        '<code>Theme::from_env()</code>',
        '<code>mono</code> when <code>NO_COLOR</code> is set, else <code>ansi</code>.',
      ],
    ],
  )}
  <h2>Tokens</h2>
  ${table(
    ['Token', 'Means'],
    tokens.map(([k, v]) => [`<code>${esc(k)}</code>`, esc(v)]),
  )}
  <h2>Motion</h2>
  ${codeBlock(`let theme = Theme::from_env().with_motion(Motion::Reduced);`, 'motion')}
  <p class="desc">Under <code>Motion::Reduced</code> a spinner holds its first glyph and a shimmer draws plain bold text. Clocks still count — reduced motion stops movement, not information.</p>`;
}

function researchPage(): string {
  return `<div class="crumb">Background</div><article class="md">${marked.parse(research, { async: false })}</article>`;
}

function licencesPage(): string {
  const rows = doc.components.map((c) => [
    `<a href="${at(`components/${c.slug}`)}" data-link>${esc(c.name)}</a>`,
    esc(PROJECT[c.source.project] ?? c.source.project),
    esc(c.source.how),
    esc(c.source.license),
    `<a href="${esc(c.source.url)}" target="_blank" rel="noopener">source ↗</a>`,
  ]);
  return `<div class="crumb">Background</div><h1>Sources and licences</h1>
  <p class="lede">The crate is Apache-2.0. Widgets carried over from OpenAI Codex and Vercel fx keep their Apache-2.0 notices; widgets from llmux (MIT, 2lab) are relicensed by their owner.</p>
  ${table(['Widget', 'From', 'How', 'Licence', 'Upstream'], rows)}
  <h2>NOTICE</h2><pre class="code">${esc(notice)}</pre>`;
}

let current: { comp: CompDoc; demo: DemoDoc } | null = null;

function demoCols(d: DemoDoc): number {
  return view.width === 'narrow' ? 40 : view.width === 'wide' ? 120 : d.cols;
}

function mountStage(): void {
  if (!current) return;
  for (const t of terms) cat.close(t.handle);
  terms = [];
  const el = document.getElementById('term');
  const label = document.getElementById('term-label');
  const note = document.getElementById('note');
  if (!el) return;
  const d = current.demo;
  const cols = demoCols(d);
  mount(el, `${current.comp.slug}/${d.id}`, cols, d.rows, d.animated, 33);
  if (label) label.textContent = `${current.comp.slug} — ${cols}×${d.rows}`;
  if (note) {
    const keys = d.interactive ? ' Click the terminal, then use the keys listed below.' : '';
    note.textContent = `${d.description}${keys}`;
    note.hidden = !note.textContent;
  }
  document.getElementById('stage')?.setAttribute('data-bg', view.bg);
}

function mountThumbs(): void {
  tileObserver =
    typeof IntersectionObserver === 'function'
      ? new IntersectionObserver((entries) => {
          for (const e of entries) {
            const t = terms.find((x) => x.el === e.target);
            if (t) t.visible = e.isIntersecting;
          }
        })
      : null;
  for (const box of Array.from(document.querySelectorAll<HTMLElement>('[data-thumb]'))) {
    const c = doc.components.find((x) => x.slug === box.dataset.thumb);
    const pre = box.querySelector<HTMLElement>('pre.term');
    if (!c || !pre || !c.demos[0]) continue;
    const d = c.demos[0];
    // Tiles stay legible: a wide demo renders at tile width, which also shows how the widget gives way.
    const t = mount(pre, `${c.slug}/${d.id}`, Math.min(d.cols, THUMB_COLS), d.rows, d.animated, 150);
    if (!t) continue;
    fitThumb(box, pre);
    tileObserver?.observe(pre);
  }
}

/** Scale a tile's terminal to fit its box. */
function fitThumb(box: HTMLElement, pre: HTMLElement): void {
  const s = Math.min(
    1,
    (box.clientWidth - 16) / Math.max(1, pre.offsetWidth),
    (box.clientHeight - 16) / Math.max(1, pre.offsetHeight),
  );
  pre.style.transform = `translate(-50%, -50%) scale(${s.toFixed(3)})`;
}

function route(): void {
  drop();
  const path = sitePath(location.pathname);
  current = null;
  let body: string;
  let active = path;
  const m = path.match(/^\/components\/([a-z-]+)$/);
  const comp = m ? doc.components.find((c) => c.slug === m[1]) : undefined;
  if (comp) {
    body = componentPage(comp);
    current = { comp, demo: comp.demos[0] };
    document.title = `${comp.name} — 2lab/tui`;
  } else if (path === '/components') {
    body = componentsPage();
    document.title = 'Widgets — 2lab/tui';
  } else if (path === '/docs/installation') {
    body = installPage();
    document.title = 'Installation — 2lab/tui';
  } else if (path === '/docs/theming') {
    body = themingPage();
    document.title = 'Theming — 2lab/tui';
  } else if (path === '/docs/research') {
    body = researchPage();
    document.title = 'Research — 2lab/tui';
  } else if (path === '/docs/licences') {
    body = licencesPage();
    document.title = 'Sources and licences — 2lab/tui';
  } else {
    body = homePage();
    active = '/';
    document.title = '2lab/tui — Terminal UI components';
  }
  app.innerHTML = shell(active, body);
  document.body.classList.remove('nav-open');
  if (current) mountStage();
  else mountThumbs();
}

function setPressed(attr: string, el: Element): void {
  for (const b of Array.from(document.querySelectorAll(`[${attr}]`))) b.setAttribute('aria-pressed', String(b === el));
}

document.addEventListener('click', (e) => {
  const t = e.target as Element;
  const a = t.closest('a[data-link]') as HTMLAnchorElement | null;
  if (a && !(e as MouseEvent).metaKey && !(e as MouseEvent).ctrlKey && a.origin === location.origin) {
    e.preventDefault();
    if (a.pathname !== location.pathname) {
      history.pushState(null, '', a.pathname);
      route();
      window.scrollTo(0, 0);
      (document.getElementById('main') as HTMLElement | null)?.focus({ preventScroll: true });
    }
    return;
  }
  if (t.closest('#theme')) {
    const next = siteTheme() === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', next);
    try {
      localStorage.setItem('2lab-tui-theme', next);
    } catch {}
    route();
    return;
  }
  if (t.closest('#menu')) {
    document.body.classList.toggle('nav-open');
    return;
  }
  const demoBtn = t.closest('[data-demo]') as HTMLElement | null;
  if (demoBtn && current) {
    const demo = current.comp.demos.find((x) => x.id === demoBtn.dataset.demo);
    if (demo) {
      current.demo = demo;
      for (const b of Array.from(document.querySelectorAll('[data-demo]')))
        b.setAttribute('aria-selected', String(b === demoBtn));
      mountStage();
    }
    return;
  }
  const w = t.closest('[data-width]') as HTMLElement | null;
  if (w) {
    view.width = w.dataset.width as typeof view.width;
    setPressed('data-width', w);
    mountStage();
    return;
  }
  const bg = t.closest('[data-bg]') as HTMLElement | null;
  if (bg && bg.tagName === 'BUTTON') {
    view.bg = bg.dataset.bg as TermBg;
    localStorage.setItem('2lab-tui-term', view.bg);
    setPressed('data-bg', bg);
    document.getElementById('stage')?.setAttribute('data-bg', view.bg);
    redrawAll();
    return;
  }
  const col = t.closest('[data-colour]') as HTMLElement | null;
  if (col) {
    view.colour = col.dataset.colour as Colour;
    localStorage.setItem('2lab-tui-colour', view.colour);
    setPressed('data-colour', col);
    redrawAll();
    return;
  }
  const mo = t.closest('#motion') as HTMLElement | null;
  if (mo) {
    view.reduced = !view.reduced;
    mo.setAttribute('aria-pressed', String(view.reduced));
    mo.classList.toggle('on', view.reduced);
    redrawAll();
    return;
  }
  if (t.closest('#replay')) {
    mountStage();
    return;
  }
  const cp = t.closest('[data-copy]') as HTMLElement | null;
  if (cp) {
    const pre = document.getElementById(cp.dataset.copy!);
    if (pre)
      void navigator.clipboard?.writeText(pre.textContent ?? '').then(() => {
        cp.textContent = 'Copied';
        setTimeout(() => {
          cp.textContent = 'Copy';
        }, 1200);
      });
  }
});

/** Keys go to the stage's demo while the terminal has focus. Tab leaves the terminal unless the demo takes it. */
document.addEventListener('keydown', (e) => {
  const el = document.activeElement;
  if (!current || !el || el.id !== 'term' || !current.demo.interactive) return;
  const t = terms.find((x) => x.el === el);
  if (!t) return;
  if (e.key === 'Tab' && !current.comp.keyboard.some(([k]) => /tab/i.test(k))) return;
  // Korean, Japanese and Chinese input: wait for the committed text, never send a half-composed syllable.
  if (e.metaKey || e.isComposing || e.key === 'Process') return;
  const changed = cat.key(t.handle, e);
  if (
    changed ||
    ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', ' ', 'PageUp', 'PageDown', 'Home', 'End', 'Tab'].includes(e.key)
  )
    e.preventDefault();
  if (changed) draw(t, performance.now());
});

window.addEventListener('popstate', route);

async function start(): Promise<void> {
  cat = await Catalog.load(fetch(wasmUrl));
  doc = cat.json<CatalogDoc>();
  route();
  requestAnimationFrame(loop);
}

start().catch((err) => {
  app.innerHTML = `<p style="padding:40px">The catalog could not start: ${esc(err)}</p>`;
});
