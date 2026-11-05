import { resolve } from 'node:path';
import { defineConfig } from 'vite';

/**
 * The catalog lives at https://2lab.ai/tui: built for the base `/tui/` into `site-dist/tui/`, so the tui Vercel project
 * serves the same paths the 2lab.ai site proxies (`/tui/:path*`). `SITE_BASE=/` builds it for a root.
 */
const base = process.env.SITE_BASE ?? '/tui/';
const dir = base.replace(/^\/+|\/+$/g, '');

export default defineConfig({
  base,
  root: resolve(__dirname, 'site'),
  publicDir: resolve(__dirname, 'site/public'),
  build: { outDir: resolve(__dirname, 'site-dist', dir), emptyOutDir: true, target: 'es2022' },
  server: { fs: { allow: [resolve(__dirname)] } },
});
