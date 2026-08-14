import { useExecute } from "@/lib/hooks";
import { useTerraform } from ".";
import { ConfirmButton } from "mogh_ui";
import { ICONS } from "@/lib/icons";

export const PlanTerraform = ({ id }: { id: string }) => {
  const terraform = useTerraform(id);
  const { mutateAsync, isPending } = useExecute("PlanTerraform");

  // Nothing to run without a Server to run terraform on.
  if (!terraform || !terraform.info.server_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.UpdateAvailable size="1rem" />}
      onClick={() => mutateAsync({ terraform: id })}
      disabled={isPending}
      loading={isPending}
    >
      Plan
    </ConfirmButton>
  );
};

export const ApplyTerraform = ({ id }: { id: string }) => {
  const terraform = useTerraform(id);
  const { mutateAsync, isPending } = useExecute("ApplyTerraform");

  if (!terraform || !terraform.info.server_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.Deploy size="1rem" />}
      onClick={() => mutateAsync({ terraform: id })}
      disabled={isPending}
      loading={isPending}
    >
      Apply
    </ConfirmButton>
  );
};

export const DestroyTerraform = ({ id }: { id: string }) => {
  const terraform = useTerraform(id);
  const { mutateAsync, isPending } = useExecute("DestroyTerraform");

  if (!terraform || !terraform.info.server_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.Destroy size="1rem" />}
      onClick={() => mutateAsync({ terraform: id })}
      disabled={isPending}
      loading={isPending}
    >
      Destroy
    </ConfirmButton>
  );
};
