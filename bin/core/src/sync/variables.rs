use std::collections::HashMap;

use anyhow::Context;
use database::mungos::find::find_collect;
use formatting::{Color, bold, colored, muted};
use komodo_client::{
  api::write::*,
  entities::{
    sync::DiffData, update::Log, user::sync_user, variable::Variable,
  },
};
use mogh_resolver::Resolve;

use crate::{api::write::WriteArgs, state::db_client};

use super::toml::TOML_PRETTY_OPTIONS;

pub fn variable_to_toml(
  variable: &Variable,
) -> anyhow::Result<String> {
  let inner = toml_pretty::to_string(variable, TOML_PRETTY_OPTIONS)
    .context("failed to serialize variable to toml")?;
  Ok(format!("[[variable]]\n{inner}"))
}

/// Stand-ins for a secret Variable's value in a pending sync diff.
///
/// The diff is persisted to `ResourceSync.info.variable_updates` and returned
/// by `GetResourceSync` to anyone holding Read on that sync — a far lower bar
/// than the `user.admin` gate on [GetVariable]. So the value can never be put
/// in it, not even for an admin: the stored document has no caller to check.
///
/// The marker states whether the value changed, rather than masking to a
/// fixed string, because the diff's whole job is to say what a run would do.
/// Masking both sides identically would render a value change as an empty
/// diff. Length is deliberately not preserved (no `"#".repeat(len)`) — that
/// leaks the length and still shows same-length edits as no change.
const SECRET_MASK: &str = "<secret>";
const SECRET_MASK_CHANGED: &str = "<secret - will change>";

/// [variable_to_toml], with the value masked when it is secret.
///
/// `secret` is the OR of both sides of a comparison, not one variable's own
/// flag: current and proposed are shown side by side, so revealing a
/// not-yet-secret proposed value would expose the secret current one
/// whenever the two are equal.
fn variable_to_diff_toml(
  variable: &Variable,
  secret: bool,
  changed: bool,
) -> anyhow::Result<String> {
  if !secret {
    return variable_to_toml(variable);
  }
  let mut masked = variable.clone();
  masked.value = if changed {
    SECRET_MASK_CHANGED
  } else {
    SECRET_MASK
  }
  .to_string();
  variable_to_toml(&masked)
}

pub struct ToUpdateItem {
  pub variable: Variable,
  pub update_value: bool,
  pub update_description: bool,
  pub update_is_secret: bool,
}

pub async fn get_updates_for_view(
  variables: &[Variable],
  delete: bool,
) -> anyhow::Result<Vec<DiffData>> {
  let map = find_collect(&db_client().variables, None, None)
    .await
    .context("failed to query db for variables")?
    .into_iter()
    .map(|v| (v.name.clone(), v))
    .collect::<HashMap<_, _>>();

  let mut diffs = Vec::<DiffData>::new();

  if delete {
    for variable in map.values() {
      if !variables.iter().any(|v| v.name == variable.name) {
        diffs.push(DiffData::Delete {
          current: variable_to_diff_toml(
            variable,
            variable.is_secret,
            false,
          )?,
        });
      }
    }
  }

  for variable in variables {
    match map.get(&variable.name) {
      Some(original) => {
        if original.value == variable.value
          && original.description == variable.description
        {
          continue;
        }
        // Either side being secret masks both — see [variable_to_diff_toml].
        let secret = original.is_secret || variable.is_secret;
        let changed = original.value != variable.value;
        diffs.push(DiffData::Update {
          proposed: variable_to_diff_toml(variable, secret, changed)?,
          current: variable_to_diff_toml(original, secret, false)?,
        });
      }
      None => {
        diffs.push(DiffData::Create {
          name: variable.name.clone(),
          proposed: variable_to_diff_toml(
            variable,
            variable.is_secret,
            false,
          )?,
        });
      }
    }
  }

  Ok(diffs)
}

pub async fn get_updates_for_execution(
  variables: Vec<Variable>,
  delete: bool,
) -> anyhow::Result<(Vec<Variable>, Vec<ToUpdateItem>, Vec<String>)> {
  let map = find_collect(&db_client().variables, None, None)
    .await
    .context("failed to query db for variables")?
    .into_iter()
    .map(|v| (v.name.clone(), v))
    .collect::<HashMap<_, _>>();

  let mut to_create = Vec::<Variable>::new();
  let mut to_update = Vec::<ToUpdateItem>::new();
  let mut to_delete = Vec::<String>::new();

  if delete {
    for variable in map.values() {
      if !variables.iter().any(|v| v.name == variable.name) {
        to_delete.push(variable.name.clone());
      }
    }
  }

  for variable in variables {
    match map.get(&variable.name) {
      Some(original) => {
        let item = ToUpdateItem {
          update_value: original.value != variable.value,
          update_description: original.description
            != variable.description,
          update_is_secret: original.is_secret != variable.is_secret,
          variable,
        };
        if !item.update_value
          && !item.update_description
          && !item.update_is_secret
        {
          continue;
        }

        to_update.push(item);
      }
      None => to_create.push(variable),
    }
  }

  Ok((to_create, to_update, to_delete))
}

pub async fn run_updates(
  to_create: Vec<Variable>,
  to_update: Vec<ToUpdateItem>,
  to_delete: Vec<String>,
) -> Option<Log> {
  if to_create.is_empty()
    && to_update.is_empty()
    && to_delete.is_empty()
  {
    return None;
  }

  let mut has_error = false;
  let mut log = String::from("running updates on Variables");

  for variable in to_create {
    if let Err(e) = (CreateVariable {
      name: variable.name.clone(),
      value: variable.value,
      description: variable.description,
      is_secret: variable.is_secret,
    })
    .resolve(&WriteArgs {
      user: sync_user().to_owned(),
    })
    .await
    {
      has_error = true;
      log.push_str(&format!(
        "\n{}: failed to create variable '{}' | {:#}",
        colored("ERROR", Color::Red),
        bold(&variable.name),
        e.error
      ));
    } else {
      log.push_str(&format!(
        "\n{}: {} variable '{}'",
        muted("INFO"),
        colored("created", Color::Green),
        bold(&variable.name)
      ))
    };
  }

  for ToUpdateItem {
    variable,
    update_value,
    update_description,
    update_is_secret,
  } in to_update
  {
    if update_value {
      if let Err(e) = (UpdateVariableValue {
        name: variable.name.clone(),
        value: variable.value,
      })
      .resolve(&WriteArgs {
        user: sync_user().to_owned(),
      })
      .await
      {
        has_error = true;
        log.push_str(&format!(
          "\n{}: failed to update variable value for '{}' | {:#}",
          colored("ERROR", Color::Red),
          bold(&variable.name),
          e.error
        ))
      } else {
        log.push_str(&format!(
          "\n{}: {} variable '{}' value",
          muted("INFO"),
          colored("updated", Color::Blue),
          bold(&variable.name)
        ))
      };
    }
    if update_description {
      if let Err(e) = (UpdateVariableDescription {
        name: variable.name.clone(),
        description: variable.description,
      })
      .resolve(&WriteArgs {
        user: sync_user().to_owned(),
      })
      .await
      {
        has_error = true;
        log.push_str(&format!(
          "\n{}: failed to update variable description for '{}' | {:#}",
          colored("ERROR", Color::Red),
          bold(&variable.name),
          e.error
        ))
      } else {
        log.push_str(&format!(
          "\n{}: {} variable '{}' description",
          muted("INFO"),
          colored("updated", Color::Blue),
          bold(&variable.name)
        ))
      };
    }
    if update_is_secret {
      if let Err(e) = (UpdateVariableIsSecret {
        name: variable.name.clone(),
        is_secret: variable.is_secret,
      })
      .resolve(&WriteArgs {
        user: sync_user().to_owned(),
      })
      .await
      {
        has_error = true;
        log.push_str(&format!(
          "\n{}: failed to update variable is secret for '{}' | {:#}",
          colored("ERROR", Color::Red),
          bold(&variable.name),
          e.error,
        ))
      } else {
        log.push_str(&format!(
          "\n{}: {} variable '{}' is secret",
          muted("INFO"),
          colored("updated", Color::Blue),
          bold(&variable.name)
        ))
      };
    }
  }

  for variable in to_delete {
    if let Err(e) = (DeleteVariable {
      name: variable.clone(),
    })
    .resolve(&WriteArgs {
      user: sync_user().to_owned(),
    })
    .await
    {
      has_error = true;
      log.push_str(&format!(
        "\n{}: failed to delete variable '{}' | {:#}",
        colored("ERROR", Color::Red),
        bold(&variable),
        e.error
      ))
    } else {
      log.push_str(&format!(
        "\n{}: {} variable '{}'",
        muted("INFO"),
        colored("deleted", Color::Red),
        bold(&variable)
      ))
    }
  }

  let stage = "Update Variables";
  Some(if has_error {
    Log::error(stage, log)
  } else {
    Log::simple(stage, log)
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn variable(name: &str, value: &str, is_secret: bool) -> Variable {
    Variable {
      name: name.to_string(),
      value: value.to_string(),
      description: String::new(),
      is_secret,
    }
  }

  /// The whole point: a secret's value must not reach a document that
  /// `GetResourceSync` hands out at Read permission.
  #[test]
  fn secret_value_never_lands_in_a_diff() {
    let secret = variable("TOKEN", "hunter2-the-real-value", true);
    let toml = variable_to_diff_toml(&secret, true, true).unwrap();
    assert!(
      !toml.contains("hunter2-the-real-value"),
      "secret value leaked into diff toml: {toml}"
    );
    assert!(toml.contains(SECRET_MASK_CHANGED), "{toml}");
  }

  /// A same-length edit is the case a length-preserving mask gets wrong:
  /// both sides render identically and the diff looks like a no-op.
  #[test]
  fn same_length_secret_change_is_still_visible() {
    let current = variable("TOKEN", "aaaaaaaa", true);
    let proposed = variable("TOKEN", "bbbbbbbb", true);
    let current_toml =
      variable_to_diff_toml(&current, true, false).unwrap();
    let proposed_toml =
      variable_to_diff_toml(&proposed, true, true).unwrap();
    assert_ne!(
      current_toml, proposed_toml,
      "a same-length secret change rendered as no diff"
    );
  }

  /// Masking must not leak the length it is hiding.
  #[test]
  fn mask_does_not_encode_length() {
    let short = variable("TOKEN", "a", true);
    let long = variable("TOKEN", &"a".repeat(200), true);
    assert_eq!(
      variable_to_diff_toml(&short, true, false).unwrap(),
      variable_to_diff_toml(&long, true, false).unwrap(),
      "mask length varies with the secret's length"
    );
  }

  /// Non-secret variables are the common case and must be untouched.
  #[test]
  fn non_secret_values_pass_through() {
    let plain = variable("LOG_LEVEL", "debug", false);
    let toml = variable_to_diff_toml(&plain, false, true).unwrap();
    assert!(toml.contains("debug"), "{toml}");
    assert_eq!(toml, variable_to_toml(&plain).unwrap());
  }

  /// A variable turning secret must mask the value it had while public,
  /// because current and proposed are displayed side by side.
  #[test]
  fn becoming_secret_masks_both_sides() {
    let current = variable("TOKEN", "was-public", false);
    let proposed = variable("TOKEN", "now-secret", true);
    let secret = current.is_secret || proposed.is_secret;
    let current_toml =
      variable_to_diff_toml(&current, secret, false).unwrap();
    let proposed_toml =
      variable_to_diff_toml(&proposed, secret, true).unwrap();
    assert!(!current_toml.contains("was-public"), "{current_toml}");
    assert!(!proposed_toml.contains("now-secret"), "{proposed_toml}");
  }
}
