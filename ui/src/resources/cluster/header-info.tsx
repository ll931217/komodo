import { useRead } from "@/lib/hooks";
import { StatusBadge } from "mogh_ui";
import { Box, Group, HoverCard, Loader, Stack, Text } from "@mantine/core";
import { Link } from "react-router-dom";
import { useFullCluster } from ".";
import { objectsFromListing, statusIntention } from "./objects";

/// Mirrors the Swarm header counts: `<n> pods` with a hover card
/// listing them and their state.
export default function ClusterHeaderInfo({
  clusterId,
  kind,
  label,
  clusterScoped,
}: {
  clusterId: string;
  /// `kubectl get` kind, eg. "pods".
  kind: string;
  /// Singular noun shown next to the count.
  label: string;
  clusterScoped?: boolean;
}) {
  const config = useFullCluster(clusterId)?.config;
  // Core rejects all_namespaces on a namespace-restricted Cluster.
  const allNamespaces = !clusterScoped && (config?.namespaces ?? []).length === 0;

  const { data, isError } = useRead(
    "ListClusterResources",
    {
      cluster: clusterId,
      kind,
      all_namespaces: allNamespaces,
    },
    { refetchInterval: 30_000, retry: false },
  );

  // Cluster-scoped reads and namespaces can both be locked down on
  // the Cluster, and a count that can't be read isn't worth a slot.
  if (isError) return null;

  const objects = data === undefined ? undefined : objectsFromListing(data);

  return (
    <Box>
      <HoverCard position="bottom-start">
        <HoverCard.Target>
          <Text>
            {objects ? (
              <>
                <b>{objects.length}</b>{" "}
                {`${label}${objects.length === 1 ? "" : "s"}`}
              </>
            ) : (
              <Loader size="xs" />
            )}
          </Text>
        </HoverCard.Target>
        <HoverCard.Dropdown mah="50vh" style={{ overflowY: "auto" }}>
          <Stack gap="xs">
            {objects?.map((object) => (
              <Group
                key={`${object.namespace}/${object.name}`}
                justify="space-between"
                className="bordered-light"
                p="sm"
                bdrs="sm"
                gap="lg"
              >
                {kind === "pods" ? (
                  <Text
                    className="hover-underline"
                    size="sm"
                    fw={500}
                    renderRoot={(props) => (
                      <Link
                        to={`/clusters/${clusterId}/pod/${encodeURIComponent(
                          object.namespace,
                        )}/${encodeURIComponent(object.name)}`}
                        {...props}
                      />
                    )}
                  >
                    {object.name}
                  </Text>
                ) : (
                  <Text size="sm" fw={500}>
                    {object.name}
                  </Text>
                )}
                <StatusBadge
                  text={object.status ?? object.ready}
                  intent={statusIntention(object.status)}
                />
              </Group>
            ))}
          </Stack>
        </HoverCard.Dropdown>
      </HoverCard>
    </Box>
  );
}
