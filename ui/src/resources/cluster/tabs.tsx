import { useLocalStorage } from "@mantine/hooks";
import { Tabs } from "@mantine/core";
import { MobileFriendlyTabsSelector, TabNoContent } from "mogh_ui";
import { useMemo } from "react";
import { useCluster } from ".";
import { clusterStateIntention } from "@/lib/color";
import { ICONS } from "@/lib/icons";
import ClusterConfig from "./config";
import ClusterObjects from "./objects";

type ClusterTabsView = "Config" | "Browser";

export default function ClusterTabs({ id }: { id: string }) {
  const [view, setView] = useLocalStorage<ClusterTabsView>({
    key: `cluster-${id}-tab`,
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
        value: "Browser",
        icon: ICONS.Inspect,
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
      {view === "Browser" ? (
        <ClusterObjects id={id} titleOther={Selector} />
      ) : (
        <ClusterConfig id={id} titleOther={Selector} />
      )}
    </Tabs>
  );
}
