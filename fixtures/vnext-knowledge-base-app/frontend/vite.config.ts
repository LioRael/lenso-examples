import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  build: {
    emptyOutDir: true,
    outDir: '../project/app/notes-web/public',
    rollupOptions: {
      output: {
        assetFileNames: 'assets/[name][extname]',
        chunkFileNames: 'assets/[name].js',
        entryFileNames: 'assets/app.js',
      },
    },
  },
  server: {
    proxy: {
      '/notes': process.env.LENSO_API_URL ?? 'http://127.0.0.1:3001',
    },
  },
});
