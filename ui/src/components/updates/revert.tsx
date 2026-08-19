/**
 * The Revert action on a historical update.
 *
 * Shows the snapshot this update can be reverted TO, or says why it
 * cannot be. It does not apply anything: the backend deliberately
 * returns a plan rather than mutating, and the apply goes through the
 * normal sync flow, which diffs before applying and records its own
 * Update. So a revert stays auditable and itself revertible.
 *
 * When a revert is NOT possible the control stays visible and disabled
 * with the reason attached, rather than disappearing. A missing button
 * teaches an operator nothing; "this update recorded no config
 * snapshot" teaches them which updates can be reverted at all.
 */
import { useState } from "react";
import { Alert, Button, Code, Group, Stack, Text } from "@mantine/core";
import { Undo2 } from "lucide-react";
import { sendCopyNotification } from "mogh_ui";

import { useRead } from "@/lib/hooks";

export default function RevertAction({
  updateId,
}: {
  updateId: string;
}) {
  const [shown, setShown] = useState(false);
  const plan = useRead("GetUpdateRevertToml", {
    update: updateId,
  }).data;

  if (!plan) return null;

  if (!plan.revertable) {
    return (
      <Group gap="xs" align="center" wrap="wrap">
        <Button
          size="xs"
          variant="light"
          leftSection={<Undo2 size={14} />}
          disabled
        >
          Revert
        </Button>
        <Text size="xs" c="dimmed" style={{ flex: 1, minWidth: 200 }}>
          {plan.reason}
        </Text>
      </Group>
    );
  }

  return (
    <Stack gap="xs">
      <Group gap="xs">
        <Button
          size="xs"
          variant="light"
          leftSection={<Undo2 size={14} />}
          onClick={() => setShown((current) => !current)}
        >
          {shown ? "Hide revert config" : "Revert"}
        </Button>
        {shown && (
          <Button
            size="xs"
            variant="subtle"
            onClick={() => {
              navigator.clipboard.writeText(plan.toml);
              sendCopyNotification("Revert config");
            }}
          >
            Copy
          </Button>
        )}
      </Group>
      {shown && (
        <Stack gap="xs">
          <Alert color="yellow" title="Applying this is a sync">
            <Text size="sm">
              This is the config as it was before this update. Komodo
              does not apply it directly - put it through a Resource
              Sync, which shows the diff before changing anything and
              records the revert as its own update.
            </Text>
          </Alert>
          <Code block style={{ maxHeight: 320, overflow: "auto" }}>
            {plan.toml}
          </Code>
        </Stack>
      )}
    </Stack>
  );
}
