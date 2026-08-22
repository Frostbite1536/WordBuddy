/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        background: {
          primary: "#09090b",
          secondary: "#18181b",
          card: "#1c1c1f",
        },
        brand: {
          DEFAULT: "#C3FF00",
          hover: "#a8dc00",
        },
        accent: {
          DEFAULT: "rgb(var(--accent-rgb) / <alpha-value>)",
          hover: "rgb(var(--accent-hover-rgb) / <alpha-value>)",
          muted: "rgb(var(--accent-rgb) / <alpha-value>)",
        },
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "sans-serif"],
        // font-heading is the in-use key; font-display is a parallel alias so
        // either name resolves to MD Nichrome without forcing a swap.
        display: ["MD Nichrome", "system-ui", "sans-serif"],
        heading: ["MD Nichrome", "system-ui", "sans-serif"],
        mono: ["JetBrains Mono", "monospace"],
      },
    },
  },
  plugins: [],
};
