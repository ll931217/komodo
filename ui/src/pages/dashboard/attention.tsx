import {
  applicationStateIntention,
  clusterStateIntention,
  terraformStateIntention,
} from "@/lib/color";
import { useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import ResourceLink from "@/resources/link";
import { Types } from "komodo_client";
import {
  DataTable,
  hexColorByIntention,
  Section,
  SortableHeader,
  StatusBadge,
} from "mogh_ui";

/**
 * Drift and failures across the Cluster / Application / Terraform
 * fleet, on the Dashboard.
 *
 * The `Active` section above answers "what is running right now", which
 * these three have no state for: an Application or Terraform run is
 * synchronous and leaves only its outcome behind, and drift is only
 * discovered by a Diff / Plan. So the question worth a Dashboard slot is
 * "what no longer matches its configuration", which otherwise requires
 * opening every resource page to find out.
 *
 * Filtered client side, unlike `Active`: none of these three carry a
 * `states` filter in their QuerySpecifics the way Stack and Deployment
 * do, and the lists are small enough that adding one to Core (entity +
 * AddFilters + gen-client) buys nothing here.
 */
export default function DashboardNeedsAttention() {
  const query = { limit: 0 } as const;
  const options = { refetchInterval: 30_000 } as const;
  const applications =
    useRead("ListApplications", query, options).data ?? [];
  const terraforms = useRead("ListTerraforms", query, options).data ?? [];
  const clusters = useRead("ListClusters", query, options).data ?? [];

  const rows = [
    ...applications
      .filter(({ info }) =>
        [
          Types.ApplicationState.Drifted,
          Types.ApplicationState.Failed,
        ].includes(info.state),
      )
      .map(({ id, info }) => ({
        type: "Application" as const,
        id,
        state: info.state as string,
        intent: applicationStateIntention(info.state),
      })),
    ...terraforms
      .filter(({ info }) =>
        [
          Types.TerraformState.Drifted,
          Types.TerraformState.Failed,
        ].includes(info.state),
      )
      .map(({ id, info }) => ({
        type: "Terraform" as const,
        id,
        state: info.state as string,
        intent: terraformStateIntention(info.state),
      })),
    ...clusters
      .filter(({ info }) => info.state === Types.ClusterState.Unreachable)
      .map(({ id, info }) => ({
        type: "Cluster" as const,
        id,
        state: info.state as string,
        intent: clusterStateIntention(info.state),
      })),
    // Critical before Warning: a failed run needs a human, drift may
    // well be someone else's deploy in progress.
  ].sort((a, b) => (a.intent === b.intent ? 0 : a.intent === "Critical" ? -1 : 1));

  if (rows.length === 0) return null;

  return (
    <Section
      title="Needs attention"
      mb="xl"
      icon={
        <ICONS.Alert size="1.1rem" color={hexColorByIntention("Warning")} />
      }
    >
      <DataTable
        tableKey="dashboard-needs-attention"
        data={rows}
        columns={[
          {
            id: "Name",
            header: ({ column }) => (
              <SortableHeader column={column} title="Name" />
            ),
            cell: ({ row }) => (
              <ResourceLink type={row.original.type} id={row.original.id} />
            ),
          },
          {
            accessorKey: "type",
            header: ({ column }) => (
              <SortableHeader column={column} title="Resource" />
            ),
          },
          {
            accessorKey: "state",
            header: ({ column }) => (
              <SortableHeader column={column} title="State" />
            ),
            cell: ({ row }) => (
              <StatusBadge
                text={row.original.state}
                intent={row.original.intent}
              />
            ),
          },
        ]}
      />
    </Section>
  );
}
