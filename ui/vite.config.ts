import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 4777,
    strictPort: true,
  },
  test: {
    globals: true,
    environment: 'jsdom',
    environmentOptions: { jsdom: { url: 'http://localhost:4777/' } },
    setupFiles: ['./src/test/setup.ts'],
  },
});
