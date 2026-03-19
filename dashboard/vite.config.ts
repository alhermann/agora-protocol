import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// AGORA_API_PORT env var configures which daemon to proxy to (default: 7313)
const apiPort = process.env.AGORA_API_PORT || '7313'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: `http://127.0.0.1:${apiPort}`,
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, ''),
      },
    },
  },
})
