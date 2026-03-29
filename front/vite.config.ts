import path from 'path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig({
  base: '/ai/manager/ui',
  plugins: [vue(), tailwindcss()],
  server: {
    port: 29528, // Matches front/vite.config.ts port
    allowedHosts: true,
    proxy: {
      '/ai/manager/api': {
        target: 'http://localhost:29527',
        changeOrigin: true,
      },
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
})
