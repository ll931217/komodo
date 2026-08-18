//! Choosing a git credential by URL prefix.
//!
//! Komodo's original lookup is exact `domain` + `username`: the resource
//! names the account it wants. That works, but it means every repo under
//! a self-hosted group has to name the same credential individually, and
//! adding a repo means remembering to set it.
//!
//! A prefix match lets one credential serve a path subtree. It is
//! deliberately a FALLBACK: an account named on the resource always
//! wins, so no existing configuration changes behaviour by upgrading.
//! Prefix matching only fires where today's answer is "no credential at
//! all".

/// Why a prefix lookup produced no single answer.
///
/// Ambiguity is reported rather than resolved. Two accounts configured
/// with the same prefix is a configuration mistake, and silently picking
/// one would mean a Komodo upgrade or a config reorder could change
/// which credential reaches a remote without anyone touching the repo.
/// The caller logs this and falls back to anonymous, which fails
/// visibly.
#[derive(Debug, PartialEq, Eq)]
pub enum PrefixMatchError {
  /// More than one account matched with the same prefix length.
  /// Carries the competing usernames, sorted, so the message is stable.
  Ambiguous {
    prefix: String,
    usernames: Vec<String>,
  },
}

impl std::fmt::Display for PrefixMatchError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      PrefixMatchError::Ambiguous { prefix, usernames } => write!(
        f,
        "{} git accounts are configured with the same path prefix {prefix:?} ({}); \
         Komodo will not guess which one to use - give the resource an explicit \
         git account, or make the prefixes distinct",
        usernames.len(),
        usernames.join(", "),
      ),
    }
  }
}

/// Implemented so callers can attach context with anyhow. Without it,
/// `with_context` silently does not apply and the compile error points at
/// the call site rather than here.
impl std::error::Error for PrefixMatchError {}

/// A configured candidate: the account username, and the repo-path
/// prefix it covers. An empty prefix never matches - it means "this
/// account is only reachable by naming it", which is every account
/// today.
#[derive(Debug, Clone, Copy)]
pub struct PrefixCandidate<'a> {
  pub username: &'a str,
  pub path_prefix: &'a str,
}

/// Split a repo path into segments, dropping empties so leading,
/// trailing and doubled slashes all normalise to the same thing.
fn segments(path: &str) -> Vec<&str> {
  path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Select the account whose `path_prefix` is the longest segment-wise
/// match for `repo_path`.
///
/// Matching is on SEGMENT boundaries, not raw string prefix. A raw
/// `starts_with` would let a prefix of `infra` match `infra-secrets/x`,
/// handing one team's credential to a different team's repo because the
/// names happen to share leading characters. That is a credential leak
/// produced by a one-line convenience, so the boundary rule is the whole
/// point of this function rather than a refinement of it.
pub fn select_by_prefix<'a>(
  candidates: &[PrefixCandidate<'a>],
  repo_path: &str,
) -> Result<Option<&'a str>, PrefixMatchError> {
  let repo = segments(repo_path);
  if repo.is_empty() {
    return Ok(None);
  }

  let mut best_len = 0usize;
  let mut winners: Vec<&'a str> = Vec::new();

  for candidate in candidates {
    let prefix = segments(candidate.path_prefix);
    // An unset prefix opts out of prefix matching entirely.
    if prefix.is_empty() {
      continue;
    }
    if prefix.len() > repo.len() {
      continue;
    }
    if repo[..prefix.len()] != prefix[..] {
      continue;
    }
    if prefix.len() > best_len {
      best_len = prefix.len();
      winners.clear();
      winners.push(candidate.username);
    } else if prefix.len() == best_len {
      winners.push(candidate.username);
    }
  }

  match winners.len() {
    0 => Ok(None),
    1 => Ok(Some(winners[0])),
    _ => {
      // Same prefix length can still be different prefixes, but from
      // the repo's point of view they matched equally, so it is
      // ambiguous either way. Report the matched depth of the repo path.
      let mut usernames: Vec<String> =
        winners.iter().map(|u| u.to_string()).collect();
      usernames.sort();
      usernames.dedup();
      if usernames.len() == 1 {
        // The same account listed twice is redundant config, not a
        // conflict - there is only one credential it could mean.
        return Ok(Some(winners[0]));
      }
      Err(PrefixMatchError::Ambiguous {
        prefix: repo[..best_len].join("/"),
        usernames,
      })
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn candidates<'a>(
    pairs: &[(&'a str, &'a str)],
  ) -> Vec<PrefixCandidate<'a>> {
    pairs
      .iter()
      .map(|(username, path_prefix)| PrefixCandidate {
        username,
        path_prefix,
      })
      .collect()
  }

  #[test]
  fn the_longest_matching_prefix_wins() {
    let c = candidates(&[
      ("broad", "infra"),
      ("narrow", "infra/komodo"),
      ("unrelated", "apps"),
    ]);
    assert_eq!(
      select_by_prefix(&c, "infra/komodo/core"),
      Ok(Some("narrow"))
    );
    // Still under `infra`, but not under the narrower prefix.
    assert_eq!(
      select_by_prefix(&c, "infra/other"),
      Ok(Some("broad"))
    );
  }

  /// The reason this is segment-wise. A raw starts_with would match
  /// here and hand the `infra` credential to a different group.
  #[test]
  fn a_prefix_does_not_match_a_similarly_named_sibling() {
    let c = candidates(&[("infra", "infra")]);
    assert_eq!(
      select_by_prefix(&c, "infra-secrets/vault"),
      Ok(None),
      "'infra' must not match 'infra-secrets' - that leaks a credential \
       across groups whose names merely share leading characters"
    );
  }

  /// A prefix equal to the whole path is a match, not an off-by-one
  /// exclusion.
  #[test]
  fn a_prefix_matching_the_entire_path_matches() {
    let c = candidates(&[("exact", "infra/komodo")]);
    assert_eq!(
      select_by_prefix(&c, "infra/komodo"),
      Ok(Some("exact"))
    );
  }

  /// A prefix deeper than the repo path cannot match.
  #[test]
  fn a_prefix_longer_than_the_path_does_not_match() {
    let c = candidates(&[("deep", "infra/komodo/core")]);
    assert_eq!(select_by_prefix(&c, "infra/komodo"), Ok(None));
  }

  #[test]
  fn slashes_normalise_so_config_style_does_not_matter() {
    let c = candidates(&[("a", "/infra/komodo/")]);
    assert_eq!(
      select_by_prefix(&c, "infra//komodo/core"),
      Ok(Some("a"))
    );
  }

  /// Every account today has no prefix. Those must never be selected
  /// implicitly, or upgrading would start sending a credential to repos
  /// that previously cloned anonymously.
  #[test]
  fn an_empty_prefix_never_matches() {
    let c = candidates(&[("legacy", ""), ("also_legacy", "  ")]);
    assert_eq!(select_by_prefix(&c, "infra/komodo"), Ok(None));
  }

  #[test]
  fn an_empty_repo_path_matches_nothing() {
    let c = candidates(&[("broad", "infra")]);
    assert_eq!(select_by_prefix(&c, ""), Ok(None));
    assert_eq!(select_by_prefix(&c, "///"), Ok(None));
  }

  /// Two different accounts claiming the same depth is a config error.
  /// Reported, not resolved - guessing would let a config reorder
  /// change which credential reaches a remote.
  #[test]
  fn equal_length_competing_prefixes_are_ambiguous_not_arbitrary() {
    let c = candidates(&[("team_a", "infra"), ("team_b", "infra")]);
    assert_eq!(
      select_by_prefix(&c, "infra/komodo"),
      Err(PrefixMatchError::Ambiguous {
        prefix: "infra".to_string(),
        usernames: vec!["team_a".to_string(), "team_b".to_string()],
      })
    );
  }

  /// Different prefixes of the same depth both matching is equally
  /// ambiguous from the repo's point of view - which cannot actually
  /// happen for a single path, so this pins that reasoning rather than a
  /// reachable case.
  #[test]
  fn the_same_account_listed_twice_is_redundant_not_ambiguous() {
    let c = candidates(&[("solo", "infra"), ("solo", "infra")]);
    assert_eq!(
      select_by_prefix(&c, "infra/komodo"),
      Ok(Some("solo"))
    );
  }

  #[test]
  fn no_candidates_at_all_is_not_an_error() {
    assert_eq!(select_by_prefix(&[], "infra/komodo"), Ok(None));
  }

  /// The error text has to name the competing accounts and tell the
  /// operator what to do; a bare "ambiguous" sends them reading source.
  #[test]
  fn the_ambiguity_message_names_the_accounts_and_the_fix() {
    let err = PrefixMatchError::Ambiguous {
      prefix: "infra".to_string(),
      usernames: vec!["team_a".to_string(), "team_b".to_string()],
    };
    let text = err.to_string();
    assert!(text.contains("team_a") && text.contains("team_b"));
    assert!(text.contains("infra"));
    assert!(
      text.contains("explicit git account"),
      "the message must state the fix, got: {text}"
    );
  }
}
