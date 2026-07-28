import { usePermissions, useRead, useWrite } from "@/lib/hooks";
import { ReactNode } from "react";
import { useFullCluster } from ".";
import { useLocalStorage } from "@mantine/hooks";
import { Types } from "komodo_client";
import { Config, ConfigItem, ConfigList } from "mogh_ui";
import { Group } from "@mantine/core";
import ResourceSelector from "@/resources/selector";
import ResourceLink from "@/resources/link";

export default function ClusterConfig({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  const { canWrite } = usePermissions({ type: "Cluster", id });
  const cluster = useFullCluster(id);
  const config = cluster?.config;
  const globalDisabled =
    useRead("GetCoreInfo", {}).data?.ui_write_disabled ?? false;
  const [update, setUpdate] = useLocalStorage<Partial<Types.ClusterConfig>>({
    key: `cluster-${id}-update-v1`,
    defaultValue: {},
  });
  const { mutateAsync } = useWrite("UpdateCluster");

  if (!config) return null;

  const disabled = globalDisabled || !canWrite;

  return (
    <Config
      titleOther={titleOther}
      disabled={disabled}
      original={config}
      update={update}
      setUpdate={setUpdate}
      onSave={() => mutateAsync({ id, config: update })}
      groups={{
        "": [
          {
            label: "Server",
            labelHidden: true,
            fields: {
              server_id: (serverId, set) => (
                <ConfigItem
                  label={
                    serverId ? (
                      <Group fz="h3" fw="bold">
                        Server:
                        <ResourceLink
                          type="Server"
                          id={serverId}
                          fz="h3"
                          iconSize="1.2rem"
                        />
                      </Group>
                    ) : (
                      "Select Server"
                    )
                  }
                  description="The Server holding the kubeconfig. Cluster commands run on its Periphery."
                >
                  <ResourceSelector
                    type="Server"
                    selected={serverId}
                    onSelect={(server_id) => set({ server_id })}
                    disabled={disabled}
                    clearable
                  />
                </ConfigItem>
              ),
            },
          },
          {
            label: "Kubeconfig",
            labelHidden: true,
            fields: {
              kubeconfig_path: {
                label: "Kubeconfig Path",
                description:
                  "Path to the kubeconfig on the Server. Leave empty to use the default kubectl resolution ($KUBECONFIG, then ~/.kube/config).",
                placeholder: "/etc/komodo/kubeconfig",
              },
              context: {
                description:
                  "The kubeconfig context to use. Leave empty for the kubeconfig's current context.",
                placeholder: "my-cluster",
              },
              namespace: {
                description:
                  "The default namespace for Cluster operations. Leave empty for 'default'.",
                placeholder: "default",
              },
            },
          },
          {
            label: "Links",
            labelHidden: true,
            fields: {
              links: (values, set) => (
                <ConfigList
                  label="Links"
                  addLabel="Add Link"
                  description="Add quick links in the resource header"
                  field="links"
                  values={values ?? []}
                  set={set}
                  disabled={disabled}
                  placeholder="Input link"
                />
              ),
            },
          },
        ],
      }}
    />
  );
}
