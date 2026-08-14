import { useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { Section } from "mogh_ui";
import { ReactNode } from "react";
import { Stack, Text } from "@mantine/core";
import ApplicationTable from "@/resources/application/table";
import TerraformTable from "@/resources/terraform/table";

/**
 * What is deployed to this Cluster, on the Cluster's own page.
 *
 * A Cluster holds the credentials and the policy; the things that
 * actually deploy into it are separate resources pointing back at it.
 * That relationship is invisible from a sidebar of peers, and "what is
 * running on this cluster" is asked here, not from a top-level list.
 */
export default function ClusterDeployed({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  const applications =
    useRead("ListApplications", {
      query: { specific: { clusters: [id] } },
    }).data ?? [];
  const terraforms =
    useRead("ListTerraforms", {
      query: { specific: { clusters: [id], servers: [] } },
    }).data ?? [];

  return (
    <Section titleOther={titleOther}>
      <Stack gap="xl">
        <Stack gap="xs">
          <Text fz="h3" fw="bold">
            Applications
          </Text>
          <Text c="dimmed" fz="sm">
            Kubernetes manifests deployed to this Cluster with kubectl.
          </Text>
          {applications.length ? (
            <ApplicationTable resources={applications} />
          ) : (
            <Text c="dimmed" fz="sm">
              No Applications target this Cluster.
            </Text>
          )}
        </Stack>

        <Stack gap="xs">
          <Text fz="h3" fw="bold">
            Terraform
          </Text>
          <Text c="dimmed" fz="sm">
            Terraform units that use this Cluster's kubeconfig, so their
            kubernetes and helm providers can authenticate.
          </Text>
          {terraforms.length ? (
            <TerraformTable resources={terraforms} />
          ) : (
            <Text c="dimmed" fz="sm">
              No Terraform resources are bridged to this Cluster.
            </Text>
          )}
        </Stack>
      </Stack>
    </Section>
  );
}

/** The tab icon, kept beside the view it belongs to. */
export const CLUSTER_DEPLOYED_ICON = ICONS.Deploy;
