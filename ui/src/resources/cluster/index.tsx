import { clusterStateIntention } from "@/lib/color";
import { useListItem, useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { RequiredResourceComponents } from "..";
import { Types } from "komodo_client";
import { HoverError, StatusBadge, hexColorByIntention } from "mogh_ui";
import { Box, Group } from "@mantine/core";
import ClusterTable from "./table";
import ClusterTabs from "./tabs";
import NewResource from "@/resources/new";
import ResourceHeader from "../header";
import ResourceLink from "@/resources/link";
import BatchExecutions from "@/resources/batch-executions";
import { DeployCluster, DestroyCluster, DiffCluster } from "./executions";
import ClusterHeaderInfo from "./header-info";

export function useCluster(
  id: string | undefined,
  useName?: boolean,
  refetchInterval?: number | false,
) {
  return useListItem("Cluster", id, useName, refetchInterval);
}

export function useFullCluster(id: string) {
  return useRead("GetCluster", { cluster: id }, { refetchInterval: 30_000 })
    .data;
}

export const ClusterComponents: RequiredResourceComponents<
  Types.ClusterConfig,
  Types.ClusterInfo,
  Types.ClusterListItemInfo,
  Types.ClusterQuerySpecifics
> = {
  useList: (query, limit, page) =>
    useRead("ListClusters", { query, limit, page }).data,
  useListItem: useCluster,
  useFull: useFullCluster,

  useResourceLinks: (cluster) => cluster?.config?.links,

  useDashboardSummaryData: () => {
    const summary = useRead(
      "GetClustersSummary",
      {},
      { refetchInterval: 10_000 },
    ).data;
    return [
      { intention: "Good", value: summary?.ok ?? 0, title: "Ok" },
      {
        intention: "Critical",
        value: summary?.unreachable ?? 0,
        title: "Unreachable",
      },
      { intention: "Unknown", value: summary?.unknown ?? 0, title: "Unknown" },
    ];
  },

  Description: () => <>Manage Kubernetes clusters.</>,

  New: () => <NewResource type="Cluster" />,

  BatchExecutions: () => (
    <BatchExecutions
      type="Cluster"
      executions={[
        ["DiffCluster", ICONS.UpdateAvailable],
        ["DeployCluster", ICONS.Deploy],
        ["DestroyCluster", ICONS.Destroy],
      ]}
    />
  ),

  Table: ClusterTable,

  Icon: ({ id, size = "1rem", noColor }) => {
    const state = useCluster(id)?.info.state;
    const color = noColor
      ? undefined
      : state && hexColorByIntention(clusterStateIntention(state));
    return <ICONS.Cluster size={size} color={color} />;
  },

  ResourcePageHeader: ({ id }) => {
    const cluster = useCluster(id);
    return (
      <ResourceHeader
        type="Cluster"
        id={id}
        resource={cluster}
        intent={clusterStateIntention(cluster?.info.state)}
        icon={ICONS.Cluster}
        name={cluster?.name}
        state={cluster?.info.state}
      />
    );
  },

  State: ({ id }) => {
    const state = useCluster(id)?.info.state;
    return <StatusBadge text={state} intent={clusterStateIntention(state)} />;
  },

  Info: {
    Server: ({ id }) => {
      const serverId = useCluster(id)?.info.server_id;
      if (!serverId) return null;
      return <ResourceLink type="Server" id={serverId} />;
    },
    // The reverse of ServerConfig.cluster_id: the Servers registered as
    // nodes of this Cluster. Distinct from the Nodes entry below, which
    // counts what kubectl reports - these are the ones Komodo manages.
    NodeServers: ({ id }) => {
      const servers = useRead("ListServers", {
        query: { specific: { clusters: [id] } },
      }).data;
      if (!servers?.length) return null;
      return (
        <Group gap="sm">
          {servers.map((server) => (
            <ResourceLink key={server.id} type="Server" id={server.id} />
          ))}
        </Group>
      );
    },
    Context: ({ id }) => {
      const cluster = useCluster(id);
      if (!cluster?.info.context) return null;
      return <Box>{cluster.info.context}</Box>;
    },
    Namespace: ({ id }) => {
      const cluster = useCluster(id);
      if (!cluster?.info.namespace) return null;
      return <Box>{cluster.info.namespace}</Box>;
    },
    Nodes: ({ id }) => (
      <ClusterHeaderInfo
        clusterId={id}
        kind="nodes"
        label="node"
        clusterScoped
      />
    ),
    Pods: ({ id }) => (
      <ClusterHeaderInfo clusterId={id} kind="pods" label="pod" />
    ),
    Deployments: ({ id }) => (
      <ClusterHeaderInfo
        clusterId={id}
        kind="deployments"
        label="deployment"
      />
    ),
    Services: ({ id }) => (
      <ClusterHeaderInfo clusterId={id} kind="services" label="service" />
    ),
    Err: ({ id }) => {
      const err = useCluster(id)?.info.err;
      if (!err) return null;
      return (
        <Box>
          <HoverError {...err} />
        </Box>
      );
    },
  },

  Executions: {
    DiffCluster,
    DeployCluster,
    DestroyCluster,
  },

  // The Config slot owns the tab container, as on Stack / Server /
  // Swarm: Config plus a Browser for the cluster's live objects.
  Config: ClusterTabs,

  Page: {},
};
