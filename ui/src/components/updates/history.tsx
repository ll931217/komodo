/**
 * A resource's revision history.
 *
 * Deliberately NOT a new query or a new diff view - both already exist
 * and work. `getUpdateQuery` already scopes ListUpdates to a resource
 * (and, for a Deployment, folds in the attached Build's runs so "why did
 * this change" includes the build that caused it), and UpdateCard
 * already opens a details modal that renders the prev_toml -> current_toml
 * diff.
 *
 * What was missing was room to read it: the existing ResourceUpdates
 * section caps at 10 entries inside a 180px scroll box, which reads as a
 * sidebar widget. Anything past the tenth meant leaving the page. This
 * is the same data, paginated, at a size you can actually scan.
 */
import { useMemo, useState } from "react";
import { Center, Group, Pagination, Stack, Text } from "@mantine/core";
import { Section } from "mogh_ui";
import { Types } from "komodo_client";

import { useRead } from "@/lib/hooks";
import { getUpdateQuery } from "@/lib/utils";
import { ICONS } from "@/lib/icons";
import UpdateCard from "./card";

export default function ResourceHistory({
  type,
  id,
}: Types.ResourceTarget) {
  const [page, setPage] = useState(0);

  // A Deployment's history is incomplete without the Build that
  // triggered it, which getUpdateQuery already handles - so this asks
  // for the deployment only to learn its build id, exactly as the
  // existing section does.
  const deployment = useRead(
    "GetDeployment",
    { deployment: id },
    { enabled: type === "Deployment" },
  ).data;
  const buildId =
    deployment?.config?.image?.type === "Build"
      ? deployment.config.image.params.build_id
      : undefined;

  const query = useMemo(
    () => getUpdateQuery({ type, id }, buildId),
    [type, id, buildId],
  );

  const result = useRead(
    "ListUpdates",
    { query, page },
    {
      enabled: !!query,
      // Without this the list blanks on every page change, which reads
      // as "the history vanished" rather than "loading".
      placeholderData: (previous) => previous,
    },
  ).data;

  const updates = result?.updates ?? [];
  // next_page is the only signal available - the API returns no total -
  // so the control shows "there is more" rather than a page count it
  // cannot know.
  const hasNext = typeof result?.next_page === "number";

  return (
    <Section
      title="History"
      icon={<ICONS.Update size="1.3rem" />}
      actions={
        <Text size="xs" c="dimmed">
          {updates.length > 0 &&
            `page ${page + 1}${hasNext ? "" : " (end)"}`}
        </Text>
      }
      forceHeaderGroup
      withBorder
    >
      <Stack gap="xs">
        {updates.length === 0 && (
          <Center c="dimmed" py="lg">
            <Text size="sm">
              {page === 0
                ? "No history yet for this resource."
                : "No further history."}
            </Text>
          </Center>
        )}
        <Stack gap={0}>
          {updates.map((update, i) => (
            <UpdateCard
              key={update.id}
              update={update}
              accent={i % 2 === 0}
              large
            />
          ))}
        </Stack>
        {(hasNext || page > 0) && (
          <Group justify="center" pt="xs">
            <Pagination
              size="sm"
              // total is a lower bound, not a count: the API reports
              // only whether another page exists.
              total={hasNext ? page + 2 : page + 1}
              value={page + 1}
              onChange={(next) => setPage(next - 1)}
              withEdges={false}
            />
          </Group>
        )}
      </Stack>
    </Section>
  );
}
