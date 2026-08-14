import { useResourceSelectionState } from "@/lib/hooks";
import ResourceLink from "@/resources/link";
import { DataTable, SortableHeader } from "mogh_ui";
import { Types } from "komodo_client";
import { ApplicationComponents } from ".";
import TableTags from "@/components/tags/table";
import { BoxProps } from "@mantine/core";

const SORT_KEYS = ["Name", "State"];

export default function ApplicationTable({
  resources,
  onServerSort,
  ...boxProps
}: {
  resources: Types.ApplicationListItem[];
  /** When provided, sorting is handled server side,
   * and sort updates are passed to this callback. */
  onServerSort?: (sort: {
    sort_by?: string;
    sort_desc?: boolean;
  }) => void;
} & BoxProps) {
  const selectionState = useResourceSelectionState("Application");

  return (
    <DataTable
      {...boxProps}
      manualSorting={!!onServerSort}
      onSortingStateChange={
        onServerSort &&
        ((sorting) => {
          const sort = sorting.find((s) => SORT_KEYS.includes(s.id));
          onServerSort(
            sort ? { sort_by: sort.id, sort_desc: sort.desc } : {},
          );
        })
      }
      tableKey="application-table"
      data={resources}
      selectOptions={{
        selectKey: ({ name }) => name,
        state: selectionState,
      }}
      columns={[
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="Name" />
          ),
          id: "Name",
          accessorKey: "name",
          cell: ({ row }) => (
            <ResourceLink type="Application" id={row.original.id} />
          ),
          size: 200,
        },
        {
          header: "Cluster",
          accessorKey: "info.cluster_id",
          cell: ({ row }) =>
            row.original.info.cluster_id ? (
              <ResourceLink
                type="Cluster"
                id={row.original.info.cluster_id}
              />
            ) : null,
          size: 200,
        },
        {
          header: "Namespace",
          accessorKey: "info.namespace",
          size: 160,
        },
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="State" />
          ),
          id: "State",
          accessorKey: "info.state",
          cell: ({ row }) => (
            <ApplicationComponents.State id={row.original.id} />
          ),
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
