// Share type-aware diagnostics between editors, npm, and Cargo's staged build.
// Project service resolves each file's tsconfig.json; generated bundles stay out
// of linting while source rules prohibit unsafe values and imperative h() views.
import js from "@eslint/js";
import stylistic from "@stylistic/eslint-plugin";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default [
  {
    // Standalone validator JS is compiler output; freshness, formatting, typed declarations and runtime tests check it.
    ignores: ["node_modules/**", "target/**", "assets/**/vendor/**", "frontend/review/generated/validators.mjs"],
  },
  js.configs.recommended,
  {
    files: ["frontend/**/*.{ts,tsx,mts,mjs}", "scripts/*.mjs", "*config*.{mjs,ts}"],
    plugins: { "@stylistic": stylistic },
    rules: { "@stylistic/max-len": ["error", { code: 120, tabWidth: 2 }] },
  },
  ...tseslint.configs.recommendedTypeChecked.map((config) => ({
    ...config,
    files: ["frontend/**/*.{ts,tsx,mts}", "*config*.ts"],
  })),
  {
    files: ["frontend/**/*.{ts,tsx,mts}", "*config*.ts"],
    plugins: { "react-hooks": reactHooks },
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    linterOptions: {
      noInlineConfig: true,
      reportUnusedDisableDirectives: "error",
      reportUnusedInlineConfigs: "error",
    },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/explicit-module-boundary-types": "error",
      "@typescript-eslint/no-floating-promises": "error",
      "@typescript-eslint/no-unsafe-argument": "error",
      "@typescript-eslint/no-unsafe-assignment": "error",
      "@typescript-eslint/no-unsafe-call": "error",
      "@typescript-eslint/no-unsafe-member-access": "error",
      "@typescript-eslint/no-unsafe-return": "error",
      "@typescript-eslint/no-unused-vars": "error",
      "@typescript-eslint/switch-exhaustiveness-check": "error",
      "@typescript-eslint/no-non-null-assertion": "error",
      "@typescript-eslint/no-unnecessary-type-assertion": "error",
      "@typescript-eslint/ban-ts-comment": [
        "error",
        { "ts-expect-error": true, "ts-ignore": true, "ts-nocheck": true, "ts-check": false },
      ],
      "no-restricted-imports": [
        "error",
        { paths: [{ name: "preact", importNames: ["h"], message: "Use TSX markup and Preact's JSX runtime." }] },
      ],
      "no-restricted-syntax": [
        "error",
        { selector: "CallExpression[callee.name='h']", message: "Use TSX markup instead of h() calls." },
      ],
    },
  },
  {
    files: ["scripts/*.mjs", "frontend/**/*.mjs", "frontend/tests/**/*.ts", "*config*.{mjs,ts}"],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["assets/**/*.js", "frontend/**/*.{ts,tsx,mts}"],
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
