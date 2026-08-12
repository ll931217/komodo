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
  Text,
} from "@mantine/core";
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
import { ReactNode, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { atom, useAtom } from "jotai";
import { FieldFilter, useFieldFilters } from "@/components/table-field-filter";
import { Types } from "komodo_client";

/// Shared across the kind tabs, as on the Swarm docker tabs.
const searchAtom = atom("");
export function useClusterObjectsSearch() {
  return useAtom(searchAtom);
}

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
  /// `kubectl get events`-style columns.
  reason?: string;
  object?: string;
  message?: string;
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

    if ((item as any)?.kind === "Event") {
      // `kubectl get events` columns: LAST SEEN, TYPE, REASON,
      // OBJECT, MESSAGE. The metadata name is machine noise, so the
      // involved object stands in for it.
      const event = item as any;
      object.status = event.type;
      object.reason = event.reason;
      object.object = event.involvedObject
        ? `${event.involvedObject.kind}/${event.involvedObject.name}`
        : undefined;
      object.message = event.message;
      object.created = event.lastTimestamp ?? event.eventTime ?? object.created;
      return object;
    }

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
    // Event type
    case "Normal":
      return "Good";
    case "Pending":
    case "ContainerCreating":
    case "PodInitializing":
    case "Terminating":
    // Event type
    case "Warning":
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

/// Kinds kubectl does not namespace, so the namespace controls
/// would only mislead.
const CLUSTER_SCOPED_KINDS = ["nodes", "namespaces"];

export default function ClusterObjects({
  id,
  kind: fixedKind,
  titleOther,
}: {
  id: string;
  /// When set, the kind is pinned by the tab and the selector hides.
  kind?: string;
  titleOther?: ReactNode;
}) {
  const cluster = useCluster(id);
  const config = useFullCluster(id)?.config;
  const { canExecute, canWrite } = usePermissions({ type: "Cluster", id });

  const [search, setSearch] = useClusterObjectsSearch();
  const [selectedKind, setKind] = useState("pods");
  const kind = fixedKind ?? selectedKind;
  const namespaced = !CLUSTER_SCOPED_KINDS.includes(kind);
  // undefined = untouched, so the Cluster's Default Namespace seeds it
  // once the config loads. "" is a deliberate clear = every namespace.
  const [namespace, setNamespace] = useState<string | undefined>(undefined);
  const [selected, setSelected] = useState<ClusterObject | null>(null);
  const [opened, { open, close }] = useDisclosure();

  const allowedNamespaces = config?.namespaces ?? [];
  // Core rejects all_namespaces on a namespace-restricted Cluster.
  const allNamespacesAllowed = allowedNamespaces.length === 0;
  const selectedNamespace = allNamespacesAllowed
    ? (namespace ?? config?.namespace ?? "")
    : (namespace ?? config?.namespace) || allowedNamespaces[0];
  // Empty namespace box = every namespace. Cluster-scoped kinds have no
  // namespace to scope by, so they always read across.
  const acrossNamespaces =
    allNamespacesAllowed && (!namespaced || !selectedNamespace);
  const queryNamespace =
    namespaced && !acrossNamespaces
      ? selectedNamespace || undefined
      : undefined;

  const { data, error, isFetching } = useRead(
    "ListClusterResources",
    {
      cluster: id,
      kind,
      namespace: queryNamespace,
      all_namespaces: acrossNamespaces,
    },
    { refetchInterval: 10_000, enabled: !!kind },
  );

  // `kubectl top` usage joined onto pods / nodes rows by name.
  // No metrics-server on the cluster just means no usage columns.
  const metricsKind =
    kind === "pods"
      ? Types.ClusterMetricsKind.Pods
      : kind === "nodes"
        ? Types.ClusterMetricsKind.Nodes
        : undefined;
  const { data: metrics } = useRead(
    "GetClusterMetrics",
    {
      cluster: id,
      kind: metricsKind!,
      namespace: queryNamespace,
      all_namespaces: acrossNamespaces,
    },
    { refetchInterval: 30_000, retry: false, enabled: !!metricsKind },
  );
  const metricsFor = (object: ClusterObject) =>
    metrics?.find(
      (m) =>
        m.name === object.name &&
        (kind === "nodes" || m.namespace === object.namespace),
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
  // Per-column narrowing, on top of the name search above. Hooks must
  // run before the early return below.
  const fieldFilters = useFieldFilters<ClusterObject>({
    Name: (object) => object.name,
    Namespace: (object) => object.namespace,
    Status: (object) => object.status,
    Reason: (object) => object.reason,
    Node: (object) => object.node,
  });

  if (!cluster) return null;

  const objects = fieldFilters.filterRows(
    filterBySplit(objectsFromListing(data), search, (object) => object.name),
  );

  return (
    <Section
      titleOther={titleOther}
      actions={<SearchInput value={search} onSearch={setSearch} />}
      mb="md"
    >
      <Stack gap="sm">
        <Group gap="sm" align="end">
          {fixedKind ? null : (
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
          )}
          {!namespaced ? null : allowedNamespaces.length > 0 ? (
            <Select
              label="Namespace"
              description="Restricted by this Cluster"
              data={allowedNamespaces}
              value={selectedNamespace}
              onChange={(value) => setNamespace(value ?? undefined)}
              w={220}
            />
          ) : (
            <Autocomplete
              label="Namespace"
              description="Leave empty for all namespaces"
              placeholder="All namespaces"
              data={namespaceOptions}
              value={selectedNamespace}
              onChange={setNamespace}
              w={220}
            />
          )}
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
                <>
                  <SortableHeader column={column} title="Name" />
                  <FieldFilter
                    id="Name"
                    filters={fieldFilters.filters}
                    setFilter={fieldFilters.setFilter}
                  />
                </>
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
                <>
                  <SortableHeader column={column} title="Namespace" />
                  <FieldFilter
                    id="Namespace"
                    filters={fieldFilters.filters}
                    setFilter={fieldFilters.setFilter}
                  />
                </>
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
                <>
                  <SortableHeader column={column} title="Status" />
                  <FieldFilter
                    id="Status"
                    filters={fieldFilters.filters}
                    setFilter={fieldFilters.setFilter}
                  />
                </>
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
            // Event columns, as `kubectl get events` prints them.
            objects.some((o) => o.reason !== undefined) && {
              header: ({ column }) => (
                <>
                  <SortableHeader column={column} title="Reason" />
                  <FieldFilter
                    id="Reason"
                    filters={fieldFilters.filters}
                    setFilter={fieldFilters.setFilter}
                  />
                </>
              ),
              accessorKey: "reason",
              size: 160,
            },
            objects.some((o) => o.object !== undefined) && {
              header: ({ column }) => (
                <SortableHeader column={column} title="Object" />
              ),
              accessorKey: "object",
              size: 220,
              cell: ({ row }) => (
                <Text size="sm" c="dimmed">
                  {row.original.object}
                </Text>
              ),
            },
            objects.some((o) => o.message !== undefined) && {
              header: "Message",
              accessorKey: "message",
              size: 400,
              cell: ({ row }) => (
                <Text size="sm" lineClamp={2} title={row.original.message}>
                  {row.original.message}
                </Text>
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
            // `kubectl top` usage, present when metrics-server answers.
            !!metrics?.length && {
              header: "CPU",
              id: "cpu",
              size: 110,
              cell: ({ row }: { row: { original: ClusterObject } }) => {
                const usage = metricsFor(row.original);
                if (!usage) return null;
                return (
                  <Text size="sm">
                    {usage.cpu}
                    {usage.cpu_percent ? (
                      <Text span size="sm" c="dimmed">
                        {` (${usage.cpu_percent})`}
                      </Text>
                    ) : null}
                  </Text>
                );
              },
            },
            !!metrics?.length && {
              header: "Memory",
              id: "memory",
              size: 130,
              cell: ({ row }: { row: { original: ClusterObject } }) => {
                const usage = metricsFor(row.original);
                if (!usage) return null;
                return (
                  <Text size="sm">
                    {usage.memory}
                    {usage.memory_percent ? (
                      <Text span size="sm" c="dimmed">
                        {` (${usage.memory_percent})`}
                      </Text>
                    ) : null}
                  </Text>
                );
              },
            },
            objects.some((o) => o.node !== undefined) && {
              header: ({ column }) => (
                <>
                  <SortableHeader column={column} title="Node" />
                  <FieldFilter
                    id="Node"
                    filters={fieldFilters.filters}
                    setFilter={fieldFilters.setFilter}
                  />
                </>
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
