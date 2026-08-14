import { useResourceSelectionState } from "@/lib/hooks";
import ResourceLink from "@/resources/link";
import { DataTable, SortableHeader } from "mogh_ui";
import { Types } from "komodo_client";
import { TerraformComponents } from ".";
import TableTags from "@/components/tags/table";
import { BoxProps } from "@mantine/core";

const SORT_KEYS = ["Name", "State"];

export default function TerraformTable({
  resources,
  onServerSort,
  ...boxProps
}: {
  resources: Types.TerraformListItem[];
  /** When provided, sorting is handled server side,
   * and sort updates are passed to this callback. */
  onServerSort?: (sort: {
    sort_by?: string;
    sort_desc?: boolean;
  }) => void;
} & BoxProps) {
  const selectionState = useResourceSelectionState("Terraform");

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
      tableKey="terraform-table"
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
            <ResourceLink type="Terraform" id={row.original.id} />
          ),
          size: 200,
        },
        {
          header: "Server",
          accessorKey: "info.server_id",
          cell: ({ row }) =>
            row.original.info.server_id ? (
              <ResourceLink type="Server" id={row.original.info.server_id} />
            ) : null,
          size: 200,
        },
        {
          header: "Directory",
          accessorKey: "info.run_directory",
          size: 200,
        },
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="State" />
          ),
          id: "State",
          accessorKey: "info.state",
          cell: ({ row }) => <TerraformComponents.State id={row.original.id} />,
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
