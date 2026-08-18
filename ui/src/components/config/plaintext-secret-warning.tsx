import { findPlaintextSecretKeys } from "@/lib/plaintext-secret";
import { Alert, Text } from "@mantine/core";

export interface PlaintextSecretWarningProps {
  /** The raw `key_value` editor contents. */
  value: string | undefined;
}

/**
 * Warns when a resource config appears to hold a pasted credential.
 *
 * Komodo encrypts Variables at rest, but resource configs are stored as
 * written - the intended path for a secret here is `[[VARIABLE]]`,
 * resolved at execution time. Nothing stops someone typing the password
 * in directly, and until now nothing said so either.
 *
 * A warning, never a block: the rule is a heuristic, and a heuristic
 * must not stop someone saving their own config.
 */
export default function PlaintextSecretWarning({
  value,
}: PlaintextSecretWarningProps) {
  const keys = findPlaintextSecretKeys(value ?? "");

  if (keys.length === 0) return null;

  return (
    <Alert color="yellow" title="Possible secret in plain text">
      <Text size="sm">
        {keys.join(", ")}{" "}
        {keys.length === 1 ? "looks like a credential" : "look like credentials"}
        . Resource configs are stored as written - define a Komodo Variable and
        reference it as{" "}
        <Text span ff="monospace">
          [[VARIABLE_NAME]]
        </Text>
        , which is kept encrypted and resolved at execution time.
      </Text>
    </Alert>
  );
}
