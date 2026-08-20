import { useNavigate } from "react-router-dom";
import { useState } from "react";
import { Checkbox, Stack, Text } from "@mantine/core";
import { UsableResource } from ".";
import { usePermissions, useRead, useWrite } from "@/lib/hooks";
import { usableResourcePath } from "@/lib/utils";
import { ConfirmModal } from "mogh_ui";
import { ICONS } from "@/lib/icons";

export default function DeleteResource({
  type,
  id,
}: {
  type: UsableResource;
  id: string;
}) {
  const nav = useNavigate();
  // Only these two can destroy what they deployed on the way out.
  const cascadable = type === "Stack" || type === "Deployment";
  const [cascade, setCascade] = useState(false);
  const key = type === "ResourceSync" ? "sync" : type.toLowerCase();
  const { canWrite } = usePermissions({ type, id });
  const resource = useRead(`Get${type}`, {
    [key]: id,
  } as any).data;
  const { mutateAsync, isPending } = useWrite(`Delete${type}`, {
    onSuccess: () => nav(`/${usableResourcePath(type)}`),
  });

  if (!resource || !canWrite) return null;

  return (
    <ConfirmModal
      title={
        <>
          Confirm <b>Delete</b>
        </>
      }
      confirmButtonContent="Delete"
      icon={<ICONS.Delete size="1rem" />}
      targetNoIcon
      targetProps={{ w: "fit", px: "xs" }}
      confirmText={resource.name}
      onConfirm={() =>
        mutateAsync(cascadable ? ({ id, cascade } as any) : { id })
      }
      topAdditonal={
        cascadable ? (
          <Stack gap={4}>
            <Checkbox
              label={`Also destroy the ${type === "Stack" ? "containers" : "container"} it deployed`}
              checked={cascade}
              onChange={(e) => setCascade(e.currentTarget.checked)}
            />
            <Text size="xs" c="dimmed">
              Off by default: deleting a {type} normally leaves what it
              deployed running, which is what makes an accidental delete
              recoverable.
            </Text>
          </Stack>
        ) : undefined
      }
      loading={isPending}
      confirmProps={{ variant: "filled", color: "red" }}
    >
      <ICONS.Delete size="1.3rem" />
    </ConfirmModal>
  );
}
