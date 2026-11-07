import type { Config } from 'tailwindcss';

/**
 * Tailwind configuration aligned with `game-node-clients/packages/config-tailwind`.
 *
 * Colors, spacing, radii, and font sizes reuse the Version 3.0 CSS variables from
 * `src/styles/theme.css` so the desktop Tauri shell and the web clients stay in sync.
 */
const config: Config = {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  corePlugins: {
    preflight: true,
  },
  theme: {
    extend: {
      fontFamily: {
        sans: [
          'Roboto',
          '-apple-system',
          'BlinkMacSystemFont',
          'Segoe UI',
          'Oxygen',
          'Ubuntu',
          'Cantarell',
          'sans-serif',
        ],
      },
      spacing: {
        xs: 'var(--spacing-xs)',
        sm: 'var(--spacing-sm)',
        md: 'var(--spacing-md)',
        lg: 'var(--spacing-lg)',
        xl: 'var(--spacing-xl)',
        compact: 'var(--spacing-compact)',
        'control-inline': 'var(--spacing-control-inline)',
      },
      inset: {
        topbar: 'var(--size-topbar-height)',
        sidebar: 'var(--sidebar-width)',
      },
      borderRadius: {
        xs: 'var(--radius-xs)',
        sm: 'var(--radius-sm)',
        md: 'var(--radius-md)',
        lg: 'var(--radius-lg)',
        xl: 'var(--radius-xl)',
      },
      fontSize: {
        xs: 'var(--font-size-xs)',
        sm: 'var(--font-size-sm)',
        md: 'var(--font-size-md)',
        lg: 'var(--font-size-lg)',
        xl: 'var(--font-size-xl)',
      },
      colors: {
        body: 'var(--color-background)',
        paper: {
          DEFAULT: '#161616',
          0: '#161616',
          1: '#262525',
          2: '#1D1D1D',
          3: '#191919',
          4: '#242424',
          5: '#1C1C1C',
          6: '#212121',
          7: '#111111',
          8: '#1A1A1A',
        },
        dimmed: {
          DEFAULT: '#808080',
          0: '#808080',
          1: '#808080',
        },
        brand: {
          DEFAULT: '#f15025',
          0: '#ffede5',
          1: '#ffd9cf',
          2: '#fbb3a0',
          3: '#f7896d',
          4: '#f36742',
          5: '#f15025',
          6: '#f14517',
          7: '#d6360b',
          8: '#c02d06',
          9: '#a82301',
        },
        secondary: '#1A1A1A',
        tertiary: '#872C13',
        chrome: '#111111',
        'chrome-border': '#202020',
        surface: '#262525',
        raised: '#222222',
        'border-subtle': '#2A2A2A',
        'text-default': '#D9D9D9',
        'text-dimmed': '#808080',
        'on-primary': '#EDEDED',
        danger: {
          DEFAULT: '#E5484D',
          subtle: '#E5484D1F',
        },
        success: {
          DEFAULT: '#3DD68C',
          subtle: '#3DD68C1F',
        },
        warning: {
          DEFAULT: '#F5A524',
          subtle: '#F5A5241F',
        },
        'accent-subtle': '#F150251F',
      },
      boxShadow: {
        modal: '0 32px 80px rgba(0, 0, 0, 0.55)',
        alert: '0 24px 64px rgba(0, 0, 0, 0.6)',
        toast: '0 12px 32px rgba(0, 0, 0, 0.5)',
      },
      maxWidth: {
        modal: 'var(--modal-max-width)',
      },
      maxHeight: {
        modal: 'var(--modal-max-height)',
      },
      width: {
        sidebar: 'var(--sidebar-width)',
        'sidebar-compact': 'var(--sidebar-width-compact)',
        logo: 'var(--size-logo-width)',
      },
      height: {
        topbar: 'var(--size-topbar-height)',
        control: 'var(--size-control-height)',
        logo: 'var(--size-logo-height)',
      },
    },
  },
  plugins: [],
};

export default config;
