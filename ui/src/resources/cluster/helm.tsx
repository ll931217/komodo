import { useExecute, usePermissions, useRead } from "@/lib/hooks";
import { useFullCluster } from ".";
import { useClusterObjectsSearch } from "./objects";
import { ICONS } from "@/lib/icons";
import { Box, Button, Group, Modal, Select, Stack, Text } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import {
  ColorIntention,
  ConfirmButton,
  DataTable,
  MonacoEditor,
  Section,
  SearchInput,
  SortableHeader,
  StatusBadge,
  filterBySplit,
} from "mogh_ui";
import { useState } from "react";

/// One row of `helm list -o json`.
type HelmRelease = {
  name: string;
  namespace: string;
  revision: string;
  updated: string;
  status: string;
  chart: string;
  app_version: string;
};

function releaseIntention(status?: string): ColorIntention {
  switch (status) {
    case "deployed":
      return "Good";
    case "failed":
      return "Critical";
    case undefined:
    case "unknown":
      return "None";
    // pending-install / pending-upgrade / pending-rollback /
    // uninstalling / superseded
    default:
      return "Warning";
  }
}

export default function ClusterHelm({
  id,
}: {
  id: string;
}) {
  const config = useFullCluster(id)?.config;
  const { canExecute } = usePermissions({ type: "Cluster", id });
  const [search, setSearch] = useClusterObjectsSearch();
  const [selected, setSelected] = useState<HelmRelease | null>(null);
  const [opened, { open, close }] = useDisclosure();

  const allowedNamespaces = config?.namespaces ?? [];
  // Core rejects all_namespaces on a namespace-restricted Cluster.
  const allNamespaces = allowedNamespaces.length === 0;
  const [namespace, setNamespace] = useState<string | null>(null);

  const { data, error } = useRead(
    "ListHelmReleases",
    {
      cluster: id,
      namespace: allNamespaces ? undefined : (namespace ?? undefined),
      all_namespaces: allNamespaces,
    },
    { refetchInterval: 30_000, retry: false },
  );

  const { mutateAsync: rollback, isPending: rollingBack } = useExecute(
    "RollbackHelmRelease",
  );
  const { mutateAsync: uninstall, isPending: uninstalling } = useExecute(
    "UninstallHelmRelease",
  );

  const releases = filterBySplit(
    (Array.isArray(data) ? data : []) as HelmRelease[],
    search,
    (release) => release.name,
  );

  return (
    <Section mb="md">
      <Stack gap="sm">
        {/* Search sits in the filter row, not the section header: the
            header here has no title of its own (the Cluster page owns
            it), so a lone control there renders as an empty band above
            the table. */}
        <Group gap="sm" align="end">
          {allowedNamespaces.length > 0 ? (
            <Select
              label="Namespace"
              description="Restricted by this Cluster"
              data={allowedNamespaces}
              value={namespace ?? (config?.namespace || "default")}
              onChange={setNamespace}
              w={220}
            />
          ) : null}
          <SearchInput value={search} onSearch={setSearch} w={220} />
        </Group>

        {error ? (
          <Text c="red" size="sm">
            Failed to list releases. Is helm available on the Server?
          </Text>
        ) : null}

        <DataTable
          tableKey="cluster-helm"
          data={releases}
          tableProps={{ verticalSpacing: 4, fz: "sm" }}
          columns={[
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Name" />
              ),
              accessorKey: "name",
              cell: ({ row }) => (
                <Button
                  variant="subtle"
                  size="compact-xs"
                  fz="sm"
                  onClick={() => {
                    setSelected(row.original);
                    open();
                  }}
                >
                  {row.original.name}
                </Button>
              ),
            },
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Namespace" />
              ),
              accessorKey: "namespace",
            },
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Status" />
              ),
              accessorKey: "status",
              cell: ({ row }) => (
                <StatusBadge
                  text={row.original.status}
                  intent={releaseIntention(row.original.status)}
                />
              ),
            },
            {
              header: "Revision",
              accessorKey: "revision",
            },
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Chart" />
              ),
              accessorKey: "chart",
            },
            {
              header: "App Version",
              accessorKey: "app_version",
            },
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Updated" />
              ),
              accessorKey: "updated",
              cell: ({ row }) => (
                <Text size="sm" c="dimmed">
                  {/* helm prints sub-second precision + tz, trim it */}
                  {row.original.updated?.split(".")[0]}
                </Text>
              ),
            },
            {
              header: "",
              id: "actions",
              cell: ({ row }) => (
                <Group gap={2} wrap="nowrap" justify="end">
                  <ConfirmButton
                    variant="subtle"
                    size="compact-xs"
                    px={4}
                    title="Rollback to previous revision"
                    disabled={!canExecute || rollingBack}
                    icon={<ICONS.History size="0.9rem" />}
                    onClick={() =>
                      rollback({
                        cluster: id,
                        name: row.original.name,
                        namespace: row.original.namespace,
                      })
                    }
                  />
                  <ConfirmButton
                    variant="subtle"
                    color="red"
                    size="compact-xs"
                    px={4}
                    title="Uninstall"
                    disabled={!canExecute || uninstalling}
                    icon={<ICONS.Destroy size="0.9rem" />}
                    onClick={() =>
                      uninstall({
                        cluster: id,
                        name: row.original.name,
                        namespace: row.original.namespace,
                      })
                    }
                  />
                </Group>
              ),
            },
          ]}
        />

        <Modal
          opened={opened}
          onClose={close}
          size="xl"
          title={
            <Group gap="xs">
              <Text fz="h3">{selected?.name}</Text>
              {selected?.namespace ? (
                <Text c="dimmed">{selected.namespace}</Text>
              ) : null}
            </Group>
          }
        >
          {selected ? (
            <ReleaseDetails
              key={selected.name + selected.namespace}
              cluster={id}
              release={selected}
            />
          ) : null}
        </Modal>
      </Stack>
    </Section>
  );
}

/// Revision history and user-supplied values for one release.
function ReleaseDetails({
  cluster,
  release,
}: {
  cluster: string;
  release: HelmRelease;
}) {
  const { data, isPending } = useRead("InspectHelmRelease", {
    cluster,
    name: release.name,
    namespace: release.namespace,
  });
  const inspect = data as
    | {
        history?: Array<{
          revision: number;
          updated: string;
          status: string;
          chart: string;
          app_version: string;
          description: string;
        }>;
        values?: unknown;
      }
    | undefined;

  if (isPending) {
    return (
      <Text size="sm" c="dimmed">
        Loading release...
      </Text>
    );
  }

  return (
    <Stack gap="sm">
      <Text fw={500}>History</Text>
      <DataTable
        tableKey="cluster-helm-history"
        data={inspect?.history ?? []}
        tableProps={{ verticalSpacing: 4, fz: "sm" }}
        columns={[
          { header: "Revision", accessorKey: "revision" },
          {
            header: "Status",
            accessorKey: "status",
            cell: ({ row }) => (
              <StatusBadge
                text={row.original.status}
                intent={releaseIntention(row.original.status)}
              />
            ),
          },
          { header: "Chart", accessorKey: "chart" },
          { header: "Description", accessorKey: "description" },
          {
            header: "Updated",
            accessorKey: "updated",
            cell: ({ row }) => (
              <Text size="sm" c="dimmed">
                {row.original.updated?.split(".")[0]}
              </Text>
            ),
          },
        ]}
      />
      <Text fw={500}>Values</Text>
      <Box h={320}>
        <MonacoEditor
          value={JSON.stringify(inspect?.values ?? null, null, 2)}
          language="json"
          readOnly
        />
      </Box>
    </Stack>
  );
}
