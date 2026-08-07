import { ReactNode, useMemo } from "react";
import { useCluster } from ".";
import { Types } from "komodo_client";
import { useLocalStorage } from "@mantine/hooks";
import { MobileFriendlyTabsSelector, Section, TabNoContent } from "mogh_ui";
import { Box, Button, Center, Group, Stack, Tabs, Text } from "@mantine/core";
import { clusterStateIntention } from "@/lib/color";
import { ICONS } from "@/lib/icons";
import ClusterObjects from "./objects";
import ClusterHelm from "./helm";
import ClusterForwards from "./forwards";

type ClusterKubernetesView =
  | "Nodes"
  | "Pods"
  | "Workloads"
  | "Services"
  | "ConfigMaps"
  | "Secrets"
  | "Events"
  | "Helm"
  | "Forwards"
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
  // Helm and Forwards render their own views, not a kubectl kind.
  Helm: undefined,
  Forwards: undefined,
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
      { value: "Forwards", icon: ICONS.ClusterForward },
      { value: "Other", icon: ICONS.ClusterOther },
    ],
    [],
  );

  // Kept for narrow screens: a vertical rail alongside the global
  // sidebar leaves nothing for the tables, so below `sm` the views stay
  // a horizontal selector in the section header.
  const Selector = (
    <Box hiddenFrom="sm">
      <MobileFriendlyTabsSelector
        tabs={tabsNoContent}
        value={view}
        onValueChange={setView as any}
      />
    </Box>
  );

  const Rail = (
    <Stack gap="0.15rem" w={168} visibleFrom="sm" style={{ flexShrink: 0 }}>
      {tabsNoContent.map(({ value, icon: Icon }) => (
        <Button
          key={value}
          variant={view === value ? "default" : "subtle"}
          color={view === value ? clusterStateIntention(state) : undefined}
          leftSection={Icon ? <Icon size="1rem" /> : undefined}
          justify="flex-start"
          size="compact-md"
          onClick={() => setView(value as ClusterKubernetesView)}
          fullWidth
        >
          {value}
        </Button>
      ))}
    </Stack>
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
        <Group align="flex-start" gap="lg" wrap="nowrap">
          {Rail}
          {/* miw=0 so the wide object tables can shrink inside the flex
              row instead of pushing the rail off-screen. */}
          <Box flex={1} miw={0}>
            {view === "Helm" ? (
              <ClusterHelm id={id} titleOther={Selector} />
            ) : view === "Forwards" ? (
              <ClusterForwards id={id} titleOther={Selector} />
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
          </Box>
        </Group>
      </Tabs>
    </Section>
  );
}
