import { Types } from "komodo_client";
import {
  useExecute,
  useInvalidate,
  useIsCancelling,
  useRead,
} from "@/lib/hooks";
import { useApplication } from ".";
import { ConfirmButton } from "mogh_ui";
import { ICONS } from "@/lib/icons";
import { EXECUTION_ACTION_STATE_REQUERY_MS } from "@/lib/utils";

/**
 * Whether kubectl is doing anything for this Application right now.
 *
 * All three states share one Cancel because they share one command:
 * cancelling sends CancelExecution for whichever request is in flight
 * and the host kills that process group.
 */
function useApplicationRunning(id: string) {
  const state = useRead(
    "GetApplicationActionState",
    { application: id },
    { refetchInterval: 5_000 },
  ).data;
  return (
    state?.deploying || state?.destroying || state?.diffing || false
  );
}

/**
 * The execution button, which becomes Cancel while one is in flight.
 *
 * One button rather than two, matching Build and Terraform: an
 * always-visible Cancel has to answer "cancel what?" when nothing is
 * running, and the honest answer is a message saying nothing was.
 */
function ApplicationExecutionButton({
  id,
  operation,
  label,
  icon,
}: {
  id: string;
  operation:
    | "DeployApplication"
    | "DestroyApplication"
    | "DiffApplication";
  label: string;
  icon: React.ReactNode;
}) {
  const application = useApplication(id);
  const invalidate = useInvalidate();
  const running = useApplicationRunning(id);
  const { mutate: run, isPending: runPending } = useExecute(operation, {
    onSuccess: () =>
      setTimeout(
        () => invalidate(["GetApplicationActionState"]),
        EXECUTION_ACTION_STATE_REQUERY_MS,
      ),
  });
  const { mutate: cancel, isPending: cancelPending } = useExecute(
    "CancelApplication",
    {
      onSuccess: () =>
        setTimeout(
          () => invalidate(["GetApplicationActionState"]),
          EXECUTION_ACTION_STATE_REQUERY_MS,
        ),
    },
  );
  const cancelling = useIsCancelling(
    { type: "Application", id },
    Types.Operation[operation],
    Types.Operation.CancelApplication,
  );

  // Nothing to deploy to without a Cluster.
  if (!application || !application.info.cluster_id) {
    return null;
  }

  if (running) {
    return (
      <ConfirmButton
        variant="filled"
        color="red"
        icon={<ICONS.Cancel size="1rem" />}
        onClick={() => cancel({ application: id })}
        loading={cancelPending || cancelling}
      >
        Cancel
      </ConfirmButton>
    );
  }

  return (
    <ConfirmButton
      icon={icon}
      onClick={() => run({ application: id })}
      loading={runPending}
    >
      {label}
    </ConfirmButton>
  );
}

export const DeployApplication = ({ id }: { id: string }) => (
  <ApplicationExecutionButton
    id={id}
    operation="DeployApplication"
    label="Deploy"
    icon={<ICONS.Deploy size="1rem" />}
  />
);

export const DestroyApplication = ({ id }: { id: string }) => (
  <ApplicationExecutionButton
    id={id}
    operation="DestroyApplication"
    label="Destroy"
    icon={<ICONS.Destroy size="1rem" />}
  />
);

export const DiffApplication = ({ id }: { id: string }) => (
  <ApplicationExecutionButton
    id={id}
    operation="DiffApplication"
    label="Diff"
    icon={<ICONS.UpdateAvailable size="1rem" />}
  />
);
