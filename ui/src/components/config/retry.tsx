import { Group, NumberInput, Stack, Switch, Text } from "@mantine/core";
import { Types } from "komodo_client";

export interface ConfigRetryProps {
  value?: Types.RetryConfig;
  disabled: boolean;
  set: (value: Types.RetryConfig) => void;
}

/// Mirrors the Rust `RetryConfig` defaults, so an unset policy reads
/// the same here as it does on the server. A blank field here would
/// otherwise look like "no wait" while the server used 30s.
const DEFAULTS: Types.RetryConfig = {
  enabled: false,
  limit: 2,
  delay_seconds: 30,
  factor: 2,
  max_delay_seconds: 600,
};

export default function ConfigRetry({
  value,
  disabled,
  set,
}: ConfigRetryProps) {
  const retry = { ...DEFAULTS, ...(value ?? {}) };
  const update = (partial: Partial<Types.RetryConfig>) =>
    set({ ...retry, ...partial });

  return (
    <Stack gap="sm">
      <Switch
        label="Retry failed runs"
        checked={retry.enabled}
        onChange={(e) => update({ enabled: e.currentTarget.checked })}
        disabled={disabled}
      />
      <Group align="start" gap="md" wrap="wrap">
        <NumberInput
          label="Retries"
          description="Attempts after the first failure"
          min={0}
          w={140}
          value={retry.limit}
          onChange={(limit) => update({ limit: Number(limit) || 0 })}
          disabled={disabled || !retry.enabled}
        />
        <NumberInput
          label="Delay (s)"
          description="Wait before the first retry"
          min={0}
          w={140}
          value={retry.delay_seconds}
          onChange={(delay_seconds) =>
            update({ delay_seconds: Number(delay_seconds) || 0 })
          }
          disabled={disabled || !retry.enabled}
        />
        <NumberInput
          label="Factor"
          description="Delay multiplier per attempt"
          min={1}
          step={0.5}
          decimalScale={2}
          w={140}
          value={retry.factor}
          onChange={(factor) => update({ factor: Number(factor) || 1 })}
          disabled={disabled || !retry.enabled}
        />
        <NumberInput
          label="Max delay (s)"
          description="Cap on the computed delay"
          min={0}
          w={140}
          value={retry.max_delay_seconds}
          onChange={(max_delay_seconds) =>
            update({ max_delay_seconds: Number(max_delay_seconds) || 0 })
          }
          disabled={disabled || !retry.enabled}
        />
      </Group>
      <Text size="xs" c="dimmed">
        Each retry runs as its own Update, so the history shows every attempt.
        Cancelled runs are never retried.
      </Text>
    </Stack>
  );
}
