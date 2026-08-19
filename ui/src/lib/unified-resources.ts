/**
 * The data model behind the unified resource list.
 *
 * Kept out of the page component so the rules that decide what appears,
 * and what a cross-type action does, can be tested without rendering
 * anything. See unified-resources.check.ts.
 */
import type { UsableResource } from "@/resources";

/**
 * Resource types the unified list covers, in display order.
 *
 * An explicit list rather than a filter over RESOURCE_TARGETS, because
 * the page calls one `useList` hook per type and React requires those
 * calls in a stable order - a list derived from a runtime predicate
 * could change length between renders.
 *
 * The cost of an explicit list is drift: this codebase has a history of
 * hand-maintained per-type maps silently omitting a new resource, which
 * the sidebar registry comments about at length. So the check file
 * asserts this against the registry rather than trusting it.
 */
export const UNIFIED_RESOURCES: UsableResource[] = [
  "Stack",
  "Deployment",
  "Application",
  "Terraform",
  "Build",
  "Repo",
  "Procedure",
  "Action",
  "ResourceSync",
];

/**
 * Types deliberately excluded, with the reason, so the next person does
 * not "fix" an omission that is a decision.
 *
 * Server / Swarm / Cluster are infrastructure - things resources run
 * ON, not things you deploy. Builder / Alerter are settings.
 */
export const UNIFIED_EXCLUDED: Record<string, string> = {
  Server: "infrastructure - resources run on it",
  Swarm: "infrastructure - resources run on it",
  Cluster: "infrastructure - resources run on it",
  Builder: "settings, not a deployable resource",
  Alerter: "settings, not a deployable resource",
};

/**
 * The write request that refreshes each type's cached state.
 *
 * Only types with a refresh appear. A type absent here simply has no
 * refresh to offer - the bulk action skips it rather than failing the
 * whole batch, because a partial refresh is more useful than none.
 */
export const REFRESH_REQUEST: Partial<Record<UsableResource, string>> = {
  Stack: "RefreshStackCache",
  Build: "RefreshBuildCache",
  Repo: "RefreshRepoCache",
  ResourceSync: "RefreshResourceSyncPending",
};

/** One row in the unified list, flattened from any resource type. */
export interface UnifiedRow {
  type: UsableResource;
  id: string;
  name: string;
  tags: string[];
}

export interface UnifiedFilters {
  /** Empty means every type in UNIFIED_RESOURCES. */
  types: UsableResource[];
  /** Every listed tag must be present - AND, not OR. */
  tags: string[];
  /** Case-insensitive substring of the name. */
  search: string;
}

/**
 * Apply the filters to a row set.
 *
 * Tags are ANDed on purpose. Tag filtering exists to narrow a large
 * list, and ORing two tags widens it - selecting a second tag would
 * show MORE rows, which reads as the filter being broken.
 */
export function filterRows(
  rows: UnifiedRow[],
  filters: UnifiedFilters,
): UnifiedRow[] {
  const search = filters.search.trim().toLowerCase();
  return rows.filter((row) => {
    if (filters.types.length && !filters.types.includes(row.type)) {
      return false;
    }
    if (
      filters.tags.length &&
      !filters.tags.every((tag) => row.tags.includes(tag))
    ) {
      return false;
    }
    if (search && !row.name.toLowerCase().includes(search)) {
      return false;
    }
    return true;
  });
}

/**
 * Group selected rows by the refresh request they need.
 *
 * Rows whose type has no refresh are reported separately rather than
 * dropped, so the UI can say what it skipped instead of silently doing
 * less than the button promised.
 */
export function groupForRefresh(rows: UnifiedRow[]): {
  batches: Array<{ request: string; type: UsableResource; ids: string[] }>;
  skipped: UnifiedRow[];
} {
  const batches = new Map<
    string,
    { request: string; type: UsableResource; ids: string[] }
  >();
  const skipped: UnifiedRow[] = [];
  for (const row of rows) {
    const request = REFRESH_REQUEST[row.type];
    if (!request) {
      skipped.push(row);
      continue;
    }
    const key = `${row.type}:${request}`;
    const existing = batches.get(key);
    if (existing) {
      existing.ids.push(row.id);
    } else {
      batches.set(key, { request, type: row.type, ids: [row.id] });
    }
  }
  return { batches: [...batches.values()], skipped };
}

/** Every distinct tag across the rows, sorted, for the filter control. */
export function availableTags(rows: UnifiedRow[]): string[] {
  return [...new Set(rows.flatMap((row) => row.tags))].sort();
}
