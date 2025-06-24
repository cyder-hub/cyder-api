/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx,css,md,mdx,html,json,scss}", // Ensure this covers all your component/page files
  ],
  darkMode: 'class', // Optional: Or 'media'
  theme: {
    extend: {},
  },
  plugins: [],
}
