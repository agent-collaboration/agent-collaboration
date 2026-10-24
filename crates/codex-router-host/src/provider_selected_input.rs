//! Resolve ephemeral identity in the stored target scope before prompt admission.
use super::*;
use collaboration_protocol::ProviderKind;
use collaboration_service::{SelectedItem, provider_capabilities::*};

impl HostSessionCommandPort {
    pub(super) async fn prepare_native_selection(
        &self,
        command: SelectedPromptCommand,
    ) -> Result<SelectedPromptPreparation, CommandFailure> {
        if !matches!(command.selections.as_slice(), [SelectedItem::Skill { .. }])
            || !matches!(
                command.content.as_slice(),
                [collaboration_service::CommandContent::Text { .. }]
            )
        {
            return Ok(rejected(SelectionFailure::UnsupportedForm));
        }
        let target = protocol_session(&command.target)?;
        // Preserve the ordinary actor validation and delivery authorization path.
        let _actor = provider_actor(&command.actor)?;
        let stored = self.stored_session(&target).await?;
        let Some(binding) = self.supervisor.binding(&target.endpoint) else {
            return Ok(rejected(SelectionFailure::WrongContext));
        };
        let Some(runtime) = self.supervisor.runtime_for(&target.endpoint) else {
            return Ok(rejected(SelectionFailure::WrongContext));
        };
        if runtime.retirement().is_cancelled() {
            return Ok(rejected(SelectionFailure::WrongContext));
        }
        match runtime
            .session_activity(String::from(target.session_id.clone()))
            .await
            .map_err(|_| CommandFailure::ProviderFailure)?
        {
            crate::ProviderSessionActivity::Running => return Err(CommandFailure::Busy),
            crate::ProviderSessionActivity::NotLoaded => {
                return Ok(rejected(SelectionFailure::Unresolved));
            }
            crate::ProviderSessionActivity::Idle => {}
        }
        let cwd = CapabilityPath::new(std::path::PathBuf::from(String::from(
            stored.working_directory,
        )))
        .map_err(|_| CommandFailure::ProviderFailure)?;
        let query = CatalogueQuery::Workspace { cwd };
        let report = self
            .supervisor
            .read_catalogue(CatalogueRead {
                endpoint: command.target.endpoint.clone(),
                query: query.clone(),
                view: CatalogueView::FilesystemCatalogues,
                refresh_native: true,
                actor: command.actor.clone(),
            })
            .await;
        let CatalogueResult::Ready { report } = report else {
            return Ok(rejected(SelectionFailure::Unresolved));
        };
        if runtime.retirement().is_cancelled()
            || self.supervisor.binding(&target.endpoint).as_ref() != Some(&binding)
            || report.scope.binding != binding
            || report.scope.query != query
            || report.scope.view != CatalogueView::FilesystemCatalogues
        {
            return Ok(rejected(SelectionFailure::WrongContext));
        }
        let CatalogueSection::Supported {
            items,
            enumeration: SourceEnumeration::Accounted,
            errors,
        } = &report.skills
        else {
            return Ok(rejected(SelectionFailure::Unresolved));
        };
        if !errors.is_empty() {
            return Ok(rejected(SelectionFailure::Unresolved));
        }
        let attachment = self
            .hub
            .attach(command.target)
            .await
            .map_err(|_| CommandFailure::SessionNotFound)?;
        let Some(commands) = latest_commands(&attachment.snapshot) else {
            return Ok(rejected(SelectionFailure::Unresolved));
        };
        Ok(
            match lower_selection(
                &command.content,
                &command.selections,
                items,
                &commands,
                binding.runtime.provider,
            ) {
                Ok(content) => SelectedPromptPreparation::Ready { content },
                Err(reason) => rejected(reason),
            },
        )
    }
}

fn rejected(reason: SelectionFailure) -> SelectedPromptPreparation {
    SelectedPromptPreparation::Rejected { reason }
}

fn latest_commands(snapshot: &[collaboration_service::HubEvent]) -> Option<Vec<String>> {
    snapshot
        .iter()
        .filter_map(|event| {
            let item = match &event.event {
                SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } => item,
                _ => return None,
            };
            collaboration_service::retained_available_command_names(item)
                .map(|commands| (event.sequence, commands))
        })
        .max_by_key(|(sequence, _)| *sequence)
        .map(|(_, commands)| commands)
}

fn lower_selection(
    content: &[collaboration_service::CommandContent],
    selections: &[SelectedItem],
    skills: &[NativeSkill],
    commands: &[String],
    provider: ProviderKind,
) -> Result<Vec<collaboration_service::CommandContent>, SelectionFailure> {
    let (
        [collaboration_service::CommandContent::Text { text }],
        [
            SelectedItem::Skill {
                submitted_name,
                submitted_path,
            },
        ],
    ) = (content, selections)
    else {
        return Err(SelectionFailure::UnsupportedForm);
    };
    let mut matching = skills.iter().filter(|skill| skill.name == *submitted_name);
    let skill = matching.next().ok_or(SelectionFailure::Unresolved)?;
    if matching.next().is_some() {
        return Err(SelectionFailure::Ambiguous);
    }
    if !matches!(&skill.origin, NativeOrigin::Filesystem { actual_path } if actual_path == submitted_path)
    {
        return Err(SelectionFailure::WrongContext);
    }
    match skill.availability {
        NativeAvailability::Available => {}
        NativeAvailability::Disabled => return Err(SelectionFailure::Disabled),
        NativeAvailability::Unknown | NativeAvailability::Unavailable => {
            return Err(SelectionFailure::Unresolved);
        }
    }
    let NativeSkillLowering::Qualified { canonical_command } = &skill.lowering else {
        return Err(SelectionFailure::UnsupportedForm);
    };
    match commands
        .iter()
        .filter(|command| command.as_str() == canonical_command.as_str())
        .count()
    {
        1 => {}
        0 => return Err(SelectionFailure::Unresolved),
        _ => return Err(SelectionFailure::Ambiguous),
    }
    let token = format!("${}", submitted_name.as_str());
    let arguments = text
        .as_str()
        .strip_prefix(&token)
        .filter(|arguments| arguments.is_empty() || arguments.starts_with(char::is_whitespace))
        .ok_or(SelectionFailure::UnsupportedForm)?;
    if skills
        .iter()
        .any(|listed| contains_token(arguments, &format!("${}", listed.name.as_str())))
    {
        return Err(SelectionFailure::UnsupportedForm);
    }
    if provider == ProviderKind::Cursor
        && arguments.split_whitespace().any(|word| {
            let Some(reference) = word.strip_prefix('/') else {
                return false;
            };
            let name = reference
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
                .collect::<String>();
            !name.is_empty()
                && (commands.contains(&name)
                    || skills.iter().any(|skill| skill.name.as_str() == name))
        })
    {
        return Err(SelectionFailure::UnsupportedForm);
    }
    let lowered = format!("/{}{arguments}", canonical_command.as_str());
    Ok(vec![
        collaboration_service::CommandContent::text(lowered)
            .map_err(|_| SelectionFailure::UnsupportedForm)?,
    ])
}

fn contains_token(text: &str, token: &str) -> bool {
    text.match_indices(token).any(|(offset, _)| {
        let before = text
            .get(..offset)
            .and_then(|prefix| prefix.chars().next_back());
        let after = text
            .get(offset + token.len()..)
            .and_then(|suffix| suffix.chars().next());
        before.is_none_or(char::is_whitespace) && after.is_none_or(char::is_whitespace)
    })
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
#[path = "provider_selected_input_tests.rs"]
mod tests;
