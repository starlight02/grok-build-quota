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
        /* UA 默认 button 带 border/padding/字体：全部清零，样式一律走工具类 */
        button { border: 0; padding: 0; background-color: transparent; font: inherit; color: inherit; cursor: pointer; }
        button:disabled { cursor: default; }
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
        /* Neutral Liquid Glass: translucency adds hierarchy without decorative color. */
        .gbq-glass {
          position: relative;
          isolation: isolate;
          background: rgba(255, 255, 255, 0.04);
          border: 1px double rgba(51, 51, 51, 0.08);
          filter: brightness(0.98);
          -webkit-backdrop-filter: blur(6px) saturate(160%);
          backdrop-filter: blur(6px) saturate(160%);
          box-shadow: inset 2px -2px 1px -1px rgba(255, 255, 255, 0.9), inset -2px 2px 1px -1px rgba(255, 255, 255, 0.9), inset 6px -6px 1px -6px rgba(255, 255, 255, 0.55), inset -6px 6px 1px -6px rgba(255, 255, 255, 0.55), inset 0 0 2px rgba(0, 0, 0, 0.55), 0 8px 18px rgba(0, 0, 0, 0.14);
        }
        .gbq-glass::before {
          content: "";
          position: absolute;
          z-index: 1;
          top: 35%;
          left: 50%;
          width: calc(100% - 16px);
          height: calc(100% - 16px);
          transform: translateX(-50%);
          pointer-events: none;
          border: 1px solid rgba(0, 0, 0, 0.32);
          filter: blur(8px);
        }
        .gbq-glass::after {
          content: "";
          position: absolute;
          z-index: 2;
          inset: 0;
          pointer-events: none;
          border-radius: inherit;
          filter: blur(3px);
          background: linear-gradient(45deg, rgba(255, 255, 255, 0.78) 0%, transparent 25%, transparent 75%, rgba(255, 255, 255, 0.78) 100%);
        }
        /* Thick Liquid Glass panel: structural surfaces (top bar, cards). */
        .gbq-panel {
          position: relative;
          isolation: isolate;
          background: rgba(255, 255, 255, 0.55);
          -webkit-backdrop-filter: blur(30px) saturate(180%);
          backdrop-filter: blur(30px) saturate(180%);
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.98), inset 0 0 0 0.5px rgba(255, 255, 255, 0.7), inset 0 -20px 40px -32px rgba(255, 255, 255, 0.7), inset 0 0 0 1px rgba(0, 0, 0, 0.035), 0 2px 6px rgba(0, 0, 0, 0.04), 0 24px 64px -18px rgba(0, 0, 0, 0.16);
        }
        .gbq-panel::before {
          content: "";
          position: absolute;
          z-index: 0;
          inset: 0;
          pointer-events: none;
          border-radius: inherit;
          background: linear-gradient(to bottom, rgba(255, 255, 255, 0.55) 0%, rgba(255, 255, 255, 0.14) 12%, rgba(255, 255, 255, 0) 34%, rgba(255, 255, 255, 0) 82%, rgba(255, 255, 255, 0.1) 100%);
        }
        .gbq-panel > * {
          position: relative;
          z-index: 1;
        }
        /* Thin Liquid Glass chip: small floating pills on top of panels. */
        .gbq-chip {
          position: relative;
          isolation: isolate;
          background: rgba(255, 255, 255, 0.5);
          -webkit-backdrop-filter: blur(14px) saturate(180%);
          backdrop-filter: blur(14px) saturate(180%);
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.95), inset 0 0 0 0.5px rgba(255, 255, 255, 0.6), inset 0 0 0 1px rgba(0, 0, 0, 0.04), 0 3px 8px rgba(0, 0, 0, 0.06), 0 8px 20px -8px rgba(0, 0, 0, 0.12);
        }
        .gbq-chip::before {
          content: "";
          position: absolute;
          z-index: 0;
          inset: 0;
          pointer-events: none;
          border-radius: inherit;
          background: linear-gradient(to bottom, rgba(255, 255, 255, 0.55) 0%, rgba(255, 255, 255, 0) 55%);
        }
        .gbq-chip > * {
          position: relative;
          z-index: 1;
        }
        .gbq-button {
          position: relative;
          isolation: isolate;
          overflow: hidden;
          border: 0;
          color: #1d1d1f;
          background: rgba(255, 255, 255, 0.4);
          -webkit-backdrop-filter: blur(16px) saturate(185%);
          backdrop-filter: blur(16px) saturate(185%);
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.95), inset 0 0 0 0.5px rgba(255, 255, 255, 0.6), inset 0 -8px 13px -9px rgba(255, 255, 255, 0.8), inset 0 0 0 1px rgba(0, 0, 0, 0.05), 0 4px 10px rgba(0, 0, 0, 0.1), 0 11px 26px -8px rgba(0, 0, 0, 0.17);
          transition: background 240ms ease, box-shadow 240ms ease, transform 220ms cubic-bezier(0.4, 0, 0.2, 1);
        }
        .gbq-button::before {
          content: "";
          position: absolute;
          z-index: 0;
          inset: 0;
          pointer-events: none;
          border-radius: inherit;
          background: linear-gradient(to bottom, rgba(255, 255, 255, 0.62) 0%, rgba(255, 255, 255, 0.16) 30%, rgba(255, 255, 255, 0) 52%, rgba(255, 255, 255, 0.1) 100%);
        }
        .gbq-button > * {
          position: relative;
          z-index: 1;
        }
        .gbq-button:hover:not(:disabled) {
          background: rgba(255, 255, 255, 0.54);
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 1), inset 0 0 0 0.5px rgba(255, 255, 255, 0.72), inset 0 -9px 15px -9px rgba(255, 255, 255, 0.9), inset 0 0 0 1px rgba(0, 0, 0, 0.05), 0 8px 16px rgba(0, 0, 0, 0.12), 0 17px 34px -8px rgba(0, 0, 0, 0.2);
          transform: translateY(-1px);
        }
        .gbq-button:active:not(:disabled) {
          transform: translateY(0) scale(0.985);
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.85), inset 0 0 0 0.5px rgba(255, 255, 255, 0.5), inset 0 2px 6px rgba(0, 0, 0, 0.09), inset 0 0 0 1px rgba(0, 0, 0, 0.06), 0 2px 6px rgba(0, 0, 0, 0.1);
        }
        .gbq-button-primary {
          color: #2b2b2f;
          background: linear-gradient(to bottom, rgba(196, 196, 202, 0.4), rgba(150, 150, 156, 0.34));
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.85), inset 0 0 0 0.5px rgba(255, 255, 255, 0.5), inset 0 -8px 13px -9px rgba(255, 255, 255, 0.6), inset 0 0 0 1px rgba(110, 110, 120, 0.28), 0 4px 10px rgba(60, 60, 70, 0.14), 0 11px 26px -8px rgba(60, 60, 70, 0.24);
        }
        .gbq-button-primary::before {
          background: linear-gradient(to bottom, rgba(255, 255, 255, 0.58) 0%, rgba(255, 255, 255, 0.14) 32%, rgba(255, 255, 255, 0) 54%, rgba(255, 255, 255, 0.1) 100%);
        }
        .gbq-button-primary:hover:not(:disabled) {
          color: #1d1d1f;
          background: linear-gradient(to bottom, rgba(206, 206, 212, 0.48), rgba(160, 160, 166, 0.42));
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.95), inset 0 0 0 0.5px rgba(255, 255, 255, 0.6), inset 0 -9px 15px -9px rgba(255, 255, 255, 0.7), inset 0 0 0 1px rgba(110, 110, 120, 0.34), 0 8px 16px rgba(60, 60, 70, 0.2), 0 17px 34px -8px rgba(60, 60, 70, 0.3);
        }
        .gbq-button-danger:hover:not(:disabled) {
          color: #c70012;
          background: linear-gradient(to bottom, rgba(255, 122, 112, 0.3), rgba(255, 69, 58, 0.24));
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.82), inset 0 0 0 0.5px rgba(255, 255, 255, 0.5), inset 0 0 0 1px rgba(200, 40, 30, 0.32), 0 6px 14px rgba(200, 40, 30, 0.2), 0 15px 30px -8px rgba(200, 40, 30, 0.28);
        }
        .gbq-button-tab {
          color: #6e6e73;
          background: transparent;
          box-shadow: none;
          -webkit-backdrop-filter: none;
          backdrop-filter: none;
        }
        .gbq-button-tab::before {
          display: none;
        }
        .gbq-button-tab:hover:not(:disabled) {
          color: #1d1d1f;
          background: rgba(255, 255, 255, 0.4);
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.82), inset 0 0 0 0.5px rgba(255, 255, 255, 0.5), 0 2px 6px rgba(0, 0, 0, 0.06);
        }
        .gbq-button-tab-active {
          color: #1d1d1f;
          background: rgba(255, 255, 255, 0.72);
          -webkit-backdrop-filter: blur(12px) saturate(180%);
          backdrop-filter: blur(12px) saturate(180%);
          box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.98), inset 0 0 0 0.5px rgba(255, 255, 255, 0.62), 0 1px 1px rgba(0, 0, 0, 0.04), 0 3px 8px rgba(0, 0, 0, 0.1);
        }
        .gbq-button-tab-active::before {
          display: block;
          background: linear-gradient(to bottom, rgba(255, 255, 255, 0.72) 0%, rgba(255, 255, 255, 0) 60%);
        }
        .gbq-switch {
          position: relative;
          border: 0;
          border-radius: 9999px;
          background: rgba(120, 120, 128, 0.16);
          box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.16), inset 0 0 0 0.5px rgba(0, 0, 0, 0.06), 0 1px 0 rgba(255, 255, 255, 0.9);
          transition: background 240ms ease, box-shadow 240ms ease;
        }
        .gbq-switch-on {
          background: linear-gradient(to bottom, rgba(60, 214, 102, 0.95), rgba(40, 190, 86, 0.95));
          box-shadow: inset 0 1px 2px rgba(255, 255, 255, 0.5), inset 0 -1px 3px rgba(20, 120, 55, 0.35), inset 0 0 0 0.5px rgba(20, 120, 55, 0.3), 0 1px 3px rgba(40, 180, 80, 0.4);
        }
        .gbq-switch:hover:not(:disabled) {
          box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.2), inset 0 0 0 0.5px rgba(0, 0, 0, 0.08), 0 1px 0 rgba(255, 255, 255, 0.9);
        }
        .gbq-switch-on:hover:not(:disabled) {
          background: linear-gradient(to bottom, rgba(70, 220, 110, 1), rgba(44, 198, 92, 1));
          box-shadow: inset 0 1px 2px rgba(255, 255, 255, 0.6), inset 0 -1px 3px rgba(20, 120, 55, 0.4), inset 0 0 0 0.5px rgba(20, 120, 55, 0.34), 0 2px 6px rgba(40, 180, 80, 0.45);
        }
        .gbq-switch:focus-visible {
          outline: none;
          box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.16), 0 0 0 3.5px rgba(0, 122, 255, 0.35);
        }
        .gbq-switch-thumb {
          background: linear-gradient(to bottom, #ffffff, #f2f2f4);
          box-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.04), inset 0 1px 0 rgba(255, 255, 255, 1), 0 1px 2px rgba(0, 0, 0, 0.12), 0 3px 8px rgba(0, 0, 0, 0.18);
        }
        @media (prefers-reduced-transparency: reduce) {
          .gbq-glass {
            background: rgba(255, 255, 255, 0.92);
            -webkit-backdrop-filter: none;
            backdrop-filter: none;
          }
          .gbq-glass::before,
          .gbq-glass::after {
            display: none;
          }
          .gbq-button {
            background: rgba(255, 255, 255, 0.92);
            -webkit-backdrop-filter: none;
            backdrop-filter: none;
          }
          .gbq-button::before {
            display: none;
          }
          .gbq-button-primary {
            background: #6c6c72;
            color: #ffffff;
            box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.3), 0 4px 10px rgba(0, 0, 0, 0.14);
          }
          .gbq-button-primary::before,
          .gbq-button-primary::after {
            display: none;
          }
          .gbq-panel {
            background: rgba(255, 255, 255, 0.94);
            -webkit-backdrop-filter: none;
            backdrop-filter: none;
          }
          .gbq-panel::before {
            display: none;
          }
          .gbq-chip {
            background: rgba(255, 255, 255, 0.94);
            -webkit-backdrop-filter: none;
            backdrop-filter: none;
          }
          .gbq-chip::before {
            display: none;
          }
          .gbq-switch {
            background: rgba(120, 120, 128, 0.22);
          }
          .gbq-switch-on {
            background: rgb(40, 190, 86);
          }
          .gbq-switch-thumb {
            background: #ffffff;
          }
        }
        @media (prefers-contrast: more) {
          .gbq-glass {
            background: rgba(255, 255, 255, 0.97);
            border-color: rgba(29, 29, 31, 0.28);
            box-shadow: 0 12px 28px rgba(0, 0, 0, 0.16);
          }
          .gbq-button {
            background: rgba(255, 255, 255, 0.97);
            box-shadow: inset 0 0 0 1px rgba(29, 29, 31, 0.28), 0 4px 10px rgba(0, 0, 0, 0.12);
          }
          .gbq-button-primary {
            background: #5a5a60;
            color: #ffffff;
            box-shadow: inset 0 0 0 2px rgba(0, 0, 0, 0.4), 0 4px 10px rgba(0, 0, 0, 0.18);
          }
          .gbq-button-primary::before,
          .gbq-button-primary::after {
            display: none;
          }
          .gbq-panel {
            background: rgba(255, 255, 255, 0.98);
            box-shadow: inset 0 0 0 1px rgba(29, 29, 31, 0.24), 0 12px 28px rgba(0, 0, 0, 0.16);
          }
          .gbq-chip {
            background: rgba(255, 255, 255, 0.98);
            box-shadow: inset 0 0 0 1px rgba(29, 29, 31, 0.24);
          }
          .gbq-switch {
            background: rgba(120, 120, 128, 0.3);
            box-shadow: inset 0 0 0 1px rgba(29, 29, 31, 0.28);
          }
          .gbq-switch-on {
            background: rgb(30, 160, 70);
          }
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
