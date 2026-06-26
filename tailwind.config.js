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
          "surface-3": "var(--rex-surface-3)",
          well: "var(--rex-well)",
          border: "var(--rex-border)",
          "border-subtle": "var(--rex-border-subtle)",
          "border-strong": "var(--rex-border-strong)",
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
        },
        status: {
          running: "var(--rex-running)",
          "running-bright": "var(--rex-running-bright)",
          stopped: "var(--rex-stopped)",
          error: "var(--rex-error)",
          warning: "var(--rex-warning)",
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
        "glow-primary": "var(--rex-glow-primary)",
      },
      keyframes: {
        "rex-ping": {
          "0%": { transform: "scale(1)", opacity: "0.5" },
          "70%": { opacity: "0" },
          "100%": { transform: "scale(2.7)", opacity: "0" },
        },
      },
      animation: {
        "rex-ping": "rex-ping 2s cubic-bezier(0, 0, 0.2, 1) infinite",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};
