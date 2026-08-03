import { useExecute, usePermissions, useRead } from "@/lib/hooks";
import { useCluster, useFullCluster } from ".";
import { ICONS } from "@/lib/icons";
import {
  Autocomplete,
  Box,
  Button,
  Group,
  Modal,
  NumberInput,
  Popover,
  Select,
  Stack,
  Switch,
  Text,
} from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import {
  ColorIntention,
  ConfirmButton,
  DataTable,
  MonacoEditor,
  Section,
  SortableHeader,
  StatusBadge,
} from "mogh_ui";
import { ReactNode, useMemo, useState } from "react";
import { Link } from "react-router-dom";

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

export type ClusterObject = {
  name: string;
  namespace: string;
  /// `kubectl get`-style columns, present when the kind carries them.
  ready?: string;
  status?: string;
  restarts?: number;
  node?: string;
  created?: string;
  raw: unknown;
};

/// Kubernetes objects are not modelled in Komodo: the API returns
/// whatever kubectl produced, so this pulls the fields the standard
/// `kubectl get` printers show and keeps the rest as raw json.
export function objectsFromListing(listing: unknown): ClusterObject[] {
  const items = (listing as { items?: unknown[] } | undefined)?.items;
  if (!Array.isArray(items)) return [];
  return items.map((item) => {
    // kubectl output, structurally unknown by design.
    const { metadata, status, spec } = (item ?? {}) as any;
    const object: ClusterObject = {
      name: metadata?.name ?? "",
      namespace: metadata?.namespace ?? "",
      // Pods carry the node they were scheduled onto.
      node: spec?.nodeName,
      created: metadata?.creationTimestamp,
      raw: item,
    };

    const containers: any[] = status?.containerStatuses ?? [];
    if (containers.length > 0 || status?.phase) {
      // Pod: READY x/y, RESTARTS, and the waiting reason
      // (CrashLoopBackOff, ImagePullBackOff) over the blander phase.
      if (containers.length > 0) {
        object.ready = `${containers.filter((c) => c.ready).length}/${containers.length}`;
        object.restarts = containers.reduce(
          (sum, c) => sum + (c.restartCount ?? 0),
          0,
        );
      }
      object.status =
        containers.find((c) => c.state?.waiting?.reason)?.state.waiting
          .reason ??
        status?.reason ??
        status?.phase;
    } else if (
      typeof status?.readyReplicas === "number" ||
      typeof status?.replicas === "number" ||
      typeof spec?.replicas === "number"
    ) {
      // Deployments / statefulsets / replicasets.
      object.ready = `${status?.readyReplicas ?? 0}/${spec?.replicas ?? status?.replicas ?? 0}`;
    } else if (typeof status?.numberReady === "number") {
      // Daemonsets.
      object.ready = `${status.numberReady}/${status.desiredNumberScheduled ?? 0}`;
    } else if (Array.isArray(status?.conditions)) {
      // Nodes and anything else exposing a Ready condition.
      const ready = status.conditions.find((c: any) => c.type === "Ready");
      if (ready) {
        object.status = ready.status === "True" ? "Ready" : "NotReady";
      }
    }
    return object;
  });
}

/// Compact kubectl-style age: 45s, 12m, 3h, 5d.
function age(created?: string): string {
  if (!created) return "";
  const seconds = (Date.now() - new Date(created).getTime()) / 1000;
  if (seconds < 0) return "";
  if (seconds < 120) return `${Math.floor(seconds)}s`;
  const minutes = seconds / 60;
  if (minutes < 120) return `${Math.floor(minutes)}m`;
  const hours = minutes / 60;
  if (hours < 48) return `${Math.floor(hours)}h`;
  return `${Math.floor(hours / 24)}d`;
}

export function statusIntention(status?: string): ColorIntention {
  switch (status) {
    case "Running":
    case "Succeeded":
    case "Completed":
    case "Ready":
    case "Active":
    case "Bound":
      return "Good";
    case "Pending":
    case "ContainerCreating":
    case "PodInitializing":
    case "Terminating":
      return "Warning";
    case undefined:
    case "Unknown":
      return "None";
    // Failed / CrashLoopBackOff / ImagePullBackOff / Evicted / ...
    default:
      return "Critical";
  }
}

/// Read requests reject with `{ status, result: { error, trace } }` and
/// no `message`, so stringifying the error itself yields
/// "[object Object]". kubectl's own stderr lands in `trace`, while
/// `error` is Core's generic wrapper, so both are shown.
function readErrorMessage(error: unknown): string {
  const result = (error as { result?: { error?: string; trace?: string[] } })
    ?.result;
  const parts = [result?.error, ...(result?.trace ?? [])]
    .map((part) => part?.trim())
    .filter(Boolean);
  return parts.length
    ? parts.join(" | ")
    : "Failed to list resources. See console.";
}

export default function ClusterObjects({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  const cluster = useCluster(id);
  const config = useFullCluster(id)?.config;
  const { canExecute, canWrite } = usePermissions({ type: "Cluster", id });

  const [kind, setKind] = useState("pods");
  const [namespace, setNamespace] = useState<string | null>(null);
  const [allNamespaces, setAllNamespaces] = useState(false);
  const [selected, setSelected] = useState<ClusterObject | null>(null);
  const [opened, { open, close }] = useDisclosure();

  const allowedNamespaces = config?.namespaces ?? [];
  const defaultNamespace = config?.namespace || "default";
  // Core rejects all_namespaces on a namespace-restricted Cluster.
  const allNamespacesAllowed = allowedNamespaces.length === 0;
  const acrossNamespaces = allNamespaces && allNamespacesAllowed;

  const { data, error, isFetching } = useRead(
    "ListClusterResources",
    {
      cluster: id,
      kind,
      namespace: acrossNamespaces ? undefined : (namespace ?? undefined),
      all_namespaces: acrossNamespaces,
    },
    { refetchInterval: 10_000, enabled: !!kind },
  );

  // Live namespaces for the selector. Cluster-scoped reads can be
  // disabled on the Cluster, so a failure just means no suggestions
  // and the field stays free-text.
  const { data: namespacesListing } = useRead(
    "ListClusterResources",
    { cluster: id, kind: "namespaces" },
    { refetchInterval: 60_000, retry: false, enabled: allNamespacesAllowed },
  );
  const namespaceOptions = objectsFromListing(namespacesListing).map(
    (o) => o.name,
  );

  const { mutateAsync: deleteObject, isPending: deleting } = useExecute(
    "DeleteClusterObject",
  );
  const { mutateAsync: restartWorkload } = useExecute("RestartClusterWorkload");
  const { mutateAsync: rollbackWorkload } = useExecute(
    "RollbackClusterWorkload",
  );
  const { mutateAsync: cordonNode } = useExecute("CordonClusterNode");
  const { mutateAsync: uncordonNode } = useExecute("UncordonClusterNode");
  const { mutateAsync: drainNode } = useExecute("DrainClusterNode");

  if (!cluster) return null;

  const objects = objectsFromListing(data);

  return (
    <Section titleOther={titleOther} mb="md">
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
            <Autocomplete
              label="Namespace"
              placeholder={defaultNamespace}
              data={namespaceOptions}
              value={namespace ?? ""}
              onChange={(value) => setNamespace(value || null)}
              disabled={acrossNamespaces}
              w={220}
            />
          )}
          {allNamespacesAllowed ? (
            <Switch
              label="All namespaces"
              checked={allNamespaces}
              onChange={(e) => setAllNamespaces(e.currentTarget.checked)}
              pb={6}
            />
          ) : null}
        </Group>

        {error ? (
          <Text c="red" size="sm">
            {readErrorMessage(error)}
          </Text>
        ) : null}

        <DataTable
          tableKey="cluster-objects"
          data={objects}
          tableProps={{ verticalSpacing: 4, fz: "sm" }}
          columns={[
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Name" />
              ),
              accessorKey: "name",
              cell: ({ row }) =>
                // Pods get a full sub-page (logs / inspect / shell);
                // other kinds open the raw json modal.
                (row.original.raw as any)?.kind === "Pod" ? (
                  <Text
                    className="hover-underline"
                    size="sm"
                    fw={500}
                    renderRoot={(props) => (
                      <Link
                        to={`/clusters/${id}/pod/${encodeURIComponent(
                          row.original.namespace,
                        )}/${encodeURIComponent(row.original.name)}`}
                        {...props}
                      />
                    )}
                  >
                    {row.original.name}
                  </Text>
                ) : (
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
              size: 160,
            },
            // kubectl-get columns, shown only when the listed kind
            // actually carries them.
            objects.some((o) => o.ready !== undefined) && {
              header: ({ column }) => (
                <SortableHeader column={column} title="Ready" />
              ),
              accessorKey: "ready",
              size: 90,
            },
            objects.some((o) => o.status !== undefined) && {
              header: ({ column }) => (
                <SortableHeader column={column} title="Status" />
              ),
              accessorKey: "status",
              size: 150,
              cell: ({ row }) => (
                <StatusBadge
                  text={row.original.status}
                  intent={statusIntention(row.original.status)}
                />
              ),
            },
            objects.some((o) => o.restarts !== undefined) && {
              header: ({ column }) => (
                <SortableHeader column={column} title="Restarts" />
              ),
              accessorKey: "restarts",
              size: 100,
              cell: ({ row }) =>
                row.original.restarts ? (
                  <Text c="orange" size="sm" fw={500}>
                    {row.original.restarts}
                  </Text>
                ) : (
                  <Text c="dimmed" size="sm">
                    0
                  </Text>
                ),
            },
            objects.some((o) => o.node !== undefined) && {
              header: ({ column }) => (
                <SortableHeader column={column} title="Node" />
              ),
              accessorKey: "node",
              size: 160,
              cell: ({ row }) => (
                <Text size="sm" c="dimmed">
                  {row.original.node}
                </Text>
              ),
            },
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Age" />
              ),
              accessorKey: "created",
              size: 80,
              sortDescFirst: true,
              cell: ({ row }) => (
                <Text size="sm" c="dimmed">
                  {age(row.original.created)}
                </Text>
              ),
            },
            {
              header: "",
              id: "actions",
              size: 170,
              cell: ({ row }) => {
                const rawKind = (row.original.raw as any)?.kind as
                  string | undefined;
                const params = {
                  cluster: id,
                  kind,
                  name: row.original.name,
                  namespace: row.original.namespace || undefined,
                };
                const unschedulable = !!(row.original.raw as any)?.spec
                  ?.unschedulable;
                return (
                  <Group gap={2} wrap="nowrap" justify="end">
                    {rawKind && ROLLOUTABLE_KINDS.includes(rawKind) && (
                      <>
                        <Button
                          variant="subtle"
                          size="compact-xs"
                          px={4}
                          title="Rolling restart"
                          disabled={!canExecute}
                          onClick={() => restartWorkload(params)}
                        >
                          <ICONS.Restart size="0.9rem" />
                        </Button>
                        <Button
                          variant="subtle"
                          size="compact-xs"
                          px={4}
                          title="Rollback to previous revision"
                          disabled={!canExecute}
                          onClick={() => rollbackWorkload(params)}
                        >
                          <ICONS.History size="0.9rem" />
                        </Button>
                      </>
                    )}
                    {rawKind && SCALABLE_KINDS.includes(rawKind) && (
                      <ScaleControl
                        cluster={id}
                        kind={kind}
                        object={row.original}
                        disabled={!canExecute}
                      />
                    )}
                    {rawKind === "Node" && (
                      <>
                        <Button
                          variant="subtle"
                          size="compact-xs"
                          px={4}
                          title={
                            unschedulable
                              ? "Uncordon (allow scheduling)"
                              : "Cordon (mark unschedulable)"
                          }
                          disabled={!canExecute}
                          onClick={() =>
                            (unschedulable ? uncordonNode : cordonNode)({
                              cluster: id,
                              node: row.original.name,
                            })
                          }
                        >
                          {unschedulable ? (
                            <ICONS.Start size="0.9rem" />
                          ) : (
                            <ICONS.Cancel size="0.9rem" />
                          )}
                        </Button>
                        <ConfirmButton
                          variant="subtle"
                          color="red"
                          size="compact-xs"
                          px={4}
                          title="Drain (cordon + evict pods)"
                          disabled={!canExecute}
                          icon={<ICONS.Prune size="0.9rem" />}
                          onClick={() =>
                            drainNode({
                              cluster: id,
                              node: row.original.name,
                              force: false,
                              delete_emptydir_data: false,
                            })
                          }
                        />
                      </>
                    )}
                    <Button
                      variant="subtle"
                      color="red"
                      size="compact-xs"
                      px={4}
                      title="Delete"
                      disabled={!canExecute || deleting}
                      onClick={() => deleteObject(params)}
                    >
                      <ICONS.Destroy size="0.9rem" />
                    </Button>
                  </Group>
                );
              },
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
            <ObjectEditor
              key={selected.name + selected.namespace}
              cluster={id}
              object={selected}
              canWrite={canWrite}
            />
          ) : null}
        </Modal>
      </Stack>
    </Section>
  );
}

/// Kinds `kubectl rollout restart|undo` accepts, as `kind:` appears in
/// the object json.
const ROLLOUTABLE_KINDS = ["Deployment", "StatefulSet", "DaemonSet"];
/// Kinds `kubectl scale` accepts.
const SCALABLE_KINDS = ["Deployment", "StatefulSet", "ReplicaSet"];

/// Replica input behind a popover, so a stray click can't scale
/// something to a surprise number.
function ScaleControl({
  cluster,
  kind,
  object,
  disabled,
}: {
  cluster: string;
  kind: string;
  object: ClusterObject;
  disabled: boolean;
}) {
  const current: number = (object.raw as any)?.spec?.replicas ?? 1;
  const [replicas, setReplicas] = useState<string | number>(current);
  const [opened, { toggle, close }] = useDisclosure();
  const { mutateAsync: scaleWorkload, isPending } = useExecute(
    "ScaleClusterWorkload",
  );

  return (
    <Popover opened={opened} onDismiss={close} position="bottom" withArrow>
      <Popover.Target>
        <Button
          variant="subtle"
          size="compact-xs"
          px={4}
          title="Scale replicas"
          disabled={disabled}
          onClick={toggle}
        >
          <ICONS.UpdateAvailable size="0.9rem" />
        </Button>
      </Popover.Target>
      <Popover.Dropdown>
        <Group gap="xs" align="end">
          <NumberInput
            label="Replicas"
            value={replicas}
            onChange={setReplicas}
            min={0}
            w={90}
            size="xs"
          />
          <Button
            size="xs"
            loading={isPending}
            onClick={async () => {
              await scaleWorkload({
                cluster,
                kind,
                name: object.name,
                replicas: Number(replicas),
                namespace: object.namespace || undefined,
              });
              close();
            }}
          >
            Scale
          </Button>
        </Group>
      </Popover.Dropdown>
    </Popover>
  );
}

/// The raw object, editable when the user can write the Cluster.
/// Apply round-trips the edited json through `kubectl apply -f`.
function ObjectEditor({
  cluster,
  object,
  canWrite,
}: {
  cluster: string;
  object: ClusterObject;
  canWrite: boolean;
}) {
  // Server bookkeeping never reaches the editor: managedFields is
  // huge and non-editable, and a stale resourceVersion makes apply
  // fail with a conflict as soon as a controller touches the object
  // (deployment status updates bump it constantly).
  const initial = useMemo(() => {
    const raw = structuredClone(object.raw) as any;
    if (raw?.metadata) {
      delete raw.metadata.managedFields;
      delete raw.metadata.resourceVersion;
    }
    return JSON.stringify(raw, null, 2);
  }, [object.raw]);
  const [contents, setContents] = useState(initial);
  const { mutateAsync: applyObject, isPending } =
    useExecute("ApplyClusterObject");

  return (
    <Stack gap="xs">
      {canWrite ? (
        <Group justify="end">
          <ConfirmButton
            size="compact-sm"
            icon={<ICONS.Save size="0.9rem" />}
            disabled={contents === initial || isPending}
            onClick={() =>
              applyObject({
                cluster,
                contents,
                namespace: object.namespace || undefined,
              })
            }
          >
            Apply
          </ConfirmButton>
        </Group>
      ) : null}
      <Box h={560}>
        <MonacoEditor
          value={contents}
          onValueChange={setContents}
          language="json"
          readOnly={!canWrite}
        />
      </Box>
    </Stack>
  );
}
