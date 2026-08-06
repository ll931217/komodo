import { ReactNode, useMemo } from "react";
import { useCluster } from ".";
import { Types } from "komodo_client";
import { useLocalStorage } from "@mantine/hooks";
import { MobileFriendlyTabsSelector, Section, TabNoContent } from "mogh_ui";
import { Center, Stack, Tabs, Text } from "@mantine/core";
import { clusterStateIntention } from "@/lib/color";
import { ICONS } from "@/lib/icons";
import ClusterObjects from "./objects";
import ClusterHelm from "./helm";

type ClusterKubernetesView =
  | "Nodes"
  | "Pods"
  | "Workloads"
  | "Services"
  | "ConfigMaps"
  | "Secrets"
  | "Events"
  | "Helm"
  | "Other";

/// The `kubectl get` kind each tab pins. "Other" pins nothing,
/// leaving the kind selector for CRDs and rarer kinds.
const VIEW_KINDS: Record<ClusterKubernetesView, string | undefined> = {
  Nodes: "nodes",
  Pods: "pods",
  Workloads: "deployments",
  Services: "services",
  ConfigMaps: "configmaps",
  Secrets: "secrets",
  Events: "events",
  // Helm renders its own releases view, not a kubectl kind.
  Helm: undefined,
  Other: undefined,
};

export default function ClusterKubernetesResources({
  id,
  titleOther,
}: {
  id: string;
  titleOther: ReactNode;
}) {
  const state = useCluster(id)?.info.state ?? Types.ClusterState.Unknown;
  const [view, setView] = useLocalStorage<ClusterKubernetesView>({
    key: "cluster-kubernetes-view-v1",
    defaultValue: "Pods",
  });

  const tabsNoContent = useMemo<TabNoContent[]>(
    () => [
      { value: "Nodes", icon: ICONS.ClusterNode },
      { value: "Pods", icon: ICONS.ClusterPod },
      { value: "Workloads", icon: ICONS.ClusterWorkload },
      { value: "Services", icon: ICONS.ClusterService },
      { value: "ConfigMaps", icon: ICONS.ClusterConfigMap },
      { value: "Secrets", icon: ICONS.ClusterSecret },
      { value: "Events", icon: ICONS.ClusterEvent },
      { value: "Helm", icon: ICONS.ClusterHelm },
      { value: "Other", icon: ICONS.ClusterOther },
    ],
    [],
  );

  const Selector = (
    <MobileFriendlyTabsSelector
      tabs={tabsNoContent}
      value={view}
      onValueChange={setView as any}
    />
  );

  if (state === Types.ClusterState.Unknown) {
    return (
      <Section titleOther={titleOther}>
        <Center h="20vh">
          <Stack align="center" justify="center" gap="0">
            <Text fz="h2">Cluster unreachable</Text>
            <Text c="dimmed">Kubernetes resources are not available</Text>
          </Stack>
        </Center>
      </Section>
    );
  }

  return (
    <Section titleOther={titleOther}>
      <Tabs color={clusterStateIntention(state)} value={view}>
        {view === "Helm" ? (
          <ClusterHelm id={id} titleOther={Selector} />
        ) : (
          <ClusterObjects
            // Remount on tab change so namespace / kind state resets,
            // instead of carrying a pod namespace over to nodes.
            key={view}
            id={id}
            kind={VIEW_KINDS[view]}
            titleOther={Selector}
          />
        )}
      </Tabs>
    </Section>
  );
}
