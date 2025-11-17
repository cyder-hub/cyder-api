import { fileURLToPath, URL } from "node:url";

import { defineConfig } from 'vite';
import { tanstackRouter } from '@tanstack/router-plugin/vite';
import solidPlugin from 'vite-plugin-solid';
import tailwindcss from '@tailwindcss/vite';
import devtools from 'solid-devtools/vite';

export default defineConfig({
  plugins: [
    devtools({
      autoname: true, // e.g. signal name will be based on variable name
    }),
    tanstackRouter({
        target: 'solid',
        autoCodeSplitting: true,
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
  resolve: {
			alias: {
				'@': fileURLToPath(new URL('./src', import.meta.url)),
			},
		},
});
