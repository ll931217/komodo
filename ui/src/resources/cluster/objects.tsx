import { useExecute, usePermissions, useRead } from "@/lib/hooks";
import { useCluster, useFullCluster } from ".";
import { ICONS } from "@/lib/icons";
import {
  Box,
  Button,
  Group,
  Modal,
  Select,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { DataTable, MonacoEditor, SortableHeader } from "mogh_ui";
import { useState } from "react";

/// Kinds offered in the selector. Kubernetes has hundreds and CRDs add
/// more, so the field stays free-text and these are only shortcuts.
const COMMON_KINDS = [
  "pods",
  "deployments",
  "replicasets",
  "statefulsets",
  "daemonsets",
  "services",
  "ingresses",
  "configmaps",
  "secrets",
  "jobs",
  "cronjobs",
  "persistentvolumeclaims",
  "serviceaccounts",
  "events",
  "namespaces",
  "nodes",
];

type ClusterObject = {
  name: string;
  namespace: string;
  raw: unknown;
};

/// Kubernetes objects are not modelled in Komodo: the API returns
/// whatever kubectl produced, so this pulls only the two fields every
/// object has and shows the rest as raw json.
function objectsFromListing(listing: unknown): ClusterObject[] {
  const items = (listing as { items?: unknown[] } | undefined)?.items;
  if (!Array.isArray(items)) return [];
  return items.map((item) => {
    const metadata =
      (item as { metadata?: { name?: string; namespace?: string } })
        .metadata ?? {};
    return {
      name: metadata.name ?? "",
      namespace: metadata.namespace ?? "",
      raw: item,
    };
  });
}

export default function ClusterObjects({ id }: { id: string }) {
  const cluster = useCluster(id);
  const config = useFullCluster(id)?.config;
  const { canExecute } = usePermissions({ type: "Cluster", id });

  const [kind, setKind] = useState("pods");
  const [namespace, setNamespace] = useState<string | null>(null);
  const [selected, setSelected] = useState<ClusterObject | null>(null);
  const [opened, { open, close }] = useDisclosure();

  const allowedNamespaces = config?.namespaces ?? [];
  const defaultNamespace = config?.namespace || "default";

  const { data, error, isFetching } = useRead(
    "ListClusterResources",
    { cluster: id, kind, namespace: namespace ?? undefined },
    { refetchInterval: 10_000, enabled: !!kind },
  );

  const { mutateAsync: deleteObject, isPending: deleting } =
    useExecute("DeleteClusterObject");

  if (!cluster) return null;

  const objects = objectsFromListing(data);

  return (
    <Stack gap="sm">
      <Group gap="sm" align="end">
        <Select
          label="Kind"
          data={COMMON_KINDS}
          value={kind}
          onChange={(value) => setKind(value ?? "pods")}
          searchable
          // Free-text so CRDs and less common kinds work too.
          allowDeselect={false}
          w={220}
        />
        {allowedNamespaces.length > 0 ? (
          <Select
            label="Namespace"
            description="Restricted by this Cluster"
            data={allowedNamespaces}
            value={namespace ?? defaultNamespace}
            onChange={setNamespace}
            w={220}
          />
        ) : (
          <TextInput
            label="Namespace"
            placeholder={defaultNamespace}
            value={namespace ?? ""}
            onChange={(e) =>
              setNamespace(e.currentTarget.value || null)
            }
            w={220}
          />
        )}
      </Group>

      {error ? (
        <Text c="red" size="sm">
          {String((error as { message?: string })?.message ?? error)}
        </Text>
      ) : null}

      <DataTable
        tableKey="cluster-objects"
        data={objects}
        columns={[
          {
            header: ({ column }) => (
              <SortableHeader column={column} title="Name" />
            ),
            accessorKey: "name",
            cell: ({ row }) => (
              <Button
                variant="subtle"
                size="compact-sm"
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
            header: "",
            id: "actions",
            cell: ({ row }) => (
              <Button
                variant="subtle"
                color="red"
                size="compact-sm"
                leftSection={<ICONS.Destroy size="0.9rem" />}
                disabled={!canExecute || deleting}
                onClick={() =>
                  deleteObject({
                    cluster: id,
                    kind,
                    name: row.original.name,
                    namespace: row.original.namespace || undefined,
                  })
                }
              >
                Delete
              </Button>
            ),
          },
        ]}
      />

      {isFetching && objects.length === 0 ? (
        <Text size="sm" c="dimmed">
          Loading {kind}...
        </Text>
      ) : null}

      <Modal
        opened={opened}
        onClose={close}
        size="xl"
        title={<Text fz="h3">{selected?.name}</Text>}
      >
        <Box h={600}>
          <MonacoEditor
            value={
              selected ? JSON.stringify(selected.raw, null, 2) : "{}"
            }
            language="json"
            readOnly
          />
        </Box>
      </Modal>
    </Stack>
  );
}
