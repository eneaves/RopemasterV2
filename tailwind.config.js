// tailwind.config.js (ESM)
import animate from "tailwindcss-animate";
import fs from "fs";
import path from "path";

/** @type {import('tailwindcss').Config} */
// Load design tokens (fallback to existing var-based values if missing)
const tokensPath = path.resolve(process.cwd(), "src/design-tokens.json");
let tokens = null;
try {
  tokens = JSON.parse(fs.readFileSync(tokensPath, "utf8"));
} catch (e) {
  // If tokens fail to load, we continue and rely on CSS variables already defined in :root
  tokens = null;
}

const g = tokens?.tokenSets?.global || {};
const color = g.color || {};
const font = g.font || {};
const size = g.size || {};
const shadow = g.shadow || {};

const map = {
  colors: {
    // primary shades (if available)
    ...(color.brand && color.brand['primary']
      ? {
          'primary-50': color.brand['primary-50']?.value,
          'primary-100': color.brand['primary-100']?.value,
          'primary-200': color.brand['primary-200']?.value,
          'primary-300': color.brand['primary-300']?.value,
          'primary-400': color.brand['primary-400']?.value,
          'primary-500': color.brand['primary-500']?.value,
          'primary-600': color.brand['primary-600']?.value,
          'primary-700': color.brand['primary-700']?.value,
          'primary-800': color.brand['primary-800']?.value,
          'primary-900': color.brand['primary-900']?.value,
        }
      : {}),
    // neutrals
    ...(color.neutral
      ? {
          'neutral-50': color.neutral['50']?.value,
          'neutral-100': color.neutral['100']?.value,
          'neutral-200': color.neutral['200']?.value,
          'neutral-300': color.neutral['300']?.value,
          'neutral-400': color.neutral['400']?.value,
          'neutral-500': color.neutral['500']?.value,
          'neutral-600': color.neutral['600']?.value,
          'neutral-700': color.neutral['700']?.value,
          'neutral-800': color.neutral['800']?.value,
          'neutral-900': color.neutral['900']?.value,
        }
      : {}),
    // semantic mappings
    ...(color.background
      ? {
          background: color.background.primary?.value,
          card: color.background.card?.value,
        }
      : {}),
    ...(color.text ? { foreground: color.text.primary?.value } : {}),
    ...(color.border ? { border: color.border.default?.value } : {}),
    ...(color.state ? { chart: {
      1: color.state['success-base']?.value,
      2: color.state['warning-base']?.value,
      3: color.state['info-base']?.value,
    }} : {}),
  },
  spacing: {
    ...(size.spacing ? Object.fromEntries(Object.entries(size.spacing).map(([k,v])=>[k, v.value])) : {}),
  },
  borderRadius: {
    ...(size.borderRadius ? {
      none: size.borderRadius.none?.value,
      sm: size.borderRadius.sm?.value,
      base: size.borderRadius.base?.value,
      md: size.borderRadius.md?.value,
      lg: size.borderRadius.lg?.value,
      xl: size.borderRadius.xl?.value,
      '2xl': size.borderRadius['2xl']?.value,
      full: size.borderRadius.full?.value,
      card: size.borderRadius['component-card']?.value,
      button: size.borderRadius['component-button']?.value,
      input: size.borderRadius['component-input']?.value,
    } : {}),
  },
  boxShadow: {
    card: shadow['component-card']?.value,
    'card-hover': shadow['component-card-hover']?.value,
    xs: shadow.xs?.value,
    sm: shadow.sm?.value,
    base: shadow.base?.value,
    md: shadow.md?.value,
    lg: shadow.lg?.value,
    xl: shadow.xl?.value,
  },
  fontFamily: {
    sans: font.family?.sans?.value,
    mono: font.family?.mono?.value,
  },
  fontSize: {
    xs: font.size?.xs?.value,
    sm: font.size?.sm?.value,
    base: font.size?.base?.value,
    lg: font.size?.lg?.value,
    xl: font.size?.xl?.value,
    '2xl': font.size?.['2xl']?.value,
    '3xl': font.size?.['3xl']?.value,
    '4xl': font.size?.['4xl']?.value,
    '5xl': font.size?.['5xl']?.value,
  },
  boxShadowLookup: shadow,
};

export default {
  darkMode: ["class"],
  content: [
    "./index.html",
    "./src/**/*.{ts,tsx,js,jsx}",
    "./src/components/**/*.{ts,tsx,js,jsx}",
    "./src/lib/**/*.{ts,tsx,js,jsx}",
  ],
  theme: {
    extend: {
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
        ...map.borderRadius,
      },
      colors: {
        // preserve var-based semantic colors for runtime theming
        background: "var(--background)",
        foreground: "var(--foreground)",
        card: { DEFAULT: "var(--card)", foreground: "var(--card-foreground)" },
        popover: { DEFAULT: "var(--popover)", foreground: "var(--popover-foreground)" },
        primary: { DEFAULT: "var(--primary)", foreground: "var(--primary-foreground)" },
        secondary: { DEFAULT: "var(--secondary)", foreground: "var(--secondary-foreground)" },
        muted: { DEFAULT: "var(--muted)", foreground: "var(--muted-foreground)" },
        accent: { DEFAULT: "var(--accent)", foreground: "var(--accent-foreground)" },
        destructive: { DEFAULT: "var(--destructive)", foreground: "var(--destructive-foreground)" },
        border: "var(--border)",
        input: "var(--input)",
        ring: "var(--ring)",
        chart: {
          1: "var(--chart-1)",
          2: "var(--chart-2)",
          3: "var(--chart-3)",
          4: "var(--chart-4)",
          5: "var(--chart-5)",
        },
        // static token-derived colors (enable utilities like bg-primary-500)
        ...(map.colors || {}),
      },
      spacing: {
        ...(map.spacing || {}),
      },
      boxShadow: {
        ...(map.boxShadow || {}),
      },
      fontFamily: {
        ...(map.fontFamily || {}),
      },
      fontSize: {
        ...(map.fontSize || {}),
      },
      keyframes: {
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: "0" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
    },
  },
  plugins: [animate],
};
