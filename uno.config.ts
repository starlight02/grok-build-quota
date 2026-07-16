import {
  defineConfig,
  presetAttributify,
  presetUno,
  transformerDirectives,
  transformerVariantGroup,
} from 'unocss'

export default defineConfig({
  cli: {
    entry: {
      patterns: ['src/**/*.rs'],
      outFile: 'style/uno.css',
    },
  },
  presets: [
    presetUno(),
    presetAttributify(),
  ],
  preflights: [
    {
      getCSS: () => `
        *, ::before, ::after { box-sizing: border-box; }
        html { -webkit-text-size-adjust: 100%; }
        html, body { margin: 0; padding: 0; }
        body { min-height: 100vh; overflow-x: hidden; -webkit-font-smoothing: antialiased; }

        /* terminal-style square-dot chase spinner */
        .gbq-dots {
          display: inline-grid;
          grid-template-columns: repeat(2, 3.5px);
          grid-auto-rows: 3.5px;
          gap: 1.6px;
          line-height: 0;
        }
        .gbq-dots i {
          display: block;
          width: 3.5px;
          height: 3.5px;
          border-radius: 1px;
          background: currentColor;
          opacity: 0.16;
          transform: scale(0.85);
          animation: gbq-dot-pulse 0.96s linear infinite;
        }
        @keyframes gbq-dot-pulse {
          0%        { opacity: 1;    transform: scale(1); }
          25%       { opacity: 1;    transform: scale(1); }
          55%, 100% { opacity: 0.16; transform: scale(0.85); }
        }
        @media (prefers-reduced-motion: reduce) {
          .gbq-dots i { animation-duration: 1.92s; }
        }
      `,
    },
  ],
  transformers: [
    transformerDirectives(),
    transformerVariantGroup(),
  ],
  theme: {
    fontFamily: {
      sans: '"SF Pro Text", "SF Pro Display", -apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif',
      mono: '"SF Mono", ui-monospace, Menlo, Consolas, monospace',
    },
  },
})
