import { useExecute } from "@/lib/hooks";
import { useCluster } from ".";
import { ConfirmButton } from "mogh_ui";
import { ICONS } from "@/lib/icons";

export const DeployCluster = ({ id }: { id: string }) => {
  const cluster = useCluster(id);
  const { mutateAsync, isPending } = useExecute("DeployCluster");

  // Nothing to apply to without a Server to run kubectl on.
  if (!cluster || !cluster.info.server_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.Deploy size="1rem" />}
      onClick={() => mutateAsync({ cluster: id })}
      disabled={isPending}
      loading={isPending}
    >
      Deploy
    </ConfirmButton>
  );
};

export const DestroyCluster = ({ id }: { id: string }) => {
  const cluster = useCluster(id);
  const { mutateAsync, isPending } = useExecute("DestroyCluster");

  if (!cluster || !cluster.info.server_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.Destroy size="1rem" />}
      onClick={() => mutateAsync({ cluster: id })}
      disabled={isPending}
      loading={isPending}
    >
      Destroy
    </ConfirmButton>
  );
};

export const DiffCluster = ({ id }: { id: string }) => {
  const cluster = useCluster(id);
  const { mutateAsync, isPending } = useExecute("DiffCluster");

  if (!cluster || !cluster.info.server_id) {
    return null;
  }

  return (
    <ConfirmButton
      icon={<ICONS.UpdateAvailable size="1rem" />}
      onClick={() => mutateAsync({ cluster: id })}
      disabled={isPending}
      loading={isPending}
    >
      Diff
    </ConfirmButton>
  );
};
