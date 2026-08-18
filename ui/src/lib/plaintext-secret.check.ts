/**
 * Self-check for findPlaintextSecretKeys. Run it directly:
 *
 *   node ui/src/lib/plaintext-secret.check.ts
 *
 * No test framework - the ui has none, and adding one to guard a single
 * regex is a worse trade than a file that runs on plain node. Kept out
 * of the app import graph so vite never bundles it.
 */
import assert from "node:assert/strict";

import { findPlaintextSecretKeys } from "./plaintext-secret.ts";

const WARNS: Array<[string, string[]]> = [
  ["DB_PASSWORD = hunter2", ["DB_PASSWORD"]],
  ["API_KEY=sk-live-abc123", ["API_KEY"]],
  ["  ACCESS_KEY : AKIAIOSFODNN7EXAMPLE", ["ACCESS_KEY"]],
  ['PASSWORD="quoted still counts"', ["PASSWORD"]],
  // Value shape alone, key says nothing.
  [
    "DATABASE_URL=postgres://admin:s3cr3t@db.internal:5432/app",
    ["DATABASE_URL"],
  ],
  ["DEPLOY_KEY=-----BEGIN OPENSSH PRIVATE KEY-----", ["DEPLOY_KEY"]],
  // Several lines, only the offender is named.
  [
    "PORT = 8080\nLOG_LEVEL = debug\nSTRIPE_SECRET_KEY = sk_live_x\n",
    ["STRIPE_SECRET_KEY"],
  ],
];

const QUIET: string[] = [
  // The whole point: a reference is the correct thing to do.
  "DB_PASSWORD = [[POSTGRES_PASSWORD]]",
  "API_TOKEN=[[GITLAB_TOKEN]]",
  // The literal escape is still not a pasted secret.
  "DB_PASSWORD = [[[POSTGRES_PASSWORD]]]",
  // Resolved elsewhere, not stored here.
  "DB_PASSWORD = ${POSTGRES_PASSWORD}",
  "DB_PASSWORD = $POSTGRES_PASSWORD",
  // Ordinary config must never trip it, or nobody reads the warning.
  "PORT = 8080\nLOG_LEVEL = debug\nDATABASE_HOST = db.internal\nUSER = app",
  "REDIS_URL = redis://cache.internal:6379",
  // Names a location, not a value.
  "PASSWORD_FILE = /run/secrets/db_password",
  "SECRET_NAME = app-tls",
  "TLS_KEY_PATH = /etc/ssl/private/app.key",
  // Nothing to leak.
  "DB_PASSWORD =",
  "# DB_PASSWORD = hunter2",
  "",
];

let failures = 0;

for (const [input, expected] of WARNS) {
  try {
    assert.deepEqual(findPlaintextSecretKeys(input), expected);
  } catch {
    failures++;
    console.error(
      `MISSED  ${JSON.stringify(input)}\n  expected ${JSON.stringify(expected)}` +
        `, got ${JSON.stringify(findPlaintextSecretKeys(input))}`,
    );
  }
}

for (const input of QUIET) {
  const got = findPlaintextSecretKeys(input);
  if (got.length > 0) {
    failures++;
    console.error(
      `FALSE POSITIVE  ${JSON.stringify(input)}\n  flagged ${JSON.stringify(got)}`,
    );
  }
}

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}

console.log(
  `plaintext-secret: ${WARNS.length} detections and ${QUIET.length} quiet cases all pass`,
);
