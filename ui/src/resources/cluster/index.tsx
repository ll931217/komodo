import { clusterStateIntention } from "@/lib/color";
import { useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { RequiredResourceComponents } from "..";
import { Types } from "komodo_client";
import { HoverError, StatusBadge, hexColorByIntention } from "mogh_ui";
import { Box } from "@mantine/core";
import ClusterTable from "./table";
import ClusterConfig from "./config";
import NewResource from "@/resources/new";
import ResourceHeader from "../header";
import ResourceLink from "@/resources/link";
import BatchExecutions from "@/components/batch-executions";
import { DeployCluster, DestroyCluster } from "./executions";

export function useCluster(id: string | undefined, useName?: boolean) {
  return useRead("ListClusters", {}).data?.find((r) =>
    useName ? r.name === id : r.id === id,
  );
}

export function useFullCluster(id: string) {
  return useRead("GetCluster", { cluster: id }, { refetchInterval: 30_000 })
    .data;
}

export const ClusterComponents: RequiredResourceComponents<
  Types.ClusterConfig,
  Types.ClusterInfo,
  Types.ClusterListItemInfo
> = {
  useList: () => useRead("ListClusters", {}).data,
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
    DeployCluster,
    DestroyCluster,
  },

  Config: ClusterConfig,

  Page: {},
};
