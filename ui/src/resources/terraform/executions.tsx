import { Types } from "komodo_client";
import {
  useExecute,
  useInvalidate,
  useIsCancelling,
  useRead,
} from "@/lib/hooks";
import { useTerraform } from ".";
import { ConfirmButton } from "mogh_ui";
import { ICONS } from "@/lib/icons";
import { EXECUTION_ACTION_STATE_REQUERY_MS } from "@/lib/utils";

/**
 * Whether terraform is doing anything on the host right now.
 *
 * All four states share one Cancel, because they share one command:
 * cancelling sends CancelExecution for whichever request is in flight,
 * and the host kills that process group. Init is included on purpose -
 * it blocks on the state backend, so a locked or unreachable backend
 * hangs there for the full timeout with nothing to show for it.
 */
function useTerraformRunning(id: string) {
  const state = useRead(
    "GetTerraformActionState",
    { terraform: id },
    { refetchInterval: 5_000 },
  ).data;
  return (
    state?.initializing ||
    state?.planning ||
    state?.applying ||
    state?.destroying ||
    false
  );
}

function useCancelTerraform() {
  const invalidate = useInvalidate();
  return useExecute("CancelTerraform", {
    onSuccess: () =>
      setTimeout(
        () => invalidate(["GetTerraformActionState"]),
        EXECUTION_ACTION_STATE_REQUERY_MS,
      ),
  });
}

/**
 * The run button, which becomes Cancel while a run is in flight.
 *
 * One button rather than two, matching Build: a Cancel that is always
 * visible has to answer "cancel what?" when nothing is running, and
 * the honest answer is a message saying nothing was running.
 */
function TerraformExecutionButton({
  id,
  operation,
  label,
  icon,
}: {
  id: string;
  operation: "PlanTerraform" | "ApplyTerraform" | "DestroyTerraform";
  label: string;
  icon: React.ReactNode;
}) {
  const terraform = useTerraform(id);
  const invalidate = useInvalidate();
  const running = useTerraformRunning(id);
  const { mutate: run, isPending: runPending } = useExecute(operation, {
    onSuccess: () =>
      setTimeout(
        () => invalidate(["GetTerraformActionState"]),
        EXECUTION_ACTION_STATE_REQUERY_MS,
      ),
  });
  const { mutate: cancel, isPending: cancelPending } = useCancelTerraform();
  const cancelling = useIsCancelling(
    { type: "Terraform", id },
    Types.Operation[operation],
    Types.Operation.CancelTerraform,
  );

  // Nothing to run without a Server to run terraform on.
  if (!terraform || !terraform.info.server_id) {
    return null;
  }

  if (running) {
    return (
      <ConfirmButton
        variant="filled"
        color="red"
        icon={<ICONS.Cancel size="1rem" />}
        onClick={() => cancel({ terraform: id })}
        loading={cancelPending || cancelling}
      >
        Cancel
      </ConfirmButton>
    );
  }

  return (
    <ConfirmButton
      icon={icon}
      onClick={() => run({ terraform: id })}
      loading={runPending}
    >
      {label}
    </ConfirmButton>
  );
}

export const PlanTerraform = ({ id }: { id: string }) => (
  <TerraformExecutionButton
    id={id}
    operation="PlanTerraform"
    label="Plan"
    icon={<ICONS.UpdateAvailable size="1rem" />}
  />
);

export const ApplyTerraform = ({ id }: { id: string }) => (
  <TerraformExecutionButton
    id={id}
    operation="ApplyTerraform"
    label="Apply"
    icon={<ICONS.Deploy size="1rem" />}
  />
);

export const DestroyTerraform = ({ id }: { id: string }) => (
  <TerraformExecutionButton
    id={id}
    operation="DestroyTerraform"
    label="Destroy"
    icon={<ICONS.Destroy size="1rem" />}
  />
);
