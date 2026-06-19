/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          "-apple-system",
          "BlinkMacSystemFont",
          "Segoe UI",
          "system-ui",
          "sans-serif",
        ],
      },
      colors: {
        claude: "#ff8c42",
        codex: "#34c759",
        accent: "#5ac8fa",
      },
      backdropBlur: {
        glass: "28px",
      },
      boxShadow: {
        glass: "0 12px 40px rgba(0,0,0,0.35), inset 0 1px 0 rgba(255,255,255,0.15)",
      },
    },
  },
  plugins: [],
};
