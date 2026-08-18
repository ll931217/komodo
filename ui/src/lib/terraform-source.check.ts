/**
 * Self-check for terraformSourceKind. Run it directly:
 *
 *   node ui/src/lib/terraform-source.check.ts
 *
 * The truth table below is transcribed from TerraformConfig::source_kind
 * in client/core/rs/src/entities/terraform.rs. If that precedence ever
 * changes, this is what should fail.
 */
import assert from "node:assert/strict";

import {
  terraformClones,
  terraformSourceKind,
  type TerraformSourceKind,
} from "./terraform-source.ts";

type Config = {
  files_on_host?: boolean;
  linked_repo?: string;
  repo?: string;
};

const CASES: Array<[Config, TerraformSourceKind]> = [
  // Nothing configured.
  [{}, "Contents"],
  [{ files_on_host: false, linked_repo: "", repo: "" }, "Contents"],

  // One source each.
  [{ files_on_host: true }, "FilesOnHost"],
  [{ linked_repo: "staging" }, "LinkedRepo"],
  [{ repo: "org/infra" }, "Repo"],

  // Precedence, which is the whole point: files_on_host beats a linked
  // repo, and a linked repo beats an inline one.
  [{ files_on_host: true, linked_repo: "staging" }, "FilesOnHost"],
  [{ files_on_host: true, repo: "org/infra" }, "FilesOnHost"],
  [
    { files_on_host: true, linked_repo: "staging", repo: "org/infra" },
    "FilesOnHost",
  ],
  [{ linked_repo: "staging", repo: "org/infra" }, "LinkedRepo"],
];

const CLONES: Array<[TerraformSourceKind, boolean]> = [
  ["FilesOnHost", false],
  // The one that is easy to get wrong: a linked Repo is cloned, so
  // reclone applies to it.
  ["LinkedRepo", true],
  ["Repo", true],
  ["Contents", false],
];

let failures = 0;

for (const [config, expected] of CASES) {
  const got = terraformSourceKind(config);
  if (got !== expected) {
    failures++;
    console.error(
      `${JSON.stringify(config)}\n  expected ${expected}, got ${got}`,
    );
  }
}

for (const [kind, expected] of CLONES) {
  const got = terraformClones(kind);
  if (got !== expected) {
    failures++;
    console.error(`terraformClones(${kind}) expected ${expected}, got ${got}`);
  }
}

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}

console.log(
  `terraform-source: ${CASES.length} precedence cases and ${CLONES.length} clone cases pass`,
);
