import { ICONS } from "@/lib/icons";
import { Section } from "mogh_ui";
import { ReactNode } from "react";
import ResourceTable from "@/resources/table";

/**
 * What is deployed to this Cluster, on the Cluster's own page.
 *
 * Both sections use the same [ResourceTable] the top-level list pages
 * use, scoped with `specific`, rather than the bare per-type tables.
 * That is deliberate: a scoped table built by hand silently loses
 * search, tag filters, pagination and the batch-execution menu, so the
 * tab ends up looking like a worse version of the page it is meant to
 * replace - and the obvious conclusion ("tabs can't do this") would be
 * wrong. `ui/src/resources/server/resources.tsx` does the same thing
 * for a Server's Stacks and Deployments; this follows it.
 *
 * The two sections are NOT the same relationship, and the copy says so:
 *
 * - An Application cannot exist without a Cluster. It has no server_id
 *   of its own and `application_cluster()` errors outright when
 *   cluster_id is empty. Listing them here is listing what belongs here.
 * - A Terraform resource runs on a Server. Its Cluster link is optional
 *   and only materializes a kubeconfig for the kubernetes/helm
 *   providers, so a unit managing a database or a DNS zone has no
 *   Cluster at all and correctly appears in none of these lists.
 */
export default function ClusterDeployed({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  return (
    <Section titleOther={titleOther} gap={48}>
      <Section
        title="Applications"
        icon={<ICONS.Application size="1.3rem" />}
        description="Kubernetes manifests this Cluster deploys with kubectl."
      >
        <ResourceTable
          type="Application"
          newProps={{ clusterId: id }}
          specific={{ clusters: [id] }}
        />
      </Section>

      <Section
        title="Terraform"
        icon={<ICONS.Terraform size="1.3rem" />}
        description="Terraform units bridged to this Cluster's kubeconfig, so their kubernetes and helm providers can authenticate. Units that manage anything else run on a Server and are not listed here."
      >
        <ResourceTable
          type="Terraform"
          newProps={{ clusterId: id }}
          specific={{ clusters: [id], servers: [] }}
        />
      </Section>
    </Section>
  );
}

/** The tab icon, kept beside the view it belongs to. */
export const CLUSTER_DEPLOYED_ICON = ICONS.Deploy;
