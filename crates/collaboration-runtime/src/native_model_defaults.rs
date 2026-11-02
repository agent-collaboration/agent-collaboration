//! Recognize the native picker's housekeeping save without persisting defaults.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelDefaultSaveRequest {
    edits: [ModelDefaultEdit; 2],
    file_path: Option<String>,
    expected_version: Option<String>,
    reload_user_config: bool,
}

#[derive(Deserialize)]
#[serde(tag = "keyPath", deny_unknown_fields)]
enum ModelDefaultEdit {
    #[serde(rename = "model")]
    Model {
        value: String,
        #[serde(rename = "mergeStrategy")]
        _merge_strategy: ReplaceOnly,
    },
    #[serde(rename = "model_reasoning_effort")]
    Effort {
        value: Value,
        #[serde(rename = "mergeStrategy")]
        _merge_strategy: ReplaceOnly,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ReplaceOnly {
    Replace,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum ModelDefaultWriteStatus {
    Ok,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModelDefaultSaveReceipt {
    status: ModelDefaultWriteStatus,
    version: &'static str,
    file_path: &'static str,
    overridden_metadata: Option<()>,
}

pub(super) fn model_default_save_receipt(params: &Value) -> Option<ModelDefaultSaveReceipt> {
    let request: ModelDefaultSaveRequest = serde_json::from_value(params.clone()).ok()?;
    if request.file_path.is_some()
        || request.expected_version.is_some()
        || !request.reload_user_config
    {
        return None;
    }
    let (model, effort) = match request.edits {
        [
            ModelDefaultEdit::Model { value: model, .. },
            ModelDefaultEdit::Effort { value: effort, .. },
        ]
        | [
            ModelDefaultEdit::Effort { value: effort, .. },
            ModelDefaultEdit::Model { value: model, .. },
        ] => (model, effort),
        _ => return None,
    };
    if !valid_default_value(&model)
        || !match effort {
            Value::Null => true,
            Value::String(value) => valid_default_value(&value),
            _ => false,
        }
    {
        return None;
    }
    Some(ModelDefaultSaveReceipt {
        status: ModelDefaultWriteStatus::Ok,
        version: "acp-compat-noop-v1",
        file_path: "/dev/null",
        overridden_metadata: None,
    })
}

fn valid_default_value(value: &str) -> bool {
    !value.is_empty() && !value.contains('\0')
}

#[cfg(test)]
#[path = "native_model_defaults_test.rs"]
mod tests;
