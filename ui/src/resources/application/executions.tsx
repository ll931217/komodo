import { useExecute } from "@/lib/hooks";
import { useApplication } from ".";
import { ConfirmButton } from "mogh_ui";
import { ICONS } from "@/lib/icons";

export const DeployApplication = ({ id }: { id: string }) => {
  const application = useApplication(id);
  const { mutateAsync, isPending } = useExecute("DeployApplication");

  // Nothing to deploy to without a Cluster.
  if (!application || !application.info.cluster_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.Deploy size="1rem" />}
      onClick={() => mutateAsync({ application: id })}
      disabled={isPending}
      loading={isPending}
    >
      Deploy
    </ConfirmButton>
  );
};

export const DestroyApplication = ({ id }: { id: string }) => {
  const application = useApplication(id);
  const { mutateAsync, isPending } = useExecute("DestroyApplication");

  if (!application || !application.info.cluster_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.Destroy size="1rem" />}
      onClick={() => mutateAsync({ application: id })}
      disabled={isPending}
      loading={isPending}
    >
      Destroy
    </ConfirmButton>
  );
};

export const DiffApplication = ({ id }: { id: string }) => {
  const application = useApplication(id);
  const { mutateAsync, isPending } = useExecute("DiffApplication");

  if (!application || !application.info.cluster_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.UpdateAvailable size="1rem" />}
      onClick={() => mutateAsync({ application: id })}
      disabled={isPending}
      loading={isPending}
    >
      Diff
    </ConfirmButton>
  );
};
