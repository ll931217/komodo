/**
 * One list spanning every deployable resource type.
 *
 * Komodo's lists are per type, so answering "what is broken right now"
 * means visiting nine pages. This is the single page that answers it,
 * with the same status badge each type already renders on its own page -
 * reused rather than reimplemented, so a state this page shows can never
 * disagree with the one the resource's own page shows.
 */
import { useMemo, useState } from "react";
import {
  Box,
  Button,
  Checkbox,
  Group,
  MultiSelect,
  Table,
  Text,
  TextInput,
} from "@mantine/core";
import { Page } from "mogh_ui";
import { RefreshCw, Search } from "lucide-react";

import { useRead, useSetTitle, useWrite } from "@/lib/hooks";
import TableTags from "@/components/tags/table";
import { ResourceComponents, type UsableResource } from "@/resources";
import ResourceLink from "@/resources/link";
import {
  UNIFIED_RESOURCES,
  availableTags,
  filterRows,
  groupForRefresh,
  type UnifiedRow,
} from "@/lib/unified-resources";

/** Stable key for a row across types - ids are only unique per type. */
const rowKey = (row: UnifiedRow) => `${row.type}:${row.id}`;

export default function UnifiedResources() {
  useSetTitle("All Resources");

  const [search, setSearch] = useState("");
  const [types, setTypes] = useState<string[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const [selected, setSelected] = useState<string[]>([]);

  // One useList call per type, written out rather than looped. React
  // requires a stable hook order, and a loop over a list that anything
  // could filter would break that the first time it changed length.
  const stacks = ResourceComponents.Stack.useList();
  const deployments = ResourceComponents.Deployment.useList();
  const applications = ResourceComponents.Application.useList();
  const terraforms = ResourceComponents.Terraform.useList();
  const builds = ResourceComponents.Build.useList();
  const repos = ResourceComponents.Repo.useList();
  const procedures = ResourceComponents.Procedure.useList();
  const actions = ResourceComponents.Action.useList();
  const syncs = ResourceComponents.ResourceSync.useList();

  const rows: UnifiedRow[] = useMemo(() => {
    const collect = (
      type: UsableResource,
      items: Array<{ id: string; name: string; tags: string[] }> | undefined,
    ): UnifiedRow[] =>
      (items ?? []).map((item) => ({
        type,
        id: item.id,
        name: item.name,
        tags: item.tags ?? [],
      }));
    return [
      ...collect("Stack", stacks),
      ...collect("Deployment", deployments),
      ...collect("Application", applications),
      ...collect("Terraform", terraforms),
      ...collect("Build", builds),
      ...collect("Repo", repos),
      ...collect("Procedure", procedures),
      ...collect("Action", actions),
      ...collect("ResourceSync", syncs),
    ];
  }, [
    stacks,
    deployments,
    applications,
    terraforms,
    builds,
    repos,
    procedures,
    actions,
    syncs,
  ]);

  const allTags = useRead("ListTags", {}).data;
  const tagOptions = useMemo(() => {
    const inUse = availableTags(rows);
    return inUse.map((id) => ({
      value: id,
      // An id with no matching tag still gets an option rather than
      // vanishing: it means a tag was deleted while resources still
      // reference it, and hiding that makes the orphan unfindable.
      label: allTags?.find((tag) => tag._id?.$oid === id)?.name ?? id,
    }));
  }, [rows, allTags]);

  const filtered = useMemo(
    () =>
      filterRows(rows, {
        types: types as UsableResource[],
        tags,
        search,
      }),
    [rows, types, tags, search],
  );

  const selectedRows = useMemo(
    () => filtered.filter((row) => selected.includes(rowKey(row))),
    [filtered, selected],
  );

  const allShownSelected =
    filtered.length > 0 &&
    filtered.every((row) => selected.includes(rowKey(row)));

  return (
    <Page
      title="All Resources"
      icon={Search}
      description={
        <Text size="sm" c="dimmed">
          Every deployable resource in one place, with the same status
          badge each type shows on its own page.
        </Text>
      }
    >
      <Group mb="md" align="flex-end" wrap="wrap">
        <TextInput
          label="Search"
          placeholder="Filter by name"
          value={search}
          onChange={(event) => setSearch(event.currentTarget.value)}
          leftSection={<Search size={14} />}
          w={{ base: "100%", sm: 240 }}
        />
        <MultiSelect
          label="Type"
          placeholder={types.length ? undefined : "All types"}
          data={UNIFIED_RESOURCES}
          value={types}
          onChange={setTypes}
          clearable
          w={{ base: "100%", sm: 260 }}
        />
        <MultiSelect
          label="Tags"
          placeholder={tags.length ? undefined : "All tags"}
          // Values are tag IDs, because that is what a resource stores;
          // labels are names, because that is what a person knows. Only
          // tags actually in use are offered - the full list would be
          // mostly options that match nothing.
          // ANDed, per filterRows: a second tag narrows, never widens.
          data={tagOptions}
          value={tags}
          onChange={setTags}
          clearable
          searchable
          w={{ base: "100%", sm: 260 }}
        />
        <BulkRefresh
          rows={selectedRows}
          onDone={() => setSelected([])}
        />
      </Group>

      <Text size="sm" c="dimmed" mb="xs">
        {filtered.length} of {rows.length} resources
        {selected.length > 0 && ` · ${selectedRows.length} selected`}
      </Text>

      <Box style={{ overflowX: "auto" }}>
        <Table highlightOnHover>
          <Table.Thead>
            <Table.Tr>
              <Table.Th w={40}>
                <Checkbox
                  aria-label="Select all shown"
                  checked={allShownSelected}
                  indeterminate={!allShownSelected && selectedRows.length > 0}
                  onChange={() =>
                    setSelected(
                      allShownSelected ? [] : filtered.map(rowKey),
                    )
                  }
                />
              </Table.Th>
              <Table.Th>Type</Table.Th>
              <Table.Th>Name</Table.Th>
              <Table.Th>State</Table.Th>
              <Table.Th>Tags</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {filtered.map((row) => {
              const key = rowKey(row);
              const RC = ResourceComponents[row.type];
              return (
                <Table.Tr key={key}>
                  <Table.Td>
                    <Checkbox
                      aria-label={`Select ${row.name}`}
                      checked={selected.includes(key)}
                      onChange={() =>
                        setSelected((current) =>
                          current.includes(key)
                            ? current.filter((k) => k !== key)
                            : [...current, key],
                        )
                      }
                    />
                  </Table.Td>
                  <Table.Td>
                    <Group gap="xs" wrap="nowrap">
                      <RC.Icon size={14} />
                      <Text size="sm">{row.type}</Text>
                    </Group>
                  </Table.Td>
                  <Table.Td>
                    <ResourceLink type={row.type} id={row.id} />
                  </Table.Td>
                  <Table.Td>
                    <RC.State id={row.id} />
                  </Table.Td>
                  <Table.Td>
                    {/* Resource `tags` are tag IDs, not names. Rendering
                        them raw would show opaque hex. TableTags is what
                        every other table uses, so a tag looks the same
                        here as it does anywhere else. */}
                    <TableTags tagIds={row.tags} />
                  </Table.Td>
                </Table.Tr>
              );
            })}
          </Table.Tbody>
        </Table>
        {filtered.length === 0 && (
          <Text size="sm" c="dimmed" ta="center" py="xl">
            {rows.length === 0
              ? "No resources yet."
              : "No resources match these filters."}
          </Text>
        )}
      </Box>
    </Page>
  );
}

/**
 * The cross-type bulk action.
 *
 * Refresh is the right first one: it is read-only, so a mis-click across
 * nine resource types costs nothing, which is not true of anything that
 * deploys or destroys.
 *
 * Types with no refresh are named in the button's disabled state rather
 * than silently skipped - a bulk action that quietly does less than it
 * says is worse than one that refuses.
 */
function BulkRefresh({
  rows,
  onDone,
}: {
  rows: UnifiedRow[];
  onDone: () => void;
}) {
  const { batches, skipped } = useMemo(
    () => groupForRefresh(rows),
    [rows],
  );

  const refreshStack = useWrite("RefreshStackCache").mutateAsync;
  const refreshBuild = useWrite("RefreshBuildCache").mutateAsync;
  const refreshRepo = useWrite("RefreshRepoCache").mutateAsync;
  const refreshSync = useWrite("RefreshResourceSyncPending").mutateAsync;
  const [running, setRunning] = useState(false);

  const run = async () => {
    setRunning(true);
    try {
      for (const batch of batches) {
        for (const id of batch.ids) {
          if (batch.type === "Stack") await refreshStack({ stack: id });
          else if (batch.type === "Build") await refreshBuild({ build: id });
          else if (batch.type === "Repo") await refreshRepo({ repo: id });
          else if (batch.type === "ResourceSync")
            await refreshSync({ sync: id });
        }
      }
      onDone();
    } finally {
      setRunning(false);
    }
  };

  const refreshable = batches.reduce(
    (total, batch) => total + batch.ids.length,
    0,
  );

  return (
    <Button
      variant="light"
      leftSection={<RefreshCw size={14} />}
      disabled={refreshable === 0 || running}
      loading={running}
      onClick={run}
      title={
        skipped.length
          ? `${skipped.length} selected resource(s) have no refresh action`
          : undefined
      }
    >
      Refresh {refreshable > 0 ? `(${refreshable})` : ""}
    </Button>
  );
}
