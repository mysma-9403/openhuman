// Behavioural cover for `scripts/check-domain-e2e-coverage.mjs` (#5936).
//
// The sibling `coverage-script-help.test.mjs` only asserts `--help` and
// argument rejection, so all three defects #5936 fixed were unpinned: the gate
// could go back to measuring a seventh of the surface and every test would stay
// green. That is the failure mode this file exists to make loud.
//
// Each test drives the real script against a fixture tree — the script keys off
// `process.cwd()`, so a temp dir with `src/openhuman/**` and `tests/**` is a
// complete world — and asserts on the specific defect, not on the exit code
// alone. Exit status is deliberately NOT the assertion where it cannot
// discriminate: an incomplete fixture trips the `declaredButMissing` guard, so
// several of these runs exit non-zero for more than one reason.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const HERE = dirname(fileURLToPath(import.meta.url));
const SCRIPT = resolve(HERE, '..', 'check-domain-e2e-coverage.mjs');

function write(root, relative, contents) {
  const full = join(root, relative);
  fs.mkdirSync(dirname(full), { recursive: true });
  fs.writeFileSync(full, contents);
}

/** A minimal `ControllerSchema` literal in the shape the scanner matches. */
function controller(namespace, fn) {
  return `
pub const SCHEMA: ControllerSchema = ControllerSchema {
    namespace: "${namespace}",
    function: "${fn}",
};
`;
}

function runGate(root, threshold = '90') {
  return spawnSync(process.execPath, [SCRIPT], {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, DOMAIN_E2E_COVERAGE_THRESHOLD: threshold },
  });
}

function fixture(t) {
  const root = fs.mkdtempSync(join(tmpdir(), 'domain-e2e-gate-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

// Defect (a): discovery used to read only files whose path matched
// `/(^|\/)schemas?(\.rs|\/)/`. The #5856/#5857 `include!` split moved
// `ControllerSchema` literals into `*_part_NN.rs` siblings, which that pattern
// does not match — 180 controllers across 12 namespaces went invisible in one
// commit and the gate simply reported a smaller world.
test('discovers controllers declared in an include!-split part file', (t) => {
  const root = fixture(t);
  // The filename is the point: `schemas_part_01.rs` does NOT match the old
  // path filter, because `schemas` is followed by `_` rather than `.rs` or `/`.
  write(root, 'src/openhuman/widgets/schemas_part_01.rs', controller('widgets', 'list'));
  write(root, 'tests/widgets_e2e.rs', 'let m = "openhuman.widgets_list";');

  const result = runGate(root);

  assert.match(
    result.stdout,
    /Discovered 1 controllers across 1 namespaces/,
    `the part file's controller must be discovered; got:\n${result.stdout}`,
  );
  assert.match(
    result.stdout,
    /\| widgets \| widgets \| 1\/1 \| 100\.0% \|/,
    `widgets must be measured at 1/1; got:\n${result.stdout}`,
  );
});

// Defect (b): `percent = expected.size === 0 ? 100 : …` meant a namespace whose
// controllers had all become invisible scored 100% — indistinguishable in the
// gate's own output from genuinely full coverage. Four namespaces were doing
// exactly that. A namespace MODULES names but discovery cannot see is now a
// hard failure, because nothing was measured.
test('fails loudly when a declared namespace discovers no controllers', (t) => {
  const root = fixture(t);
  write(root, 'src/openhuman/widgets/schemas_part_01.rs', controller('widgets', 'list'));
  write(root, 'tests/widgets_e2e.rs', 'let m = "openhuman.widgets_list";');

  const result = runGate(root);

  assert.equal(result.status, 1, `an unmeasurable namespace must fail the gate; got:\n${result.stdout}`);
  assert.match(
    result.stderr,
    /namespace\(s\) with no discovered controllers/,
    `the failure must say nothing was measured; got:\n${result.stderr}`,
  );
  // `config` is a MODULES entry with no controller in this fixture. Scoring it
  // 100% is the bug; naming it as unmeasured is the fix.
  assert.match(result.stderr, /\bconfig\b/, `the unmeasured namespace must be named; got:\n${result.stderr}`);
  assert.doesNotMatch(
    result.stdout,
    /\| config \|.*100\.0% \|/,
    `an unmeasured namespace must never be reported as 100% covered; got:\n${result.stdout}`,
  );
});

// Defect (c): MODULES used to be the *scope* of the check, so ~50 namespaces
// were never measured at any threshold simply because nobody added a line.
// MODULES is now presentational; every discovered namespace is measured.
test('measures a discovered namespace that MODULES does not name', (t) => {
  const root = fixture(t);
  // `widgets` appears nowhere in MODULES.
  write(root, 'src/openhuman/widgets/schemas_part_01.rs', controller('widgets', 'list'));
  write(root, 'src/openhuman/widgets/schemas_part_02.rs', controller('widgets', 'purge'));
  // Only one of the two is named by an e2e target: 1/2 = 50%, under the 90% bar.
  write(root, 'tests/widgets_e2e.rs', 'let m = "openhuman.widgets_list";');

  const result = runGate(root);

  assert.match(
    result.stdout,
    /\| widgets \| widgets \| 1\/2 \| 50\.0% \|/,
    `an unlisted namespace must still be measured; got:\n${result.stdout}`,
  );
  assert.match(
    result.stderr,
    /widgets \(1\/2, 50\.0%\)/,
    `an unlisted namespace below the threshold must fail the gate by name; got:\n${result.stderr}`,
  );
  assert.match(
    result.stdout,
    /openhuman\.widgets_purge/,
    `the uncovered controller must be reported as missing; got:\n${result.stdout}`,
  );
});
