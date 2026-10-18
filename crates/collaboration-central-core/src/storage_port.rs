use collaboration_replication_protocol::SqlValue;

#[derive(Clone, Debug, PartialEq)]
enum ReadProjection {
    Columns(Vec<String>),
    RowExistence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoundStatement {
    sql: String,
    parameters: Vec<BoundValue>,
    read_projection: Option<ReadProjection>,
}

impl BoundStatement {
    pub(crate) fn new(sql: impl Into<String>, parameters: Vec<SqlValue>) -> Self {
        Self {
            sql: sql.into(),
            parameters: parameters.into_iter().map(BoundValue::from).collect(),
            read_projection: None,
        }
    }

    pub(crate) fn query(
        sql: impl Into<String>,
        parameters: Vec<SqlValue>,
        columns: Vec<String>,
    ) -> Self {
        let mut statement = Self::new(sql, parameters);
        statement.read_projection = Some(ReadProjection::Columns(columns));
        statement
    }
    pub fn sql(&self) -> Result<String, StoreError> {
        let encoded = crate::bound_sql::encode_bindings(&self.sql, &self.parameters)?;
        Ok(match &self.read_projection {
            Some(ReadProjection::Columns(columns)) => {
                crate::bound_sql::project_transport_columns(&encoded, columns)
            }
            Some(ReadProjection::RowExistence) | None => encoded,
        })
    }
    pub(crate) fn existence(sql: impl Into<String>, parameters: Vec<SqlValue>) -> Self {
        let mut statement = Self::new(sql, parameters);
        statement.read_projection = Some(ReadProjection::RowExistence);
        statement
    }
    #[must_use]
    pub fn reads_values(&self) -> bool {
        matches!(self.read_projection, Some(ReadProjection::Columns(_)))
    }
    pub fn decode_row(&self, raw: Vec<(String, BoundValue)>) -> Result<SqlRow, StoreError> {
        let columns = match &self.read_projection {
            Some(ReadProjection::Columns(columns)) => columns,
            Some(ReadProjection::RowExistence) if raw.is_empty() => {
                return SqlRow::from_transport(Vec::new());
            }
            _ => {
                return Err(StoreError::Corrupt {
                    field: "query read plan".into(),
                });
            }
        };
        if raw.len() != columns.len() * 2 {
            return Err(StoreError::Corrupt {
                field: "query column cardinality".into(),
            });
        }
        let mut decoded = Vec::with_capacity(columns.len());
        for (index, name) in columns.iter().enumerate() {
            let find = |key: &str| -> Result<&BoundValue, StoreError> {
                let mut values = raw
                    .iter()
                    .filter(|(column, _)| column == key)
                    .map(|(_, value)| value);
                let value = values
                    .next()
                    .ok_or_else(|| StoreError::Corrupt { field: key.into() })?;
                if values.next().is_some() {
                    return Err(StoreError::Corrupt { field: key.into() });
                }
                Ok(value)
            };
            let kind = find(&format!("_copy_type_{index}"))?;
            let value = find(&format!("_copy_value_{index}"))?;
            let encoded = match (kind, value) {
                (BoundValue::Text(kind), BoundValue::Text(value)) if kind == "integer" => {
                    BoundValue::IntegerText(value.clone())
                }
                (BoundValue::Text(kind), BoundValue::Real(value)) if kind == "real" => {
                    BoundValue::Real(*value)
                }
                (BoundValue::Text(kind), BoundValue::Text(value)) if kind == "text" => {
                    BoundValue::Text(value.clone())
                }
                (BoundValue::Text(kind), BoundValue::Blob(value)) if kind == "blob" => {
                    BoundValue::Blob(value.clone())
                }
                (BoundValue::Text(kind), BoundValue::Null) if kind == "null" => BoundValue::Null,
                _ => {
                    return Err(StoreError::Corrupt {
                        field: name.clone(),
                    });
                }
            };
            decoded.push((name.clone(), encoded));
        }
        SqlRow::from_transport(decoded)
    }

    #[must_use]
    pub fn parameters(&self) -> &[BoundValue] {
        &self.parameters
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BoundValue {
    IntegerText(String),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Null,
}
impl From<SqlValue> for BoundValue {
    fn from(value: SqlValue) -> Self {
        match value {
            SqlValue::Integer(value) => Self::IntegerText(value.to_string()),
            SqlValue::Real(value) => Self::Real(value),
            SqlValue::Text(value) => Self::Text(value),
            SqlValue::Blob(value) => Self::Blob(value),
            SqlValue::Null => Self::Null,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct SqlRow {
    columns: Vec<(String, SqlValue)>,
}

impl SqlRow {
    pub fn from_transport(columns: Vec<(String, BoundValue)>) -> Result<Self, StoreError> {
        let columns = columns
            .into_iter()
            .map(|(name, value)| {
                let value = match value {
                    BoundValue::IntegerText(value) => {
                        SqlValue::Integer(value.parse().map_err(|_| StoreError::Corrupt {
                            field: name.clone(),
                        })?)
                    }
                    BoundValue::Real(value) if value.is_finite() => SqlValue::Real(value),
                    BoundValue::Real(_) => return Err(StoreError::Corrupt { field: name }),
                    BoundValue::Text(value) => SqlValue::Text(value),
                    BoundValue::Blob(value) => SqlValue::Blob(value),
                    BoundValue::Null => SqlValue::Null,
                };
                Ok((name, value))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok(Self { columns })
    }
    pub fn value(&self, name: &str) -> Result<&SqlValue, StoreError> {
        self.columns
            .iter()
            .find_map(|(column, value)| (column == name).then_some(value))
            .ok_or_else(|| StoreError::Corrupt { field: name.into() })
    }

    pub fn text(&self, name: &str) -> Result<&str, StoreError> {
        match self.value(name)? {
            SqlValue::Text(value) => Ok(value),
            _ => Err(StoreError::Corrupt { field: name.into() }),
        }
    }

    pub fn integer(&self, name: &str) -> Result<i64, StoreError> {
        match self.value(name)? {
            SqlValue::Integer(value) => Ok(*value),
            _ => Err(StoreError::Corrupt { field: name.into() }),
        }
    }

    pub fn unsigned(&self, name: &str) -> Result<u64, StoreError> {
        self.text(name)?
            .parse()
            .map_err(|_| StoreError::Corrupt { field: name.into() })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StoreError {
    #[error("storage capacity exhausted")]
    Full,
    #[error("storage constraint: {message}")]
    Constraint { message: String },
    #[error("storage unavailable: {message}")]
    Unavailable { message: String },
    #[error("invalid stored field {field}")]
    Corrupt { field: String },
}

/// **Superseded (2026-10-16).** A synchronous storage interface was the Lead's inference, not the
/// owner's requirement. It is replaced by an asynchronous callback transaction scope over SQLx executors
/// (`docs/specs/2026-10-16-central-one-core/program-design.md`, section 5).
pub trait SqlTx {
    fn execute(&mut self, statement: BoundStatement) -> Result<u64, StoreError>;
    fn query(&mut self, statement: BoundStatement) -> Result<Vec<SqlRow>, StoreError>;
}
