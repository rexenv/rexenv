// Hooks-only lint gate. NOT a style linter, deliberately.
//
// Adopted 24 Aug 2026 after the rule was measured rather than guessed at
// (docs/TODO.md, ledger #396). The scope is two rules, and widening it is a
// separate decision with a separate cost: a general ruleset over a codebase that
// has never run one produces hundreds of findings, and a gate nobody can get to
// zero is a gate that gets `--max-warnings 999` and stops meaning anything.
//
// What earned these two:
//   `rules-of-hooks` would have caught the crash fixed in e9fc144 — a `useQuery`
//   after an early return, which reached a user-visible failure on /sites/:id and
//   was found by a WebKit render check written for something else. Proven by
//   reverting that commit and watching this config fail by name.
//
//   `exhaustive-deps` is a WARNING upstream and an ERROR here, because
//   `--max-warnings 0` would make the distinction meaningless anyway and an
//   error says plainly what the gate does. All three findings in the tree when
//   this landed were deliberate and are suppressed WITH a reason at the line.
//
// `reportUnusedDisableDirectives` is the half that keeps the suppressions
// honest: three of the seventeen that existed before this config suppressed
// problems that no longer existed, and nothing could tell you which three.
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default [
  { ignores: ["dist/**", "src-tauri/**", "scripts/wk-checks/node_modules/**"] },
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: { "react-hooks": reactHooks },
    linterOptions: { reportUnusedDisableDirectives: "error" },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      // A CORE rule, on for one reason: `SiteDetail`已 carried an
      // `eslint-disable-next-line no-control-regex` for a deliberate
      // control-character check on env values. With the rule off that directive
      // suppresses nothing and `reportUnusedDisableDirectives` calls it stale —
      // so leaving it off would have deleted an honest comment. Turning the rule
      // ON is what makes the comment load-bearing again, which is what the row
      // asked for: the suppressions should MEAN something.
      "no-control-regex": "error",
    },
  },
];
