import js from "@eslint/js";
import globals from "globals";

export default [
  {
    ignores: ["node_modules/**", "target/**"],
  },
  js.configs.recommended,
  {
    files: ["assets/**/*.js"],
    languageOptions: {
      ecmaVersion: 2023,
      sourceType: "script",
      globals: {
        ...globals.browser,
      },
    },
    rules: {
      "no-console": "off",
    },
  },
];
