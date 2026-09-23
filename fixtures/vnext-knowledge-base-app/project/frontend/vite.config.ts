import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import { createDevBackendMiddleware } from './dev-backend.mjs';

export default defineConfig({
  plugins: [
    react(),
    {
      name: 'lenso-dev-backend',
      configureServer(server) {
        server.middlewares.use(createDevBackendMiddleware(process.env.LENSO_API_URL_FILE));
      },
    },
  ],
  build: {
    emptyOutDir: true,
    outDir: '../app/notes-web/public',
    rollupOptions: {
      output: {
        assetFileNames: 'assets/[name][extname]',
        chunkFileNames: 'assets/[name].js',
        entryFileNames: 'assets/app.js',
      },
    },
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
  },
});
