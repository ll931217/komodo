import {
  ActionIcon,
  Button,
  Code,
  Group,
  Select,
  Stack,
  Text,
  Textarea,
} from "@mantine/core";
import { Types } from "komodo_client";
import { CircleMinus, Plus } from "lucide-react";
import { ALERT_TYPES } from "./alert-types";

export default function AlerterConfigTemplates({
  templates,
  set,
  disabled,
}: {
  templates: Types.AlertTemplate[];
  set: (templates: Types.AlertTemplate[]) => void;
  disabled: boolean;
}) {
  const update = (index: number, partial: Partial<Types.AlertTemplate>) =>
    set(templates.map((t, i) => (i === index ? { ...t, ...partial } : t)));

  return (
    <Stack gap="sm">
      <Text size="xs" c="dimmed">
        Overrides the message for one alert type. Anything without a template
        keeps the built-in format. <Code fz="xs">{"{{level}}"}</Code>,{" "}
        <Code fz="xs">{"{{name}}"}</Code>,{" "}
        <Code fz="xs">{"{{resource_type}}"}</Code>,{" "}
        <Code fz="xs">{"{{resource_id}}"}</Code> and any field of the alert's
        own data are substituted. A placeholder naming nothing is left as
        written, so a typo shows up in the message.
      </Text>
      {templates.map((template, index) => (
        <Group key={index} align="start" gap="sm" wrap="wrap">
          <Select
            label="Alert Type"
            data={ALERT_TYPES}
            value={template.alert_type}
            w={260}
            searchable
            allowDeselect={false}
            onChange={(alert_type) =>
              alert_type &&
              update(index, {
                alert_type: alert_type as Types.AlertData["type"],
              })
            }
            disabled={disabled}
          />
          <Textarea
            label="Message"
            placeholder="{{level}}: {{name}} is unreachable"
            value={template.template}
            w={380}
            autosize
            minRows={2}
            onChange={(e) => update(index, { template: e.target.value })}
            disabled={disabled}
          />
          <ActionIcon
            variant="subtle"
            color="red"
            mt="xl"
            onClick={() => set(templates.filter((_, i) => i !== index))}
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
            ...templates,
            {
              alert_type: ALERT_TYPES[0] as Types.AlertData["type"],
              template: "",
            },
          ])
        }
        disabled={disabled}
      >
        Add Template
      </Button>
    </Stack>
  );
}
