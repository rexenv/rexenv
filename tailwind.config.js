/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Raw rexenv palette (reference these directly when not using a shadcn token)
        rex: {
          bg: "var(--rex-bg)",
          "surface-1": "var(--rex-surface-1)",
          "surface-2": "var(--rex-surface-2)",
          "surface-2-hover": "var(--rex-surface-2-hover)",
          "surface-3": "var(--rex-surface-3)",
          well: "var(--rex-well)",
          border: "var(--rex-border)",
          "border-subtle": "var(--rex-border-subtle)",
          "border-strong": "var(--rex-border-strong)",
          "border-strong-hover": "var(--rex-border-strong-hover)",
          text: "var(--rex-text)",
          "text-bright": "var(--rex-text-bright)",
          "text-muted": "var(--rex-text-muted)",
          "text-dim": "var(--rex-text-dim)",
          "text-faint": "var(--rex-text-faint)",
          "text-label": "var(--rex-text-label)",
        },
        brand: {
          DEFAULT: "var(--rex-brand)",
          hover: "var(--rex-brand-hover)",
          strong: "var(--rex-brand-strong)",
          light: "var(--rex-brand-light)",
          tint: "var(--rex-brand-tint)",
          "tint-bg": "var(--rex-brand-tint-bg)",
          active: "var(--rex-brand-active)",
        },
        danger: {
          DEFAULT: "var(--rex-danger)",
          hover: "var(--rex-danger-hover)",
          active: "var(--rex-danger-active)",
        },
        status: {
          running: "var(--rex-running)",
          "running-bright": "var(--rex-running-bright)",
          stopped: "var(--rex-stopped)",
          error: "var(--rex-error)",
          "error-bright": "var(--rex-error-bright)",
          warning: "var(--rex-warning)",
          "warning-bright": "var(--rex-warning-bright)",
          // pill tints (translucent fill + border per status)
          "running-bg": "var(--rex-running-bg)",
          "running-border": "var(--rex-running-border)",
          "warning-bg": "var(--rex-warning-bg)",
          "warning-border": "var(--rex-warning-border)",
          "stopped-bg": "var(--rex-stopped-bg)",
          "stopped-border": "var(--rex-stopped-border)",
          "error-bg": "var(--rex-error-bg)",
          "error-border": "var(--rex-error-border)",
        },
        toggle: {
          on: "var(--rex-toggle-on)",
          "on-border": "var(--rex-toggle-on-border)",
          off: "var(--rex-toggle-off)",
          "off-border": "var(--rex-toggle-off-border)",
        },
        // shadcn/ui semantic tokens
        background: "var(--background)",
        foreground: "var(--foreground)",
        card: {
          DEFAULT: "var(--card)",
          foreground: "var(--card-foreground)",
        },
        popover: {
          DEFAULT: "var(--popover)",
          foreground: "var(--popover-foreground)",
        },
        primary: {
          DEFAULT: "var(--primary)",
          foreground: "var(--primary-foreground)",
        },
        secondary: {
          DEFAULT: "var(--secondary)",
          foreground: "var(--secondary-foreground)",
        },
        muted: {
          DEFAULT: "var(--muted)",
          foreground: "var(--muted-foreground)",
        },
        accent: {
          DEFAULT: "var(--accent)",
          foreground: "var(--accent-foreground)",
        },
        destructive: {
          DEFAULT: "var(--destructive)",
          foreground: "var(--destructive-foreground)",
        },
        border: "var(--border)",
        input: "var(--input)",
        ring: "var(--ring)",
      },
      fontFamily: {
        display: "var(--rex-font-display)",
        sans: "var(--rex-font-ui)",
        mono: "var(--rex-font-mono)",
      },
      borderRadius: {
        sm: "var(--rex-radius-sm)",
        DEFAULT: "var(--rex-radius)",
        md: "var(--rex-radius)",
        lg: "var(--rex-radius-lg)",
        xl: "var(--rex-radius-xl)",
      },
      boxShadow: {
        card: "var(--rex-shadow-card)",
        menu: "var(--rex-shadow-menu)",
        "glow-primary": "var(--rex-glow-primary)",
        "glow-run": "var(--rex-glow-run)",
        "glow-err": "var(--rex-glow-err)",
        "glow-crown": "var(--rex-glow-crown)",
      },
      keyframes: {
        "rex-ping": {
          "0%": { transform: "scale(1)", opacity: "0.5" },
          "70%": { opacity: "0" },
          "100%": { transform: "scale(2.7)", opacity: "0" },
        },
        "rex-spin": {
          to: { transform: "rotate(360deg)" },
        },
        "rex-err": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.4" },
        },
        "rex-float": {
          "0%, 100%": { transform: "translateY(0)" },
          "50%": { transform: "translateY(-8px)" },
        },
        "rex-aura": {
          "0%, 100%": { opacity: "0.7", transform: "translateX(-50%) scale(1)" },
          "50%": { opacity: "1", transform: "translateX(-50%) scale(1.08)" },
        },
      },
      animation: {
        "rex-ping": "rex-ping 2.4s ease-out infinite",
        "rex-spin": "rex-spin 0.9s linear infinite",
        "rex-err": "rex-err 1.8s ease-in-out infinite",
        "rex-float": "rex-float 4s ease-in-out infinite",
        "rex-aura": "rex-aura 6s ease-in-out infinite",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};
