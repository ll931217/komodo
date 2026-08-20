import { Stack, Text } from "@mantine/core";
import { Types } from "komodo_client";
import ConfigMaintenanceWindows from "@/components/maintenance-windows";

export interface ConfigExecutionWindowsProps {
  value?: Types.ExecutionWindows;
  disabled: boolean;
  set: (value: Types.ExecutionWindows) => void;
}

export default function ConfigExecutionWindows({
  value,
  disabled,
  set,
}: ConfigExecutionWindowsProps) {
  const allow = value?.allow ?? [];
  const deny = value?.deny ?? [];

  return (
    <Stack gap="lg">
      <Text size="xs" c="dimmed">
        Deny wins: a run inside a deny window is refused even if an allow
        window also matches. Automated runs (schedules, webhooks, sync-driven
        deploys) can never override a closed window; a human admin can.
      </Text>
      <Stack gap="xs">
        <Text fw={500} size="sm">
          Allow windows
        </Text>
        <Text size="xs" c="dimmed">
          When set, runs are only allowed inside these windows. Leave empty for
          no restriction.
        </Text>
        <ConfigMaintenanceWindows
          windows={allow}
          onUpdate={(allow) => set({ allow, deny })}
          disabled={disabled}
        />
      </Stack>
      <Stack gap="xs">
        <Text fw={500} size="sm">
          Deny windows
        </Text>
        <Text size="xs" c="dimmed">
          Runs are refused inside these windows — a deploy freeze.
        </Text>
        <ConfigMaintenanceWindows
          windows={deny}
          onUpdate={(deny) => set({ allow, deny })}
          disabled={disabled}
        />
      </Stack>
    </Stack>
  );
}
