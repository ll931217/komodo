import { terraformClones, terraformSourceKind } from "@/lib/terraform-source";
import { usePermissions, useRead, useWrite } from "@/lib/hooks";
import { ReactNode } from "react";
import { useFullTerraform } from ".";
import { useLocalStorage } from "@mantine/hooks";
import { Types } from "komodo_client";
import {
  Config,
  ConfigItem,
  ConfigList,
  MonacoEditor,
  ShowHideButton,
} from "mogh_ui";
import { Group, Stack } from "@mantine/core";
import ResourceSelector from "@/resources/selector";
import ResourceLink from "@/resources/link";
import PlaintextSecretWarning from "@/components/config/plaintext-secret-warning";
import SecretsSearch from "@/components/config/secrets-search";

export default function TerraformConfig({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  const { canWrite } = usePermissions({ type: "Terraform", id });
  const terraform = useFullTerraform(id);
  const config = terraform?.config;
  const globalDisabled =
    useRead("GetCoreInfo", {}).data?.ui_write_disabled ?? false;
  const [update, setUpdate] = useLocalStorage<Partial<Types.TerraformConfig>>({
    key: `terraform-${id}-update-v1`,
    defaultValue: {},
  });
  const [show, setShow] = useLocalStorage({
    key: `terraform-${id}-show-v1`,
    defaultValue: { env: true },
  });
  const { mutateAsync } = useWrite("UpdateTerraform");

  if (!config) return null;

  const disabled = globalDisabled || !canWrite;

  // Exactly one source wins, so every other source's fields are dead and
  // are hidden rather than left on screen inviting edits the run will
  // ignore. The precedence lives in one place, with a self-check.
  const filesOnHost = update.files_on_host ?? config.files_on_host;
  const sourceKind = terraformSourceKind({
    files_on_host: filesOnHost,
    linked_repo: update.linked_repo ?? config.linked_repo,
    repo: update.repo ?? config.repo,
  });
  const clonesSomething = terraformClones(sourceKind);

  return (
    <Config
      titleOther={titleOther}
      disabled={disabled}
      original={config}
      update={update}
      setUpdate={setUpdate}
      onSave={async () => {
        // Core deserializes these through file_contents_deserializer /
        // env_vars_deserializer, which append a trailing newline. Send the
        // value Core will store, or the saved config never equals this
        // pending update and the unsaved-changes indicator stays lit forever.
        const config = { ...update };
        for (const field of ["file_contents", "environment"] as const) {
          const value = config[field];
          if (value && !value.endsWith("\n")) config[field] = value + "\n";
        }
        await mutateAsync({ id, config });
      }}
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
                  description="The Server whose Periphery runs terraform for this resource."
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
              cluster_id: (clusterId, set) => (
                <ConfigItem
                  label="Cluster"
                  description="Optionally bridge a Komodo Cluster: its kubeconfig is materialized for the run and exported as TF_VAR_kubeconfig_path / KUBE_CONFIG_PATH, so the kubernetes and helm providers authenticate without a second copy of the credentials."
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
            },
          },
          {
            label: "Source",
            labelHidden: true,
            fields: {
              files_on_host: {
                label: "Files On Host",
                description:
                  "Source the terraform tree from files already on the Server, at the Root Directory below.",
              },
              root_directory: {
                label: "Root Directory",
                description:
                  "Directory on the Server holding the terraform tree.",
                placeholder: "/etc/komodo/terraform",
                hidden: sourceKind !== "FilesOnHost",
              },
              linked_repo: (linkedRepo, set) =>
                filesOnHost ? null : (
                  <ConfigItem
                    label="Linked Repo"
                    description="Source the tree from a Komodo Repo resource. Clear it to configure a git repo inline instead."
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
                  "A git repo to clone the tree from: {namespace}/{repo_name}",
                placeholder: "org/infra",
                hidden: sourceKind !== "Repo" && sourceKind !== "Contents",
              },
              branch: {
                description: "The branch to clone.",
                placeholder: "main",
                hidden: sourceKind !== "Repo",
              },
              commit: {
                description: "Optionally pin a specific commit hash.",
                placeholder: "latest",
                hidden: sourceKind !== "Repo",
              },
              git_provider: {
                label: "Git Provider",
                description: "The git provider domain.",
                placeholder: "github.com",
                hidden: sourceKind !== "Repo",
              },
              git_account: {
                label: "Git Account",
                description:
                  "The account used for private repos. Empty can only clone public repos.",
                hidden: sourceKind !== "Repo",
              },
              reclone: {
                description:
                  "Delete and reclone the repo instead of pulling it. Safe with Managed State, which keeps the state file outside the checkout.",
                hidden: !clonesSomething,
              },
              run_directory: {
                label: "Run Directory",
                description:
                  "The unit directory to run terraform in (-chdir), relative to the tree root. The whole tree is always materialized, since units reference ../../modules-style paths.",
                placeholder: "live/local/workloads",
              },
              file_contents: (value, set) =>
                sourceKind !== "Contents" ? null : (
                  <ConfigItem
                    label="Terraform"
                    description="Terraform managed here, written as main.tf into a persistent working directory on the Server. Supports [[VARIABLE]] interpolation."
                  >
                    <MonacoEditor
                      value={value}
                      onValueChange={(file_contents) => set({ file_contents })}
                      // mogh_ui's monaco has no hcl mode; toml is the
                      // closest of the ones it ships.
                      language="toml"
                      readOnly={disabled}
                    />
                  </ConfigItem>
                ),
            },
          },
          {
            label: "Environment",
            description:
              "Written to a private env file on the Server and sourced before the run - TF_VAR_* and provider credentials. Never passed on the command line.",
            actions: (
              <ShowHideButton
                show={show.env}
                setShow={(env) => setShow({ ...show, env })}
              />
            ),
            contentHidden: !show.env,
            fields: {
              environment: (env, set) => (
                <Stack>
                  <PlaintextSecretWarning value={env} />
                  <SecretsSearch
                    server={update.server_id ?? config.server_id}
                  />
                  <MonacoEditor
                    value={env || "  # TF_VAR_example = value\n"}
                    onValueChange={(environment) => set({ environment })}
                    language="key_value"
                    readOnly={disabled}
                  />
                </Stack>
              ),
              skip_secret_interp: {
                label: "Skip Secret Interpolation",
                description:
                  "Do not interpolate Komodo Variables into the environment and terraform contents.",
              },
            },
          },
          {
            label: "Run",
            labelHidden: true,
            fields: {
              managed_state: {
                label: "Managed State",
                description:
                  "Keep the local backend's state file outside the checkout, at a Periphery-managed path, via init -backend-config=path=. Turn off for units that declare their own remote backend.",
              },
              proxy_url: {
                label: "Proxy Url",
                description:
                  "Exported as HTTP_PROXY / HTTPS_PROXY for providers that fetch from outside the cluster, such as helm chart repos.",
                placeholder: "http://proxy.internal:8888",
              },
              no_proxy: {
                label: "No Proxy",
                description:
                  "Exported alongside the proxy, so the Kubernetes api server is dialed directly instead of through it.",
                placeholder: "10.0.0.0/8,localhost",
              },
              extra_args: (values, set) => (
                <ConfigList
                  label="Extra Args"
                  addLabel="Add Arg"
                  description="Additional arguments passed to plan / apply / destroy. Never put secrets here: the command line is visible to every process on the host."
                  field="extra_args"
                  values={values ?? []}
                  set={set}
                  disabled={disabled}
                  placeholder="-target=module.example"
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
                  "Whether to alert when a scheduled plan finds drift, or when a run fails.",
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
                  "Whether an incoming webhook triggers a Plan for this resource.",
              },
              webhook_secret: {
                label: "Webhook Secret",
                description:
                  "An alternate secret for this resource. Empty uses the default from the core config.",
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
