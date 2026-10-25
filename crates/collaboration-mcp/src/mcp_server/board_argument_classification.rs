//! Typed rejections for board tool arguments that do not decode.
//!
//! A board tool's arguments decode straight into the board request type. When they don't, the
//! decoder's own message can name supplied keys and variants, so the tool classifies the
//! arguments instead: the rejection names the field and its requirement and never repeats a
//! supplied value, key or variant.
use message_board::*;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{borrow::Cow, marker::PhantomData};

#[path = "board_variant_validation.rs"]
mod board_variant_validation;
use board_variant_validation::validate_closed_variants;

const IDENTITY_REQUIREMENT: &str = "must be a human identity with a 1 to 4096 byte humanId without NUL, or a session identity with a canonical service UUID, 1 to 64 byte lowercase endpointId, and 1 to 4096 byte sessionId without NUL";

/// Decodes a board tool's arguments, or classifies them into the board's typed rejection.
pub(super) fn decode_board_arguments<TRequest: DeserializeOwned>(
    tool: &str,
    arguments: Map<String, Value>,
) -> Result<TRequest, BoardError> {
    let arguments = Value::Object(arguments);
    TRequest::deserialize(&arguments).map_err(|_| classify_board_arguments(tool, &arguments))
}

/// A board tool's arguments left undecoded, for a tool whose handler decodes them itself.
/// It publishes `TRequest`'s schema as the tool's input schema.
pub(super) struct UndecodedBoardArguments<TRequest> {
    arguments: Map<String, Value>,
    request: PhantomData<fn() -> TRequest>,
}

impl<TRequest: DeserializeOwned> UndecodedBoardArguments<TRequest> {
    pub(super) fn decode(self, tool: &str) -> Result<TRequest, BoardError> {
        decode_board_arguments(tool, self.arguments)
    }
}

impl<'de, TRequest> Deserialize<'de> for UndecodedBoardArguments<TRequest> {
    fn deserialize<TDeserializer: Deserializer<'de>>(
        deserializer: TDeserializer,
    ) -> Result<Self, TDeserializer::Error> {
        Map::deserialize(deserializer).map(|arguments| Self {
            arguments,
            request: PhantomData,
        })
    }
}

impl<TRequest: JsonSchema> JsonSchema for UndecodedBoardArguments<TRequest> {
    fn inline_schema() -> bool {
        TRequest::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        TRequest::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        TRequest::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        TRequest::json_schema(generator)
    }
}

/// The typed rejection for `tool`'s `arguments`, which did not decode.
pub(super) fn classify_board_arguments(tool: &str, arguments: &Value) -> BoardError {
    classify_failure(tool, arguments).unwrap_or_else(|| malformed_request(tool))
}

fn classify_failure(tool: &str, arguments: &Value) -> Option<BoardError> {
    let Some(fields) = arguments.as_object() else {
        return Some(invalid_field("request", "must be a JSON object"));
    };
    if matches!(tool, "thread_create" | "thread_refile")
        && fields
            .get("about")
            .is_none_or(|about| serde_json::from_value::<ThreadSubject>(about.clone()).is_err())
    {
        return Some(BoardError {
            kind: BoardFailureKind::InvalidShape,
            ..invalid_field(
                "about",
                "must name a project subject line, workstream, task, work-trail row or workspace ADR",
            )
        });
    }
    validate_required_identities(tool, fields)
        .err()
        .or_else(|| validate_optional_identity(fields, "actor", false).err())
        .or_else(|| validate_reader(tool, fields).err())
        .or_else(|| validate_optional_identity(fields, "actingFor", true).err())
        .or_else(|| validate_known_uuid_v7_fields(fields).err())
        .or_else(|| validate_known_feed_positions(fields).err())
        .or_else(|| validate_metadata_fields(tool, fields).err())
        .or_else(|| validate_page(fields).err())
        .or_else(|| validate_repository(fields).err())
        .or_else(|| validate_closed_variants(tool, fields).err())
}

fn malformed_request(tool: &str) -> BoardError {
    invalid_field(
        "request",
        format!(
            "must contain the documented camelCase fields and supported kind variants for {tool}"
        ),
    )
}

fn validate_required_identities(tool: &str, fields: &Map<String, Value>) -> Result<(), BoardError> {
    const ACTOR_METHODS: &[&str] = &[
        "workspace_create",
        "workspace_update",
        "workspace_archive",
        "repository_attach",
        "repository_detach",
        "board_create",
        "board_update",
        "board_archive",
        "board_close",
        "project_create",
        "project_update",
        "thread_post",
        "thread_create",
        "thread_refile",
        "thread_join",
        "thread_leave",
        "thread_resolve",
        "thread_unresolve",
        "thread_watch",
        "thread_unwatch",
        "project_watch",
        "project_unwatch",
        "inbox_acknowledge",
        "thread_subscribe",
        "thread_unsubscribe",
        "thread_subscriptions",
        "thread_wait",
    ];
    const READER_METHODS: &[&str] = &["thread_list", "inbox_fetch", "inbox_workspaces"];
    if ACTOR_METHODS.contains(&tool) && fields.get("actor").is_none_or(Value::is_null) {
        return Err(invalid_identity("actor", IDENTITY_REQUIREMENT));
    }
    if READER_METHODS.contains(&tool) && fields.get("reader").is_none_or(Value::is_null) {
        return Err(invalid_identity("reader", IDENTITY_REQUIREMENT));
    }
    Ok(())
}

fn validate_optional_identity(
    fields: &Map<String, Value>,
    field: &'static str,
    acting_for: bool,
) -> Result<(), BoardError> {
    let Some(value) = fields.get(field) else {
        return Ok(());
    };
    if value.is_null() && field != "actor" {
        return Ok(());
    }
    let valid = if acting_for {
        serde_json::from_value::<ActingForIdentity>(value.clone()).is_ok()
    } else {
        serde_json::from_value::<Identity>(value.clone()).is_ok()
    };
    if !valid {
        return Err(invalid_identity(
            field,
            if acting_for {
                "must be null or a human identity with a 1 to 4096 byte humanId without NUL"
            } else {
                IDENTITY_REQUIREMENT
            },
        ));
    }
    Ok(())
}

fn validate_reader(tool: &str, fields: &Map<String, Value>) -> Result<(), BoardError> {
    if !matches!(tool, "thread_show" | "feed_read" | "work_trail_show") {
        return validate_optional_identity(fields, "reader", false);
    }
    let reader = fields
        .get("reader")
        .ok_or_else(|| invalid_field("reader", "must be anonymous or identified"))?;
    if reader.get("kind").and_then(Value::as_str) == Some("identified")
        && reader
            .get("reader")
            .is_none_or(|identity| serde_json::from_value::<Identity>(identity.clone()).is_err())
    {
        return Err(invalid_identity("reader.reader", IDENTITY_REQUIREMENT));
    }
    if serde_json::from_value::<ReadActor>(reader.clone()).is_err() {
        return Err(invalid_field(
            "reader",
            "must be anonymous or identified with its reader identity",
        ));
    }
    Ok(())
}

fn validate_known_uuid_v7_fields(fields: &Map<String, Value>) -> Result<(), BoardError> {
    for field in [
        "workspaceId",
        "boardId",
        "projectId",
        "postId",
        "rootPostId",
        "threadId",
    ] {
        validate_uuid_field(fields.get(field), field)?;
    }
    for (value, field) in [
        (fields.get("thread"), "thread"),
        (
            fields
                .get("placement")
                .and_then(|value| value.get("thread")),
            "placement.thread",
        ),
        (
            fields.get("scope").and_then(|value| value.get("thread")),
            "scope.thread",
        ),
    ] {
        if let Some(value) = value
            && serde_json::from_value::<ThreadRef>(value.clone()).is_err()
        {
            return Err(invalid_field(
                field,
                "must contain a thread UUIDv7 and optional project UUIDv7",
            ));
        }
    }
    if let Some(record) = fields.get("record") {
        validate_uuid_field(record.get("id"), "record.id")?;
        if record
            .get("kind")
            .is_none_or(|kind| serde_json::from_value::<RecordKind>(kind.clone()).is_err())
        {
            return Err(invalid_field(
                "record.kind",
                "must be a supported record kind",
            ));
        }
    }
    for (container_name, id_fields) in [
        ("placement", &["projectId", "rootPostId"][..]),
        (
            "scope",
            &["projectId", "rootPostId", "boardId", "workspaceId"][..],
        ),
    ] {
        if let Some(container) = fields.get(container_name).and_then(Value::as_object) {
            for field in id_fields {
                validate_uuid_field(container.get(*field), &format!("{container_name}.{field}"))?;
            }
        }
    }
    if let Some(references) = fields.get("references").and_then(Value::as_array) {
        for (index, reference) in references.iter().enumerate() {
            if let Some(reference) = reference.as_object() {
                for field in ["postId", "rootPostId"] {
                    validate_uuid_field(
                        reference.get(field),
                        &format!("references[{index}].{field}"),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_uuid_field(value: Option<&Value>, field: &str) -> Result<(), BoardError> {
    if let Some(value) = value
        && value
            .as_str()
            .and_then(|value| PostId::try_from(value.to_owned()).ok())
            .is_none()
    {
        return Err(invalid_field(
            field,
            "must be a canonical lowercase RFC UUIDv7",
        ));
    }
    Ok(())
}

fn validate_known_feed_positions(fields: &Map<String, Value>) -> Result<(), BoardError> {
    if let Some(selection) = fields.get("selection").and_then(Value::as_object) {
        for field in ["after", "from", "to"] {
            if let Some(position) = selection.get(field)
                && let Some(cursor) = position.get("cursor")
            {
                let prefix = format!("selection.{field}.cursor");
                if let Some(projects) = cursor.get("perProject").and_then(Value::as_object) {
                    for cursor in projects.values() {
                        validate_feed_cursor(cursor, &prefix)?;
                    }
                } else {
                    validate_feed_cursor(cursor, &prefix)?;
                }
            }
        }
    }
    for field in ["after", "through", "before", "rangeTop"] {
        if let Some(cursor) = fields.get(field) {
            validate_feed_cursor(cursor, field)?;
        }
    }
    if let Some(entries) = fields.get("entries").and_then(Value::as_array) {
        for (index, entry) in entries.iter().enumerate() {
            if let Some(cursor) = entry.get("through") {
                validate_feed_cursor(cursor, &format!("entries[{index}].through"))?;
            }
        }
    }
    Ok(())
}
fn validate_feed_cursor(value: &Value, field: &str) -> Result<(), BoardError> {
    if let Some(position) = value.get("position")
        && position
            .as_u64()
            .and_then(|value| FeedPosition::try_from(value).ok())
            .is_none()
    {
        return Err(invalid_field(
            format!("{field}.position"),
            "must be an integer from 0 through 9223372036854775807",
        ));
    }
    if let Some(epoch) = value.get("epoch")
        && epoch
            .as_u64()
            .and_then(|value| EpochNumber::try_from(value).ok())
            .is_none()
    {
        return Err(invalid_field(
            format!("{field}.epoch"),
            "must be an integer from 1 through 9223372036854775807",
        ));
    }
    Ok(())
}

fn validate_metadata_fields(tool: &str, fields: &Map<String, Value>) -> Result<(), BoardError> {
    let project_metadata = matches!(tool, "project_create" | "project_update");
    let field = if project_metadata { "title" } else { "name" };
    if let Some(value) = fields.get(field) {
        let valid = value
            .as_str()
            .and_then(|value| ResourceName::try_from(value.to_owned()).ok())
            .is_some();
        if !valid {
            if project_metadata {
                return Err(invalid_project_name(
                    field,
                    "must trim to 1 through 256 UTF-8 bytes and be unique within its board",
                ));
            }
            return Err(invalid_field(
                field,
                "must trim to 1 through 256 UTF-8 bytes",
            ));
        }
    } else if project_metadata {
        return Err(invalid_project_name(
            field,
            "is required and must trim to 1 through 256 UTF-8 bytes",
        ));
    }
    if let Some(value) = fields.get("description")
        && value
            .as_str()
            .and_then(|value| Description::try_from(value.to_owned()).ok())
            .is_none()
    {
        return Err(invalid_field(
            "description",
            "must contain at most 16384 UTF-8 bytes",
        ));
    }
    if let Some(value) = fields.get("text")
        && value
            .as_str()
            .and_then(|value| PostText::try_from(value.to_owned()).ok())
            .is_none()
    {
        return Err(invalid_field(
            "text",
            "must contain 1 through 65536 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn validate_page(fields: &Map<String, Value>) -> Result<(), BoardError> {
    let Some(page) = fields.get("page") else {
        return Ok(());
    };
    if let Some(limit) = page.get("limit")
        && limit
            .as_u64()
            .and_then(|limit| u32::try_from(limit).ok())
            .and_then(|limit| PageLimit::try_from(limit).ok())
            .is_none()
    {
        return Err(invalid_field(
            "page.limit",
            "must be an integer from 1 through 100",
        ));
    }
    if serde_json::from_value::<PageRequest>(page.clone()).is_err() {
        return Err(invalid_field(
            "page",
            "must contain only a bounded limit and optional opaque cursor",
        ));
    }
    Ok(())
}

fn validate_repository(fields: &Map<String, Value>) -> Result<(), BoardError> {
    let Some(repository) = fields.get("repository") else {
        return Ok(());
    };
    if repository.is_null() {
        return Ok(());
    }
    if let Some(repository_fields) = repository.as_object() {
        for (field, constructor_valid) in [
            (
                "normalizedOrigin",
                repository_fields
                    .get("normalizedOrigin")
                    .and_then(Value::as_str)
                    .map(|value| NormalizedOrigin::try_from(value.to_owned()).is_ok()),
            ),
            (
                "commonDirectory",
                repository_fields
                    .get("commonDirectory")
                    .and_then(Value::as_str)
                    .map(|value| CommonDirectory::try_from(value.to_owned()).is_ok()),
            ),
        ] {
            if constructor_valid == Some(false) {
                return Err(invalid_field(
                    format!("repository.{field}"),
                    "must be canonical and contain at most 4096 UTF-8 bytes",
                ));
            }
        }
    }
    if serde_json::from_value::<RepositoryRef>(repository.clone()).is_err() {
        return Err(invalid_field(
            "repository",
            "must be a canonical origin or local repository reference",
        ));
    }
    Ok(())
}

fn invalid_field(field: impl Into<String>, requirement: impl Into<String>) -> BoardError {
    BoardError::invalid_field(field, requirement)
}

fn invalid_identity(field: &'static str, requirement: &'static str) -> BoardError {
    BoardError {
        kind: BoardFailureKind::InvalidIdentity,
        stage: BoardFailureStage::Validation,
        message: format!("Correct {field}: {requirement}."),
        next_action: BoardNextAction::CorrectRequest,
        details: BoardErrorDetails::FieldConstraint {
            field: field.to_owned(),
            requirement: requirement.to_owned(),
        },
    }
}

fn invalid_project_name(field: &'static str, requirement: &'static str) -> BoardError {
    BoardError {
        kind: BoardFailureKind::InvalidProjectName,
        stage: BoardFailureStage::Validation,
        message: format!("Correct {field}: {requirement}."),
        next_action: BoardNextAction::SelectDifferentName,
        details: BoardErrorDetails::FieldConstraint {
            field: field.to_owned(),
            requirement: requirement.to_owned(),
        },
    }
}
