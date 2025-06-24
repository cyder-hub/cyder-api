import { defineConfig } from 'vite';
import solidPlugin from 'vite-plugin-solid';
import tailwindcss from '@tailwindcss/vite';
import devtools from 'solid-devtools/vite';

export default defineConfig({
  plugins: [
    devtools({
      autoname: true, // e.g. signal name will be based on variable name
    }),
    solidPlugin(),
    tailwindcss(),
  ],
  base: "/ai/manager/ui",
  server: {
    port: 29528,
    allowedHosts: true,
    proxy: {
      '/ai/manager/api': {
        target: 'http://localhost:29527',
        changeOrigin: true,
      },
    },
  },
  build: {
    target: 'esnext',
  },
});
