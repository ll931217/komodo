import { usePermissions, useRead, useWrite } from "@/lib/hooks";
import { ReactNode } from "react";
import { useFullApplication } from ".";
import { useLocalStorage } from "@mantine/hooks";
import { Types } from "komodo_client";
import { Config, ConfigItem, ConfigList, MonacoEditor } from "mogh_ui";
import { Group } from "@mantine/core";
import ResourceSelector from "@/resources/selector";
import ResourceLink from "@/resources/link";

export default function ApplicationConfig({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  const { canWrite } = usePermissions({ type: "Application", id });
  const application = useFullApplication(id);
  const config = application?.config;
  const globalDisabled =
    useRead("GetCoreInfo", {}).data?.ui_write_disabled ?? false;
  const [update, setUpdate] = useLocalStorage<
    Partial<Types.ApplicationConfig>
  >({
    key: `application-${id}-update-v1`,
    defaultValue: {},
  });
  const { mutateAsync } = useWrite("UpdateApplication");

  if (!config) return null;

  const disabled = globalDisabled || !canWrite;

  return (
    <Config
      titleOther={titleOther}
      disabled={disabled}
      original={config}
      update={update}
      setUpdate={setUpdate}
      onSave={async () => {
        // Core deserializes file_contents through
        // file_contents_deserializer, which appends a trailing
        // newline. Send the value Core will store, or the saved config
        // never equals this pending update and the unsaved-changes
        // indicator stays lit forever.
        const config = { ...update };
        if (config.file_contents && !config.file_contents.endsWith("\n")) {
          config.file_contents = config.file_contents + "\n";
        }
        await mutateAsync({ id, config });
      }}
      groups={{
        "": [
          {
            label: "Cluster",
            labelHidden: true,
            fields: {
              cluster_id: (clusterId, set) => (
                <ConfigItem
                  label={
                    clusterId ? (
                      <Group fz="h3" fw="bold">
                        Cluster:
                        <ResourceLink
                          type="Cluster"
                          id={clusterId}
                          fz="h3"
                          iconSize="1.2rem"
                        />
                      </Group>
                    ) : (
                      "Select Cluster"
                    )
                  }
                  description="The Cluster to deploy to. It supplies the kubeconfig, the Server that runs kubectl, and the policy this Application cannot widen - allowed namespaces, and whether cluster-scoped objects may be touched at all."
                >
                  <ResourceSelector
                    type="Cluster"
                    selected={clusterId}
                    onSelect={(cluster_id) => set({ cluster_id })}
                    disabled={disabled}
                    clearable
                  />
                </ConfigItem>
              ),
              namespace: {
                description:
                  "The namespace to deploy into. Empty uses the Cluster's default. Must be permitted by the Cluster's allowed namespaces - and note a kustomization setting `namespace:` itself wins over this for the objects it generates.",
                placeholder: "default",
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
                  description="Source the manifests from a Komodo Repo resource. Takes precedence over the git fields below. Attaching one requires read access to that Repo."
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
              kustomize: {
                label: "Kustomize",
                description:
                  "Apply the run directory with kustomize (kubectl apply -k), which requires a kustomization.yaml in it. File Paths are ignored when this is on.",
              },
              file_contents: (value, set) => (
                <ConfigItem
                  label="Manifests"
                  description="Manifests managed here, written to the Server at execution time. Supports [[VARIABLE]] interpolation. Used only when no other manifest source above is configured."
                >
                  <MonacoEditor
                    value={value}
                    onValueChange={(file_contents) => set({ file_contents })}
                    language="yaml"
                    readOnly={disabled}
                  />
                </ConfigItem>
              ),
              skip_secret_interp: {
                label: "Skip Secret Interpolation",
                description:
                  "Do not interpolate Komodo Variables into the manifests. The Cluster's kubeconfig has its own setting.",
              },
            },
          },
          {
            label: "Deploy",
            labelHidden: true,
            fields: {
              wait_ready: {
                label: "Wait Until Ready",
                description:
                  "After a successful deploy, wait for the applied workloads to roll out (kubectl rollout status) and fail the Deploy if they never become ready.",
              },
              extra_args: (values, set) => (
                <ConfigList
                  label="Extra Args"
                  addLabel="Add Arg"
                  description="Additional arguments passed to kubectl apply / delete."
                  field="extra_args"
                  values={values ?? []}
                  set={set}
                  disabled={disabled}
                  placeholder="--prune"
                />
              ),
            },
          },
          {
            label: "Alerts",
            labelHidden: true,
            fields: {
              send_alerts: {
                label: "Send Alerts",
                description:
                  "Whether to alert when a deploy fails, or a scheduled Diff finds differences.",
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
                  "Whether an incoming webhook triggers a Deploy for this Application.",
              },
              webhook_secret: {
                label: "Webhook Secret",
                description:
                  "An alternate secret for this Application. Empty uses the default from the core config.",
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
