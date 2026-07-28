import { useSelectedResources } from "@/lib/hooks";
import ResourceLink from "@/resources/link";
import { DataTable, SortableHeader } from "mogh_ui";
import { Types } from "komodo_client";
import { ClusterComponents } from ".";
import TableTags from "@/components/tags/table";
import { BoxProps } from "@mantine/core";

export default function ClusterTable({
  resources,
  ...boxProps
}: {
  resources: Types.ClusterListItem[];
} & BoxProps) {
  const [_, setSelectedResources] = useSelectedResources("Cluster");

  return (
    <DataTable
      {...boxProps}
      tableKey="cluster-table"
      data={resources}
      selectOptions={{
        selectKey: ({ name }) => name,
        onSelect: setSelectedResources,
      }}
      columns={[
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="Name" />
          ),
          accessorKey: "name",
          cell: ({ row }) => (
            <ResourceLink type="Cluster" id={row.original.id} />
          ),
          size: 200,
        },
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="Server" />
          ),
          accessorKey: "info.server_id",
          cell: ({ row }) =>
            row.original.info.server_id ? (
              <ResourceLink type="Server" id={row.original.info.server_id} />
            ) : null,
          size: 200,
        },
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="State" />
          ),
          accessorKey: "info.state",
          cell: ({ row }) => <ClusterComponents.State id={row.original.id} />,
          size: 120,
        },
        {
          header: "Tags",
          cell: ({ row }) => <TableTags tagIds={row.original.tags} />,
        },
      ]}
    />
  );
}
