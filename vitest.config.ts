import { defineConfig } from 'vitest/config';

export default defineConfig({ test: { include: ['site/**/*.test.ts'], environment: 'happy-dom' } });
