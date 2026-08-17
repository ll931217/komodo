import { terraformStateIntention } from "@/lib/color";
import { useListItem, useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { RequiredResourceComponents } from "..";
import { Types } from "komodo_client";
import { StatusBadge, hexColorByIntention } from "mogh_ui";
import { Box } from "@mantine/core";
import TerraformTable from "./table";
import TerraformConfig from "./config";
import NewResource from "@/resources/new";
import ResourceHeader from "../header";
import ResourceLink from "@/resources/link";
import BatchExecutions from "@/resources/batch-executions";
import {
  ApplyTerraform,
  DestroyTerraform,
  PlanTerraform,
} from "./executions";

export function useTerraform(
  id: string | undefined,
  useName?: boolean,
  refetchInterval?: number | false,
) {
  return useListItem("Terraform", id, useName, refetchInterval);
}

export function useFullTerraform(id: string) {
  return useRead("GetTerraform", { terraform: id }).data;
}

export const TerraformComponents: RequiredResourceComponents<
  Types.TerraformConfig,
  Types.TerraformInfo,
  Types.TerraformListItemInfo,
  Types.TerraformQuerySpecifics
> = {
  useList: (query, limit, page) =>
    useRead("ListTerraforms", { query, limit, page }).data,
  useListItem: useTerraform,
  useFull: useFullTerraform,

  useResourceLinks: (terraform) => terraform?.config?.links,

  useDashboardSummaryData: () => {
    const summary = useRead(
      "GetTerraformsSummary",
      {},
      { refetchInterval: 10_000 },
    ).data;
    return [
      { intention: "Good", value: summary?.ok ?? 0, title: "Ok" },
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

  Description: () => <>Run terraform on a Server.</>,

  New: ({ clusterId }) => (
    <NewResource<Types.TerraformConfig>
      type="Terraform"
      config={() => (clusterId ? { cluster_id: clusterId } : {})}
    />
  ),

  BatchExecutions: () => (
    <BatchExecutions
      type="Terraform"
      executions={[
        ["BatchPlanTerraform", ICONS.UpdateAvailable],
        ["BatchApplyTerraform", ICONS.Deploy],
        ["BatchDestroyTerraform", ICONS.Destroy],
      ]}
    />
  ),

  Table: TerraformTable,

  Icon: ({ id, size = "1rem", noColor }) => {
    const state = useTerraform(id)?.info.state;
    const color = noColor
      ? undefined
      : state && hexColorByIntention(terraformStateIntention(state));
    return <ICONS.Terraform size={size} color={color} />;
  },

  ResourcePageHeader: ({ id }) => {
    const terraform = useTerraform(id);
    return (
      <ResourceHeader
        type="Terraform"
        id={id}
        resource={terraform}
        intent={terraformStateIntention(terraform?.info.state)}
        icon={ICONS.Terraform}
        name={terraform?.name}
        state={terraform?.info.state}
      />
    );
  },

  State: ({ id }) => {
    const state = useTerraform(id)?.info.state;
    return <StatusBadge text={state} intent={terraformStateIntention(state)} />;
  },

  Info: {
    Server: ({ id }) => {
      const serverId = useTerraform(id)?.info.server_id;
      if (!serverId) return null;
      return <ResourceLink type="Server" id={serverId} />;
    },
    Source: ({ id }) => {
      const sourceKind = useTerraform(id)?.info.source_kind;
      if (!sourceKind) return null;
      return <Box>{sourceKind}</Box>;
    },
    Directory: ({ id }) => {
      const runDirectory = useTerraform(id)?.info.run_directory;
      if (!runDirectory) return null;
      return <Box>{runDirectory}</Box>;
    },
  },

  Executions: {
    PlanTerraform,
    ApplyTerraform,
    DestroyTerraform,
  },

  // Plan / apply / destroy output is a Types.Update, so it renders in the
  // generic update log viewer - there is no custom diff component.
  Config: TerraformConfig,

  Page: {},
};
