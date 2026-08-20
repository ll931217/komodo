import {
  ActionIcon,
  Button,
  Code,
  Group,
  Select,
  Stack,
  Switch,
  Text,
  TextInput,
} from "@mantine/core";
import { Types } from "komodo_client";
import { CircleMinus, Plus } from "lucide-react";

const LEVELS: Types.SeverityLevel[] = ["Ok", "Warning", "Critical"] as any;

export interface ConfigCustomAlertsProps {
  value?: Types.CustomAlert[];
  disabled: boolean;
  set: (value: Types.CustomAlert[]) => void;
}

export default function ConfigCustomAlerts({
  value,
  disabled,
  set,
}: ConfigCustomAlertsProps) {
  const alerts = value ?? [];
  const update = (index: number, partial: Partial<Types.CustomAlert>) =>
    set(alerts.map((a, i) => (i === index ? { ...a, ...partial } : a)));

  return (
    <Stack gap="sm">
      <Text size="xs" c="dimmed">
        Evaluated against the Server's live stats every monitoring cycle.
        Variables: <Code fz="xs">cpu_perc</Code>,{" "}
        <Code fz="xs">mem_perc</Code>, <Code fz="xs">mem_used_gb</Code>,{" "}
        <Code fz="xs">disk_perc</Code> (fullest disk),{" "}
        <Code fz="xs">swap_perc</Code>, <Code fz="xs">load_1</Code>,{" "}
        <Code fz="xs">load_5</Code>, <Code fz="xs">load_15</Code>,{" "}
        <Code fz="xs">containers</Code>,{" "}
        <Code fz="xs">containers_running</Code>,{" "}
        <Code fz="xs">network_ingress_bytes</Code>,{" "}
        <Code fz="xs">network_egress_bytes</Code>,{" "}
        <Code fz="xs">state</Code>. Use{" "}
        <Code fz="xs">&gt;</Code> / <Code fz="xs">&lt;</Code> with plain
        numbers; <Code fz="xs">==</Code> on a percentage needs a decimal
        point.
      </Text>
      {alerts.map((alert, index) => (
        <Group key={index} align="start" gap="sm" wrap="wrap">
          <TextInput
            label="Name"
            placeholder="swap thrashing"
            value={alert.name}
            w={180}
            onChange={(e) => update(index, { name: e.target.value })}
            disabled={disabled}
          />
          <TextInput
            label="Expression"
            placeholder="mem_perc > 80 && swap_used_gb > 1"
            value={alert.expression}
            w={320}
            onChange={(e) => update(index, { expression: e.target.value })}
            disabled={disabled}
          />
          <Select
            label="Level"
            data={LEVELS}
            value={alert.level}
            w={130}
            allowDeselect={false}
            onChange={(level) =>
              level && update(index, { level: level as Types.SeverityLevel })
            }
            disabled={disabled}
          />
          <Switch
            label="Enabled"
            mt="xl"
            checked={alert.enabled}
            onChange={(e) =>
              update(index, { enabled: e.currentTarget.checked })
            }
            disabled={disabled}
          />
          <ActionIcon
            variant="subtle"
            color="red"
            mt="xl"
            onClick={() => set(alerts.filter((_, i) => i !== index))}
            disabled={disabled}
          >
            <CircleMinus size="1rem" />
          </ActionIcon>
        </Group>
      ))}
      <Button
        variant="light"
        w="fit-content"
        leftSection={<Plus size="1rem" />}
        onClick={() =>
          set([
            ...alerts,
            {
              name: "",
              expression: "",
              level: "Warning" as Types.SeverityLevel,
              enabled: true,
            },
          ])
        }
        disabled={disabled}
      >
        Add Condition
      </Button>
    </Stack>
  );
}
