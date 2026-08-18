/**
 * Which source a Terraform resource actually runs from.
 *
 * Exactly one source wins, so every other source's config fields are
 * dead. The UI hides them rather than leaving them on screen inviting
 * edits the run will ignore.
 *
 * This MUST match `TerraformConfig::source_kind` in
 * `client/core/rs/src/entities/terraform.rs` - the backend picks the
 * source, this only decides what to draw. If they disagree the page
 * hides a field the run still reads, which is worse than showing a
 * dead one.
 *
 * Self-check: `node ui/src/lib/terraform-source.check.ts`
 */
export type TerraformSourceKind =
  | "FilesOnHost"
  | "LinkedRepo"
  | "Repo"
  | "Contents";

export function terraformSourceKind({
  files_on_host,
  linked_repo,
  repo,
}: {
  files_on_host?: boolean;
  linked_repo?: string;
  repo?: string;
}): TerraformSourceKind {
  if (files_on_host) return "FilesOnHost";
  if (linked_repo) return "LinkedRepo";
  if (repo) return "Repo";
  return "Contents";
}

/** Whether the active source clones a git repo, so `reclone` applies. */
export function terraformClones(kind: TerraformSourceKind): boolean {
  // Not inline-git-only: a linked Repo is cloned too, and
  // helpers/terraform.rs passes reclone through for it -
  // TerraformSource::Repo { reclone }.
  return kind === "LinkedRepo" || kind === "Repo";
}
