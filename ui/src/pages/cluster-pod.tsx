import LogSection from "@/components/log-section";
import InspectSection from "@/components/inspect-section";
import TerminalSection from "@/components/terminal/section";
import { useExecute, usePermissions, useRead, useSetTitle } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { useCluster } from "@/resources/cluster";
import {
  objectsFromListing,
  statusIntention,
} from "@/resources/cluster/objects";
import ResourceSubPage from "@/resources/sub-page";
import { Box, Center, Select, Switch, Tabs, Text } from "@mantine/core";
import { useLocalStorage } from "@mantine/hooks";
import { Types } from "komodo_client";
import {
  ConfirmButton,
  MobileFriendlyTabsSelector,
  TabNoContent,
} from "mogh_ui";
import { useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

export default function ClusterPod() {
  const { type, id, namespace, pod } = useParams() as {
    type: string;
    id: string;
    namespace: string;
    pod: string;
  };
  if (type !== "clusters") {
    return (
      <Center h="50vh">
        <Text>This resource type does not have any pods.</Text>
      </Center>
    );
  }
  return <ClusterPodInner clusterId={id} namespace={namespace} pod={pod} />;
}

function ClusterPodInner({
  clusterId,
  namespace,
  pod,
}: {
  clusterId: string;
  namespace: string;
  pod: string;
}) {
  const cluster = useCluster(clusterId);
  useSetTitle(`${cluster?.name} | Pod | ${pod}`);
  const navigate = useNavigate();
  const { canExecute } = usePermissions({ type: "Cluster", id: clusterId });

  const listing = useRead(
    "ListClusterResources",
    { cluster: clusterId, kind: "pods", namespace },
    { refetchInterval: 10_000 },
  ).data;
  const object = objectsFromListing(listing).find((o) => o.name === pod);
  const raw = object?.raw as any;
  const node: string | undefined = raw?.spec?.nodeName;

  const { mutateAsync: deleteObject, isPending: deleting } = useExecute(
    "DeleteClusterObject",
    { onSuccess: () => navigate(`/clusters/${clusterId}`) },
  );
  const destroy = () =>
    deleteObject({ cluster: clusterId, kind: "pods", name: pod, namespace });

  return (
    <ResourceSubPage
      entityTypeName="Pod"
      parentType="Cluster"
      parentId={clusterId}
      name={pod}
      icon={ICONS.Container}
      intent={statusIntention(object?.status)}
      state={object?.status}
      status={object?.ready && `Ready ${object.ready}`}
      info={
        <>
          <Text>{namespace}</Text>
          {node && <Text>{node}</Text>}
          {object?.restarts ? (
            <Text c="orange">{object.restarts} restarts</Text>
          ) : null}
        </>
      }
      executions={
        <>
          {/* Deleting an owned pod is how Kubernetes restarts it. */}
          <ConfirmButton
            icon={<ICONS.Refresh size="1rem" />}
            disabled={!canExecute || deleting}
            onClick={destroy}
          >
            Restart
          </ConfirmButton>
          <ConfirmButton
            color="red"
            icon={<ICONS.Destroy size="1rem" />}
            disabled={!canExecute || deleting}
            onClick={destroy}
          >
            Delete
          </ConfirmButton>
        </>
      }
    >
      <PodTabs
        clusterId={clusterId}
        namespace={namespace}
        pod={pod}
        raw={raw}
        status={object?.status}
      />
    </ResourceSubPage>
  );
}

type PodTabsView = "Log" | "Inspect" | "Terminals";

function PodTabs({
  clusterId,
  namespace,
  pod,
  raw,
  status,
}: {
  clusterId: string;
  namespace: string;
  pod: string;
  raw: any;
  status?: string;
}) {
  const [_view, setView] = useLocalStorage<PodTabsView>({
    key: `cluster-${clusterId}-pod-tabs-v1`,
    defaultValue: "Log",
  });
  const { specificLogs, specificInspect, specificTerminal } = usePermissions({
    type: "Cluster",
    id: clusterId,
  });

  const containers: string[] = (raw?.spec?.containers ?? [])
    .map((c: any) => c?.name)
    .filter(Boolean);
  const [_container, setContainer] = useState<string | null>(null);
  const container =
    containers.length > 1 ? (_container ?? containers[0]) : undefined;
  const [previous, setPrevious] = useState(false);

  const view =
    (_view === "Inspect" && !specificInspect) ||
    (_view === "Terminals" && !specificTerminal)
      ? "Log"
      : _view;

  const tabs = useMemo<TabNoContent[]>(
    () => [
      {
        value: "Log",
        hidden: !specificLogs,
        icon: ICONS.Log,
      },
      {
        value: "Inspect",
        hidden: !specificInspect,
        icon: ICONS.Inspect,
      },
      {
        value: "Terminals",
        hidden: !specificTerminal,
        icon: ICONS.Terminal,
      },
    ],
    [specificLogs, specificInspect, specificTerminal],
  );

  const Selector = (
    <MobileFriendlyTabsSelector
      tabs={tabs}
      value={view}
      onValueChange={setView as any}
    />
  );

  const terminalTarget: Types.TerminalTarget = useMemo(
    () => ({
      type: "ClusterPod",
      params: { cluster: clusterId, namespace, pod, container },
    }),
    [clusterId, namespace, pod, container],
  );

  let View = Selector;
  switch (view) {
    case "Log":
      View = (
        <LogSection
          target={{
            type: "ClusterPod",
            clusterId,
            pod,
            namespace,
            container,
            previous,
          }}
          titleOther={Selector}
          disabled={!specificLogs}
          extraController={
            <>
              {containers.length > 1 && (
                <Select
                  value={container}
                  onChange={setContainer}
                  data={containers}
                  allowDeselect={false}
                  w={160}
                />
              )}
              <Switch
                label="Previous crash"
                checked={previous}
                onChange={(e) => setPrevious(e.currentTarget.checked)}
              />
            </>
          }
        />
      );
      break;
    case "Inspect":
      View = <InspectSection json={raw} titleOther={Selector} />;
      break;
    case "Terminals":
      View = <TerminalSection target={terminalTarget} titleOther={Selector} />;
      break;
  }

  return (
    <Box>
      <Tabs color={statusIntention(status)} value={view}>
        {View}
      </Tabs>
    </Box>
  );
}
