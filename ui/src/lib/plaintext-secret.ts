/**
 * Detects credentials pasted directly into a resource config.
 *
 * Komodo encrypts Variables at rest, but resource configs (Stack
 * environment, Deployment env vars, Terraform variables, Build args)
 * are stored as written. The intended path for a secret is
 * `[[VARIABLE]]`, resolved at execution time from the encrypted
 * collection - nothing stops someone typing the password in directly.
 *
 * This is a guess, so it drives a warning and never a block.
 *
 * Self-check: `node ui/src/lib/plaintext-secret.check.ts`
 */

/**
 * Keys whose value is a credential often enough that the name alone is
 * a strong signal. Deliberately narrow - a list that also matched USER,
 * HOST or URL would fire on most environments, and a warning that fires
 * on everything is a warning nobody reads.
 */
const CREDENTIAL_KEY =
  /(^|_)(PASSWORD|PASSWD|PWD|SECRET|TOKEN|APIKEY|API_KEY|ACCESS_KEY|SECRET_KEY|PRIVATE_KEY|CREDENTIALS?)(_|$)/i;

/** Keys that look credential-shaped but name a location, not a value. */
const NOT_A_VALUE_KEY = /(_FILE|_PATH|_DIR|_ID|_NAME|_ENABLED)$/i;

/**
 * Value shapes that are a credential whatever the key is called: a
 * pasted PEM block, and a connection string carrying inline
 * `user:password@`.
 */
const PEM_BLOCK = /-----BEGIN [A-Z ]*PRIVATE KEY-----/;
const INLINE_CREDENTIAL_URL = /[a-z][a-z0-9+.-]*:\/\/[^\s:/@]+:[^\s@/]+@/i;

/**
 * Values that are a reference, not a literal: a Komodo interpolation
 * (`[[NAME]]`), its escape (`[[[NAME]]]`), and shell or compose style
 * `${NAME}` / `$NAME`, which resolve somewhere else entirely.
 */
const REFERENCE =
  /^(\[\[\[?[^\]]+\]?\]\]|\$\{[^}]+\}|\$[A-Za-z_][A-Za-z0-9_]*)$/;

/**
 * The keys in a `key_value` config whose value looks like a raw
 * credential rather than a reference to one.
 */
export function findPlaintextSecretKeys(environment: string): string[] {
  const flagged: string[] = [];

  for (const raw of environment.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;

    const separator = line.search(/[=:]/);
    if (separator < 1) continue;

    const key = line.slice(0, separator).trim();
    // Quoting does not make it any less of a pasted secret.
    const value = line
      .slice(separator + 1)
      .trim()
      .replace(/^(['"])([\s\S]*)\1$/, "$2")
      .trim();

    if (!value || REFERENCE.test(value)) continue;

    const byKey = CREDENTIAL_KEY.test(key) && !NOT_A_VALUE_KEY.test(key);
    const byValue = PEM_BLOCK.test(value) || INLINE_CREDENTIAL_URL.test(value);

    if (byKey || byValue) flagged.push(key);
  }

  return flagged;
}
