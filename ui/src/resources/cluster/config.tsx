import { usePermissions, useRead, useWrite } from "@/lib/hooks";
import { ReactNode } from "react";
import { useFullCluster } from ".";
import { useLocalStorage } from "@mantine/hooks";
import { Types } from "komodo_client";
import { Config, ConfigItem, ConfigList, MonacoEditor } from "mogh_ui";
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
              kubeconfig_contents: (value, set) => (
                <ConfigItem
                  label="Kubeconfig"
                  description="Kubeconfig managed here, written to the Server at execution time. Supports [[VARIABLE]] interpolation so credentials can live in Komodo Variables. Any kubectl auth method works, including bearer token, client certificate, and exec plugins for EKS/GKE/AKS. Takes precedence over the path below."
                >
                  <MonacoEditor
                    value={value}
                    onValueChange={(kubeconfig_contents) =>
                      set({ kubeconfig_contents })
                    }
                    language="yaml"
                    readOnly={disabled}
                  />
                </ConfigItem>
              ),
              kubeconfig_path: {
                label: "Kubeconfig Path",
                description:
                  "Path to an existing kubeconfig on the Server. If both fields are empty, kubectl's default resolution is used ($KUBECONFIG, then ~/.kube/config).",
                placeholder: "/etc/komodo/kubeconfig",
              },
              skip_secret_interp: {
                label: "Skip Secret Interpolation",
                description:
                  "Do not interpolate Komodo Variables into the kubeconfig.",
              },
              context: {
                description:
                  "The kubeconfig context to use. Leave empty for the kubeconfig's current context.",
                placeholder: "my-cluster",
              },
              proxy_url: {
                label: "Proxy Url",
                description:
                  "Optional proxy used to reach the Kubernetes api server.",
                placeholder: "http://proxy.internal:8888",
              },
            },
          },
          {
            label: "Manifest Source",
            labelHidden: true,
            fields: {
              files_on_host: {
                label: "Files On Host",
                description:
                  "Source the manifests from files already on the Server, using the directory and paths below.",
              },
              linked_repo: (linkedRepo, set) => (
                <ConfigItem
                  label="Linked Repo"
                  description="Source the manifests from a Komodo Repo resource. Takes precedence over the git fields below."
                >
                  <ResourceSelector
                    type="Repo"
                    selected={linkedRepo}
                    onSelect={(linked_repo) => set({ linked_repo })}
                    disabled={disabled}
                    clearable
                  />
                </ConfigItem>
              ),
              repo: {
                description:
                  "A git repo to clone manifests from: {namespace}/{repo_name}",
                placeholder: "org/manifests",
              },
              branch: {
                description: "The branch to clone.",
                placeholder: "main",
              },
              commit: {
                description: "Optionally pin a specific commit hash.",
                placeholder: "latest",
              },
              git_provider: {
                label: "Git Provider",
                description: "The git provider domain.",
                placeholder: "github.com",
              },
              git_account: {
                label: "Git Account",
                description:
                  "The account used for private repos. Empty can only clone public repos.",
              },
              reclone: {
                description:
                  "Delete and reclone the repo instead of pulling it.",
              },
              run_directory: {
                label: "Run Directory",
                description:
                  "Directory the manifests live in, relative to the repo root, or absolute for files on host.",
                placeholder: "./",
              },
              file_paths: (values, set) => (
                <ConfigList
                  label="File Paths"
                  addLabel="Add Path"
                  description="Manifest paths relative to the run directory. Empty applies the whole directory."
                  field="file_paths"
                  values={values ?? []}
                  set={set}
                  disabled={disabled}
                  placeholder="Input path"
                />
              ),
            },
          },
          {
            label: "Deploy",
            labelHidden: true,
            fields: {
              wait_ready: {
                label: "Wait Until Ready",
                description:
                  "After a successful apply, wait for the applied workloads to roll out (kubectl rollout status) and fail the Deploy if they never become ready.",
              },
            },
          },
          {
            label: "Webhook",
            labelHidden: true,
            fields: {
              webhook_enabled: {
                label: "Webhook Enabled",
                description:
                  "Whether an incoming webhook triggers a Deploy for this Cluster.",
              },
              webhook_secret: {
                label: "Webhook Secret",
                description:
                  "An alternate secret for this Cluster. Empty uses the default from the core config.",
              },
            },
          },
          {
            label: "Scope",
            labelHidden: true,
            fields: {
              namespace: {
                label: "Default Namespace",
                description:
                  "The namespace Cluster operations target when none is given. Leave empty for 'default'.",
                placeholder: "default",
              },
              namespaces: (values, set) => (
                <ConfigList
                  label="Allowed Namespaces"
                  addLabel="Add Namespace"
                  description="Restrict Cluster operations to these namespaces. Empty allows every namespace."
                  field="namespaces"
                  values={values ?? []}
                  set={set}
                  disabled={disabled}
                  placeholder="Input namespace"
                />
              ),
              cluster_resources: {
                label: "Allow Cluster Resources",
                description:
                  "Whether cluster-scoped objects (Namespaces, ClusterRoles, CRDs) may be touched. Turn off to limit this Cluster to namespaced objects.",
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
