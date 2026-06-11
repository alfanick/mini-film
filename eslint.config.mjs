import js from "@eslint/js";
import globals from "globals";

export default [
  {
    ignores: ["node_modules/**", "target/**", "assets/**/vendor/**"],
  },
  js.configs.recommended,
  {
    files: ["assets/**/*.js"],
    languageOptions: {
      ecmaVersion: 2023,
      sourceType: "module",
      globals: {
        ...globals.browser,
      },
    },
    rules: {
      "no-console": "off",
    },
  },
];
