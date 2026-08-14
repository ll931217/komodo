import { applicationStateIntention } from "@/lib/color";
import { useListItem, useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { RequiredResourceComponents } from "..";
import { Types } from "komodo_client";
import { StatusBadge, hexColorByIntention } from "mogh_ui";
import { Box } from "@mantine/core";
import ApplicationTable from "./table";
import ApplicationConfig from "./config";
import NewResource from "@/resources/new";
import ResourceHeader from "../header";
import ResourceLink from "@/resources/link";
import BatchExecutions from "@/resources/batch-executions";
import {
  DeployApplication,
  DestroyApplication,
  DiffApplication,
} from "./executions";

export function useApplication(
  id: string | undefined,
  useName?: boolean,
  refetchInterval?: number | false,
) {
  return useListItem("Application", id, useName, refetchInterval);
}

export function useFullApplication(id: string) {
  return useRead("GetApplication", { application: id }).data;
}

export const ApplicationComponents: RequiredResourceComponents<
  Types.ApplicationConfig,
  Types.ApplicationInfo,
  Types.ApplicationListItemInfo,
  Types.ApplicationQuerySpecifics
> = {
  useList: (query, limit, page) =>
    useRead("ListApplications", { query, limit, page }).data,
  useListItem: useApplication,
  useFull: useFullApplication,

  useResourceLinks: (application) => application?.config?.links,

  useDashboardSummaryData: () => {
    const summary = useRead(
      "GetApplicationsSummary",
      {},
      { refetchInterval: 10_000 },
    ).data;
    return [
      {
        intention: "Good",
        value: summary?.deployed ?? 0,
        title: "Deployed",
      },
      {
        intention: "Warning",
        value: summary?.drifted ?? 0,
        title: "Drifted",
      },
      {
        intention: "Critical",
        value: summary?.failed ?? 0,
        title: "Failed",
      },
      { intention: "Unknown", value: summary?.unknown ?? 0, title: "Unknown" },
    ];
  },

  Description: () => <>Deploy manifests to a Cluster.</>,

  New: () => <NewResource type="Application" />,

  BatchExecutions: () => (
    <BatchExecutions
      type="Application"
      executions={[
        ["BatchDiffApplication", ICONS.UpdateAvailable],
        ["BatchDeployApplication", ICONS.Deploy],
        ["BatchDestroyApplication", ICONS.Destroy],
      ]}
    />
  ),

  Table: ApplicationTable,

  Icon: ({ id, size = "1rem", noColor }) => {
    const state = useApplication(id)?.info.state;
    const color = noColor
      ? undefined
      : state && hexColorByIntention(applicationStateIntention(state));
    return <ICONS.Application size={size} color={color} />;
  },

  ResourcePageHeader: ({ id }) => {
    const application = useApplication(id);
    return (
      <ResourceHeader
        type="Application"
        id={id}
        resource={application}
        intent={applicationStateIntention(application?.info.state)}
        icon={ICONS.Application}
        name={application?.name}
        state={application?.info.state}
      />
    );
  },

  State: ({ id }) => {
    const state = useApplication(id)?.info.state;
    return (
      <StatusBadge text={state} intent={applicationStateIntention(state)} />
    );
  },

  Info: {
    Cluster: ({ id }) => {
      const clusterId = useApplication(id)?.info.cluster_id;
      if (!clusterId) return null;
      return <ResourceLink type="Cluster" id={clusterId} />;
    },
    Namespace: ({ id }) => {
      const namespace = useApplication(id)?.info.namespace;
      if (!namespace) return null;
      return <Box>{namespace}</Box>;
    },
    Source: ({ id }) => {
      const sourceKind = useApplication(id)?.info.source_kind;
      if (!sourceKind) return null;
      return <Box>{sourceKind}</Box>;
    },
  },

  Executions: {
    DiffApplication,
    DeployApplication,
    DestroyApplication,
  },

  Config: ApplicationConfig,

  Page: {},
};
