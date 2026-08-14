import { useLocalStorage } from "@mantine/hooks";
import { Tabs } from "@mantine/core";
import { MobileFriendlyTabsSelector, TabNoContent } from "mogh_ui";
import { useMemo } from "react";
import { useCluster } from ".";
import { clusterStateIntention } from "@/lib/color";
import { ICONS } from "@/lib/icons";
import ClusterConfig from "./config";
import ClusterKubernetesResources from "./kubernetes";
import ClusterDeployed, { CLUSTER_DEPLOYED_ICON } from "./deployed";

type ClusterTabsView = "Config" | "Deployed" | "Kubernetes";

export default function ClusterTabs({ id }: { id: string }) {
  const [view, setView] = useLocalStorage<ClusterTabsView>({
    key: `cluster-${id}-tab-v1`,
    defaultValue: "Config",
  });
  const state = useCluster(id)?.info.state;

  const tabs = useMemo<TabNoContent[]>(
    () => [
      {
        value: "Config",
        icon: ICONS.Settings,
      },
      {
        value: "Deployed",
        icon: CLUSTER_DEPLOYED_ICON,
      },
      {
        value: "Kubernetes",
        icon: ICONS.Cluster,
      },
    ],
    [],
  );

  const Selector = (
    <MobileFriendlyTabsSelector
      tabs={tabs}
      value={view}
      onValueChange={setView as any}
    />
  );

  return (
    <Tabs color={clusterStateIntention(state)} value={view}>
      {view === "Kubernetes" ? (
        <ClusterKubernetesResources id={id} titleOther={Selector} />
      ) : view === "Deployed" ? (
        <ClusterDeployed id={id} titleOther={Selector} />
      ) : (
        <ClusterConfig id={id} titleOther={Selector} />
      )}
    </Tabs>
  );
}
