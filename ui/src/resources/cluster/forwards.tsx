import { useExecute, usePermissions, useRead } from "@/lib/hooks";
import { useFullCluster } from ".";
import { objectsFromListing } from "./objects";
import { ICONS } from "@/lib/icons";
import {
  Autocomplete,
  Button,
  Group,
  NumberInput,
  Popover,
  Select,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import {
  ConfirmButton,
  DataTable,
  Section,
  SortableHeader,
  StatusBadge,
} from "mogh_ui";
import { ReactNode, useState } from "react";

/// `kubectl port-forward` sessions on the Cluster's Server.
/// The forward binds on the Server, not the browser: reach it from
/// machines that can reach the Server.
export default function ClusterForwards({
  id,
  titleOther,
}: {
  id: string;
  titleOther: ReactNode;
}) {
  const { canExecute } = usePermissions({ type: "Cluster", id });

  const { data: forwards, refetch } = useRead(
    "ListClusterPortForwards",
    { cluster: id },
    { refetchInterval: 10_000, retry: false },
  );

  const { mutateAsync: deleteForward } = useExecute(
    "DeleteClusterPortForward",
  );

  return (
    <Section
      titleOther={titleOther}
      actions={
        <NewForward id={id} disabled={!canExecute} onCreated={refetch} />
      }
      mb="md"
    >
      <Stack gap="sm">
        <Text size="sm" c="dimmed">
          Forwards listen on the Cluster's Server, not your machine.
        </Text>
        <DataTable
          tableKey="cluster-forwards"
          data={forwards ?? []}
          tableProps={{ verticalSpacing: 4, fz: "sm" }}
          columns={[
            {
              header: ({ column }) => (
                <SortableHeader column={column} title="Name" />
              ),
              accessorKey: "name",
            },
            {
              header: "Resource",
              accessorKey: "resource",
            },
            {
              header: "Namespace",
              accessorKey: "namespace",
            },
            {
              header: "Listen",
              cell: ({ row }) => (
                <Text size="sm">
                  {row.original.address}:{row.original.local_port}
                </Text>
              ),
            },
            {
              header: "Remote Port",
              accessorKey: "remote_port",
            },
            {
              header: "State",
              cell: ({ row }) => (
                <StatusBadge
                  text={row.original.alive ? "Running" : "Dead"}
                  intent={row.original.alive ? "Good" : "Critical"}
                />
              ),
            },
            {
              header: "",
              id: "actions",
              cell: ({ row }) => (
                <Group justify="end">
                  <ConfirmButton
                    variant="subtle"
                    color="red"
                    size="compact-xs"
                    px={4}
                    title="Stop forward"
                    disabled={!canExecute}
                    icon={<ICONS.Destroy size="0.9rem" />}
                    onClick={async () => {
                      await deleteForward({
                        cluster: id,
                        name: row.original.name,
                      });
                      refetch();
                    }}
                  />
                </Group>
              ),
            },
          ]}
        />
      </Stack>
    </Section>
  );
}

function NewForward({
  id,
  disabled,
  onCreated,
}: {
  id: string;
  disabled: boolean;
  onCreated: () => void;
}) {
  const [opened, { toggle, close }] = useDisclosure();
  const [name, setName] = useState("");
  const [resource, setResource] = useState("");
  const [namespace, setNamespace] = useState("");
  const [localPort, setLocalPort] = useState<string | number>("");
  const [remotePort, setRemotePort] = useState<string | number>("");
  const [address, setAddress] = useState("");
  const { mutateAsync: createForward, isPending } = useExecute(
    "CreateClusterPortForward",
  );

  const config = useFullCluster(id)?.config;
  const allowedNamespaces = config?.namespaces ?? [];
  const defaultNamespace = config?.namespace || "default";

  // Live suggestions, fetched only while the popover is open. Any of
  // these reads can be denied on the Cluster: a failure just means no
  // suggestions and the field stays free-text (same as objects.tsx).
  const { data: namespacesListing } = useRead(
    "ListClusterResources",
    { cluster: id, kind: "namespaces" },
    { retry: false, enabled: opened && allowedNamespaces.length === 0 },
  );
  const namespaceOptions = objectsFromListing(namespacesListing).map(
    (o) => o.name,
  );
  const resourceParams = {
    cluster: id,
    namespace: namespace || undefined,
  };
  const { data: podsListing } = useRead(
    "ListClusterResources",
    { ...resourceParams, kind: "pods" },
    { retry: false, enabled: opened },
  );
  const { data: servicesListing } = useRead(
    "ListClusterResources",
    { ...resourceParams, kind: "services" },
    { retry: false, enabled: opened },
  );
  const resourceOptions = [
    ...objectsFromListing(podsListing).map((o) => `pod/${o.name}`),
    ...objectsFromListing(servicesListing).map((o) => `service/${o.name}`),
  ];

  const incomplete = !name || !resource || !localPort || !remotePort;

  return (
    <Popover opened={opened} onDismiss={close} position="bottom-end">
      <Popover.Target>
        <Button
          size="compact-sm"
          leftSection={<ICONS.Add size="1rem" />}
          disabled={disabled}
          onClick={toggle}
        >
          New Forward
        </Button>
      </Popover.Target>
      <Popover.Dropdown>
        <Stack gap="xs" w={280}>
          <TextInput
            label="Name"
            placeholder="my-forward"
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            size="xs"
          />
          {allowedNamespaces.length > 0 ? (
            <Select
              label="Namespace"
              description="Restricted by this Cluster"
              data={allowedNamespaces}
              value={namespace || defaultNamespace}
              onChange={(value) => setNamespace(value ?? "")}
              size="xs"
              comboboxProps={{ withinPortal: false }}
            />
          ) : (
            <Autocomplete
              label="Namespace"
              placeholder={defaultNamespace}
              data={namespaceOptions}
              value={namespace}
              onChange={setNamespace}
              size="xs"
              comboboxProps={{ withinPortal: false }}
            />
          )}
          <Autocomplete
            label="Resource"
            placeholder="pod/api-0 or service/api"
            data={resourceOptions}
            value={resource}
            onChange={setResource}
            size="xs"
            comboboxProps={{ withinPortal: false }}
          />
          <Group grow>
            <NumberInput
              label="Server port"
              value={localPort}
              onChange={setLocalPort}
              min={1}
              max={65535}
              size="xs"
            />
            <NumberInput
              label="Remote port"
              value={remotePort}
              onChange={setRemotePort}
              min={1}
              max={65535}
              size="xs"
            />
          </Group>
          <TextInput
            label="Address"
            description="0.0.0.0 exposes to the Server's network"
            placeholder="127.0.0.1"
            value={address}
            onChange={(e) => setAddress(e.currentTarget.value)}
            size="xs"
          />
          <Button
            size="xs"
            loading={isPending}
            disabled={incomplete}
            onClick={async () => {
              await createForward({
                cluster: id,
                name,
                resource,
                namespace: namespace || undefined,
                local_port: Number(localPort),
                remote_port: Number(remotePort),
                address: address || undefined,
              });
              onCreated();
              close();
            }}
          >
            Start
          </Button>
        </Stack>
      </Popover.Dropdown>
    </Popover>
  );
}
