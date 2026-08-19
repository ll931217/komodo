/**
 * Self-check for the unified resource list. Run it directly:
 *
 *   node ui/src/lib/unified-resources.check.ts
 *
 * The coverage assertion is the important one. UNIFIED_RESOURCES is a
 * hand-maintained list, and a new resource type that nobody adds to it
 * simply never appears on the page - the exact failure the sidebar
 * registry warns about, in its least visible form. So every type must be
 * either covered or explicitly excluded with a reason; there is no
 * "neither".
 */
import assert from "node:assert/strict";

import {
  REFRESH_REQUEST,
  UNIFIED_EXCLUDED,
  UNIFIED_RESOURCES,
  availableTags,
  filterRows,
  groupForRefresh,
  type UnifiedRow,
} from "./unified-resources.ts";

/**
 * Transcribed from RESOURCE_TARGETS in ui/src/resources/index.ts.
 * Importing it would drag in every resource component and its React
 * dependencies, which a plain node run cannot load.
 */
const RESOURCE_TARGETS = [
  "Server",
  "Swarm",
  "Cluster",
  "Application",
  "Terraform",
  "Stack",
  "Deployment",
  "Build",
  "Repo",
  "Procedure",
  "Action",
  "Builder",
  "Alerter",
  "ResourceSync",
];

let failures = 0;
function check(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  ok   ${name}`);
  } catch (error) {
    failures += 1;
    console.error(`  FAIL ${name}`);
    console.error(`       ${(error as Error).message.split("\n")[0]}`);
  }
}

console.log("unified-resources:");

check("every resource type is covered or explicitly excluded", () => {
  const unaccounted = RESOURCE_TARGETS.filter(
    (type) =>
      !UNIFIED_RESOURCES.includes(type as never) &&
      !(type in UNIFIED_EXCLUDED),
  );
  assert.deepEqual(
    unaccounted,
    [],
    `these types appear on neither list, so they would silently vanish ` +
      `from the unified page: ${unaccounted.join(", ")}`,
  );
});

check("no type is both included and excluded", () => {
  const both = UNIFIED_RESOURCES.filter((type) => type in UNIFIED_EXCLUDED);
  assert.deepEqual(both, [], `contradictory: ${both.join(", ")}`);
});

check("the include list names only real resource types", () => {
  const unknown = UNIFIED_RESOURCES.filter(
    (type) => !RESOURCE_TARGETS.includes(type),
  );
  assert.deepEqual(unknown, [], `not real types: ${unknown.join(", ")}`);
});

check("every refresh request targets a covered type", () => {
  const stray = Object.keys(REFRESH_REQUEST).filter(
    (type) => !UNIFIED_RESOURCES.includes(type as never),
  );
  assert.deepEqual(
    stray,
    [],
    `refresh configured for types the page never shows: ${stray.join(", ")}`,
  );
});

const rows: UnifiedRow[] = [
  { type: "Stack", id: "s1", name: "web-frontend", tags: ["prod", "web"] },
  { type: "Stack", id: "s2", name: "web-backend", tags: ["prod"] },
  { type: "Build", id: "b1", name: "frontend-image", tags: ["web"] },
  { type: "Procedure", id: "p1", name: "nightly", tags: [] },
];

check("no filters returns everything", () => {
  assert.equal(
    filterRows(rows, { types: [], tags: [], search: "" }).length,
    4,
  );
});

check("type filter narrows to that type", () => {
  const got = filterRows(rows, { types: ["Stack"], tags: [], search: "" });
  assert.deepEqual(got.map((r) => r.id), ["s1", "s2"]);
});

check("tags are ANDed, so a second tag never widens the result", () => {
  const one = filterRows(rows, { types: [], tags: ["prod"], search: "" });
  const two = filterRows(rows, {
    types: [],
    tags: ["prod", "web"],
    search: "",
  });
  assert.equal(one.length, 2);
  assert.equal(two.length, 1, "ORing tags would return 3 here");
  assert.ok(
    two.length <= one.length,
    "adding a tag must never show more rows",
  );
});

check("search is case-insensitive and matches substrings", () => {
  const got = filterRows(rows, { types: [], tags: [], search: "WEB-" });
  assert.deepEqual(got.map((r) => r.id), ["s1", "s2"]);
});

check("filters combine", () => {
  const got = filterRows(rows, {
    types: ["Stack"],
    tags: ["web"],
    search: "front",
  });
  assert.deepEqual(got.map((r) => r.id), ["s1"]);
});

check("refresh groups by type and reports what it cannot do", () => {
  const { batches, skipped } = groupForRefresh(rows);
  const stack = batches.find((b) => b.type === "Stack");
  assert.ok(stack, "stacks should batch together");
  assert.deepEqual(stack.ids, ["s1", "s2"]);
  assert.equal(stack.request, "RefreshStackCache");
  assert.deepEqual(
    skipped.map((r) => r.id),
    ["p1"],
    "a type with no refresh must be reported, not silently dropped",
  );
});

check("refreshing nothing is not an error", () => {
  const { batches, skipped } = groupForRefresh([]);
  assert.deepEqual(batches, []);
  assert.deepEqual(skipped, []);
});

/**
 * Verified against the live API: a resource's `tags` field holds tag
 * IDs, not names -
 *   ListStacks -> tags: ["69e5e349c39de2f284b74efe", ...]
 * so anything rendering or offering them raw shows opaque hex. The page
 * resolves them through ListTags. This pins the ASSUMPTION so the next
 * person does not "simplify" the resolution away.
 */
check("tag values are treated as opaque ids, never as display text", () => {
  const idRows: UnifiedRow[] = [
    {
      type: "Stack",
      id: "s1",
      name: "web",
      tags: ["69e5e349c39de2f284b74efe"],
    },
  ];
  const got = filterRows(idRows, {
    types: [],
    tags: ["69e5e349c39de2f284b74efe"],
    search: "",
  });
  assert.equal(got.length, 1, "filtering must match on the id");
  // Filtering by what a human would type must NOT match - if it did,
  // the id would be doubling as a name somewhere.
  assert.equal(
    filterRows(idRows, { types: [], tags: ["prod"], search: "" }).length,
    0,
  );
});

check("available tags are deduplicated and sorted", () => {
  assert.deepEqual(availableTags(rows), ["prod", "web"]);
  assert.deepEqual(availableTags([]), []);
});

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log("  all checks passed");
