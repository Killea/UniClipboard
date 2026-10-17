// lint-staged task table (moved out of package.json so TS tasks can filter
// tool-ignored inputs first — oxlint exits 1 on "no files" when every staged
// file is ignored, e.g. a commit that only regenerates src/api/generated/).
//
// Keep this list in sync with the generated-code entries in
// .oxlintrc.json `ignorePatterns`; only entries that can plausibly be staged
// matter here (logs/lockfiles/node_modules are never staged anyway).
const OXLINT_IGNORED_STAGED = [
  /apps\/gui\/src\/api\/generated\//,
  /apps\/gui\/src\/lib\/[^/]*\.generated\.ts$/,
];

const quote = (file) => JSON.stringify(file);

const tsTasks = (files) => {
  const rest = files.filter((f) => !OXLINT_IGNORED_STAGED.some((re) => re.test(f)));
  if (rest.length === 0) {
    return [];
  }
  const args = rest.map(quote).join(' ');
  return [`oxlint --fix ${args}`, `oxfmt --write --no-error-on-unmatched-pattern ${args}`];
};

export default {
  'Cargo.toml': ['node scripts/refresh-agents-structure.mjs'],
  '*.{js,jsx,ts,tsx}': tsTasks,
  '*.md': ['node scripts/fix-md-cjk-emphasis.mjs', 'autocorrect --fix'],
  '*.{json,css,html}': ['oxfmt --write --no-error-on-unmatched-pattern'],
  '{apps,crates}/**/*.rs': ['rustfmt --edition 2021'],
};
