import { Alert, Code, List, Text } from "@mantine/core";
import { TriangleAlert } from "lucide-react";
import { useRead } from "@/lib/hooks";

/// Warns about live objects on a Server that no Komodo resource owns.
///
/// Reported, never pruned - `docker system prune` cannot tell a
/// deliberately hand-run container from litter, and neither can this.
export default function OrphansWarning({
  serverId,
}: {
  serverId: string;
}) {
  const orphans =
    useRead(
      "ListOrphanedObjects",
      { server: serverId },
      { refetchInterval: 30_000 },
    ).data ?? [];

  if (orphans.length === 0) return null;

  return (
    <Alert
      color="yellow"
      variant="light"
      icon={<TriangleAlert size="1.2rem" />}
      title={`${orphans.length} orphaned object${orphans.length === 1 ? "" : "s"} on this server`}
    >
      <List spacing={4} size="sm" listStyleType="none">
        {orphans.map((orphan) => (
          <List.Item key={`${orphan.kind}-${orphan.name}`}>
            <Code fz="sm">{orphan.name}</Code>{" "}
            <Text span size="xs" c="dimmed">
              ({orphan.kind})
            </Text>
            <Text size="xs" c="dimmed">
              {orphan.reason}
            </Text>
          </List.Item>
        ))}
      </List>
      <Text size="xs" c="dimmed" mt="xs">
        Nothing here is deleted automatically. Add names to the server's
        "Ignore Orphans" config to stop reporting the intentional ones.
      </Text>
    </Alert>
  );
}
