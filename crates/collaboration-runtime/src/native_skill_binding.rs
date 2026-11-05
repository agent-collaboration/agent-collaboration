//! One native connection's explicit cwd bindings and immediate skill views.

use crate::native_skill_reference::encode_skill_reference;
use collaboration_contracts::SkillViewAttachment;
use message_board::SessionRef;
use serde_json::{Value, json};
use session_event_model::{ProviderSkillScope, SkillView};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};
use tokio::sync::watch;

pub(super) struct BoundSkillView {
    pub(super) cwd: PathBuf,
    pub(super) session: SessionRef,
    pub(super) attachment: SkillViewAttachment,
}

pub(super) struct BindingChange {
    pub(super) cwd: PathBuf,
    pub(super) watcher: Option<(SessionRef, watch::Receiver<SkillView>)>,
}

enum CwdBinding {
    Bound {
        session: SessionRef,
        receiver: watch::Receiver<SkillView>,
        notified: SkillView,
    },
    Ambiguous,
}
struct CompletedBinding {
    start: u64,
    end: u64,
    cwd: PathBuf,
}

#[derive(Default)]
enum ModelBinding {
    #[default]
    Unbound,
    Bound(SessionRef),
    Ambiguous,
}

#[derive(Default)]
pub(super) struct NativeSkillBindings {
    next_event: u64,
    pending: HashMap<u64, u64>,
    completed: Vec<CompletedBinding>,
    bindings: BTreeMap<PathBuf, CwdBinding>,
    model_binding: ModelBinding,
}

impl NativeSkillBindings {
    pub(super) fn model_session(&self) -> Option<&SessionRef> {
        match &self.model_binding {
            ModelBinding::Bound(session) => Some(session),
            ModelBinding::Unbound | ModelBinding::Ambiguous => None,
        }
    }
    pub(super) fn begin(&mut self) -> u64 {
        self.next_event += 1;
        let token = self.next_event;
        self.pending.insert(token, token);
        token
    }

    /// Successful overlapping requests remain ambiguous regardless of response order.
    pub(super) fn complete(
        &mut self,
        token: u64,
        bound: Option<BoundSkillView>,
    ) -> Option<BindingChange> {
        let start = self.pending.remove(&token)?;
        self.next_event += 1;
        let end = self.next_event;
        let changed = bound.and_then(|bound| {
            let cwd = bound.cwd;
            self.model_binding = if self
                .completed
                .iter()
                .any(|completed| completed.end > start && completed.start < end)
            {
                ModelBinding::Ambiguous
            } else {
                ModelBinding::Bound(bound.session.clone())
            };
            let overlapping = self.completed.iter().any(|completed| {
                completed.cwd == cwd && completed.end > start && completed.start < end
            });
            let previous = self.bindings.get(&cwd).map(binding_identity);
            let next = if overlapping {
                CwdBinding::Ambiguous
            } else {
                let notified = bound.attachment.receiver.borrow().clone();
                CwdBinding::Bound {
                    session: bound.session,
                    receiver: bound.attachment.receiver,
                    notified,
                }
            };
            let same = previous.as_ref() == Some(&binding_identity(&next));
            let watcher = match &next {
                CwdBinding::Bound {
                    session, receiver, ..
                } => Some((session.clone(), receiver.clone())),
                CwdBinding::Ambiguous => None,
            };
            self.bindings.insert(cwd.clone(), next);
            self.completed.push(CompletedBinding {
                start,
                end,
                cwd: cwd.clone(),
            });
            if same {
                None
            } else {
                Some(BindingChange { cwd, watcher })
            }
        });
        if let Some(oldest) = self.pending.values().min().copied() {
            self.completed.retain(|completed| completed.end > oldest);
        } else {
            self.completed.clear();
        }
        changed
    }

    pub(super) fn changed(&mut self, cwd: &PathBuf, session: &SessionRef) -> bool {
        let Some(CwdBinding::Bound {
            session: current,
            receiver,
            notified,
        }) = self.bindings.get_mut(cwd)
        else {
            return false;
        };
        if current != session {
            return false;
        }
        let latest = receiver.borrow().clone();
        if *notified == latest {
            return false;
        }
        *notified = latest;
        true
    }

    pub(super) fn list(&self, params: &Value) -> Result<Value, ()> {
        let cwds = match params.get("cwds") {
            None => Vec::new(),
            Some(Value::Array(cwds)) => cwds
                .iter()
                .map(|cwd| cwd.as_str().map(PathBuf::from).ok_or(()))
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err(()),
        };
        let cwds = if cwds.is_empty() && self.bindings.len() == 1 {
            self.bindings
                .iter()
                .filter_map(|(cwd, binding)| {
                    matches!(binding, CwdBinding::Bound { .. }).then_some(cwd.clone())
                })
                .collect()
        } else {
            cwds
        };
        let data = cwds.into_iter().map(|cwd| {
            let mut skills = Vec::new();
            let mut errors = Vec::new();
            match self.bindings.get(&cwd) {
                Some(CwdBinding::Ambiguous) => errors.push(json!({"path":cwd,"message":"provider session binding is ambiguous; explicitly start or resume the intended session"})),
                Some(CwdBinding::Bound { session, receiver, .. }) => match &*receiver.borrow() {
                    SkillView::Unknown => {},
                    SkillView::Unavailable(reason) => errors.push(json!({"path":cwd,"message":format!("provider skill source unavailable: {reason:?}")})),
                    SkillView::Available { skills: available, errors: row_errors } => {
                        skills = available.iter().map(|skill| json!({
                            "name":skill.invocation_name.as_str(),"description":skill.description,"path":encode_skill_reference(session,&skill.invocation_name),
                            "scope":match skill.scope { ProviderSkillScope::User=>"user",ProviderSkillScope::Repo=>"repo",ProviderSkillScope::System=>"system" },
                            "enabled":true,"shortDescription":null,"interface":null,"dependencies":null,"pluginId":null
                        })).collect();
                        errors.extend(row_errors.iter().map(|error| json!({"path":cwd,"message":format!("{}: {:?}",error.advertised_name,error.reason)})));
                    }
                },
                None => {},
            }
            json!({"cwd":cwd,"skills":skills,"errors":errors})
        }).collect::<Vec<_>>();
        Ok(json!({"data":data}))
    }
}

fn binding_identity(binding: &CwdBinding) -> Option<(SessionRef, SkillView)> {
    match binding {
        CwdBinding::Bound {
            session, notified, ..
        } => Some((session.clone(), notified.clone())),
        CwdBinding::Ambiguous => None,
    }
}

pub(super) async fn bound_response(
    response: &Value,
    events: &dyn collaboration_contracts::SessionEventHub,
    endpoint: &message_board::SessionEndpointRef,
) -> Option<BoundSkillView> {
    let thread_id = response.pointer("/result/thread/id")?.as_str()?;
    let summary = events
        .sessions(endpoint.clone())
        .await
        .ok()?
        .into_iter()
        .find(|summary| super::thread_alias(&summary.session) == thread_id)?;
    let cwd = summary.working_directory;
    let session = summary.session;
    let attachment = events.skill_view(session.clone(), cwd.clone()).await.ok()?;
    Some(BoundSkillView {
        cwd,
        session,
        attachment,
    })
}

#[cfg(test)]
#[path = "native_skill_binding_test.rs"]
mod tests;
