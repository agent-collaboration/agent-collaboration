use std::fmt::{self, Debug, Formatter};
use std::future::{self, Future};
use std::str::FromStr;
use std::time::Duration;

use crate::database::{DoConnectOptions, DoConnection};
use log::LevelFilter;
use sqlx_core::Url;
use sqlx_core::connection::ConnectOptions;
use sqlx_core::error::Error;

impl DoConnection {
    /// Wraps a Durable Object's storage: `state.storage()`.
    #[must_use]
    pub fn new(storage: worker::Storage) -> Self {
        let sql = storage.sql();
        Self {
            storage: worker::send::SendWrapper::new(storage),
            sql,
        }
    }

    /// The storage this connection wraps, for anything this crate does not
    /// cover -- the key-value API, alarms -- while keeping the connection.
    #[must_use]
    pub fn storage(&self) -> &worker::Storage {
        &self.storage
    }

    /// The SQL API this connection runs queries through.
    #[must_use]
    pub fn sql(&self) -> &worker::SqlStorage {
        &self.sql
    }

    /// The storage this connection wraps, giving up the connection. See
    /// [`storage`](Self::storage) to borrow it instead.
    #[must_use]
    pub fn into_storage(self) -> worker::Storage {
        self.storage.0
    }
}

impl From<worker::Storage> for DoConnection {
    fn from(storage: worker::Storage) -> Self {
        Self::new(storage)
    }
}

impl Debug for DoConnection {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("DoConnection").finish_non_exhaustive()
    }
}

impl FromStr for DoConnectOptions {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let url = Url::parse(s).map_err(|error| Error::Configuration(error.into()))?;
        Self::from_url(&url)
    }
}

impl ConnectOptions for DoConnectOptions {
    type Connection = DoConnection;

    fn from_url(url: &Url) -> Result<Self, Error> {
        if url.scheme() == "do" {
            Ok(Self { url: url.clone() })
        } else {
            Err(Error::Configuration(
                format!("expected a `do://` URL, got scheme `{}`", url.scheme()).into(),
            ))
        }
    }

    fn to_url_lossy(&self) -> Url {
        self.url.clone()
    }

    fn connect(&self) -> impl Future<Output = Result<DoConnection, Error>> + Send + '_ {
        future::ready(Err(Error::Configuration(
            "a Durable Object connection cannot be opened from a URL; build it from the object's \
             storage with `DoConnection::new(state.storage())`"
                .into(),
        )))
    }

    // No statement logging yet, so there is nothing for these to configure.
    fn log_statements(self, _level: LevelFilter) -> Self {
        self
    }

    fn log_slow_statements(self, _level: LevelFilter, _duration: Duration) -> Self {
        self
    }
}

#[cfg(test)]
#[path = "connection_test.rs"]
mod tests;
