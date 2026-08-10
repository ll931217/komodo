import { useRead, useResourceSelectionState } from "@/lib/hooks";
import ResourceLink from "@/resources/link";
import { DataTable, SortableHeader } from "mogh_ui";
import { Types } from "komodo_client";
import { useCallback } from "react";
import { ServerComponents } from "..";
import TableTags from "@/components/tags/table";
import { BoxProps } from "@mantine/core";
import {
  FieldFilter,
  useFieldFilters,
} from "@/components/table-field-filter";

const SORT_KEYS = ["Name", "Region", "Version", "State"];

export default function StandardServerTable({
  resources,
  onServerSort,
  ...boxProps
}: {
  resources: Types.ServerListItem[];
  /** When provided, sorting is handled server side,
   * and sort updates are passed to this callback. */
  onServerSort?: (sort: { sort_by?: string; sort_desc?: boolean }) => void;
} & BoxProps) {
  const selectionState = useResourceSelectionState("Server");
  const { filters, setFilter, filterRows } =
    useFieldFilters<Types.ServerListItem>({
      Name: (server) => server.name,
      Region: (server) => server.info.region,
      Cluster: (server) => server.info.node_name,
      Version: (server) => server.info.version,
      State: (server) => server.info.state,
    });
  const deployments = useRead("ListDeployments", { limit: 0 }).data;
  const stacks = useRead("ListStacks", { limit: 0 }).data;
  const repos = useRead("ListRepos", { limit: 0 }).data;
  const resourcesCount = useCallback(
    (id: string) => {
      return (
        (deployments?.filter((d) => d.info.server_id === id).length || 0) +
        (stacks?.filter((d) => d.info.server_id === id).length || 0) +
        (repos?.filter((d) => d.info.server_id === id).length || 0)
      );
    },
    [deployments, stacks, repos],
  );

  return (
    <DataTable
      {...boxProps}
      manualSorting={!!onServerSort}
      onSortingStateChange={
        onServerSort &&
        ((sorting) => {
          const sort = sorting.find((s) => SORT_KEYS.includes(s.id));
          onServerSort(sort ? { sort_by: sort.id, sort_desc: sort.desc } : {});
        })
      }
      tableKey="standard-server-table"
      data={filterRows(resources)}
      selectOptions={{
        selectKey: ({ name }) => name,
        state: selectionState,
      }}
      columns={[
        {
          size: 250,
          id: "Name",
          accessorKey: "name",
          header: ({ column }) => (
            <>
              <SortableHeader column={column} title="Name" />
              <FieldFilter id="Name" filters={filters} setFilter={setFilter} />
            </>
          ),
          cell: ({ row }) => (
            <ResourceLink type="Server" id={row.original.id} />
          ),
        },
        {
          size: 100,
          accessorKey: "id",
          // The resource count is computed on the client,
          // it cannot be sorted server side.
          enableSorting: !onServerSort,
          sortingFn: (a, b) => {
            const sa = resourcesCount(a.original.id);
            const sb = resourcesCount(b.original.id);

            if (!sa && !sb) return 0;
            if (!sa) return 1;
            if (!sb) return -1;

            if (sa > sb) return 1;
            else if (sa < sb) return -1;
            else return 0;
          },
          header: ({ column }) => (
            <SortableHeader column={column} title="Resources" />
          ),
          cell: ({ row }) => {
            return <>{resourcesCount(row.original.id)}</>;
          },
        },
        {
          size: 200,
          id: "Region",
          accessorKey: "info.region",
          header: ({ column }) => (
            <>
              <SortableHeader column={column} title="Region" />
              <FieldFilter
                id="Region"
                filters={filters}
                setFilter={setFilter}
              />
            </>
          ),
        },
        {
          size: 200,
          id: "Cluster",
          accessorKey: "info.node_name",
          enableSorting: false,
          header: () => (
            <>
              Cluster Node
              <FieldFilter
                id="Cluster"
                filters={filters}
                setFilter={setFilter}
              />
            </>
          ),
          cell: ({ row }) =>
            row.original.info.cluster_id ? (
              <ResourceLink
                type="Cluster"
                id={row.original.info.cluster_id}
              />
            ) : null,
        },
        {
          size: 150,
          id: "Version",
          accessorKey: "info.version",
          header: ({ column }) => (
            <>
              <SortableHeader column={column} title="Version" />
              <FieldFilter
                id="Version"
                filters={filters}
                setFilter={setFilter}
              />
            </>
          ),
          // cell: ({ row }) => <ServerVersion id={row.original.id} />,
        },
        {
          size: 150,
          id: "State",
          accessorKey: "info.state",
          header: ({ column }) => (
            <>
              <SortableHeader column={column} title="State" />
              <FieldFilter id="State" filters={filters} setFilter={setFilter} />
            </>
          ),
          cell: ({ row }) => <ServerComponents.State id={row.original.id} />,
        },
        {
          header: "Tags",
          cell: ({ row }) => <TableTags tagIds={row.original.tags} />,
        },
      ]}
    />
  );
}
