//! Request-scoped native capability observations supplied by the Host.
//!
//! These values are not a wire protocol or an installation registry. The
//! provider owns each native fact; the app-server codec owns consumer defaults.
use collaboration_protocol::ProviderBindingIdentity;
use message_board::{Identity, SessionEndpointRef, SessionRef};
use std::{future::Future, path::PathBuf, pin::Pin};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityName(String);

impl CapabilityName {
    pub fn new(value: String) -> Result<Self, CatalogueReadFailure> {
        if value.trim().is_empty() {
            return Err(CatalogueReadFailure::InvalidNativeData);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCommandName(String);

impl NativeCommandName {
    pub fn new(value: String) -> Result<Self, CatalogueReadFailure> {
        if value.is_empty()
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.')
            })
        {
            return Err(CatalogueReadFailure::InvalidNativeData);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSkillId(CapabilityName);

impl NativeSkillId {
    pub fn new(value: String) -> Result<Self, CatalogueReadFailure> {
        CapabilityName::new(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePluginId(CapabilityName);

impl NativePluginId {
    pub fn new(value: String) -> Result<Self, CatalogueReadFailure> {
        CapabilityName::new(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityPath(PathBuf);

impl CapabilityPath {
    pub fn new(path: PathBuf) -> Result<Self, CatalogueReadFailure> {
        if !path.is_absolute() || path.to_str().is_none_or(|value| value.contains('\0')) {
            return Err(CatalogueReadFailure::InvalidScope);
        }
        Ok(Self(path))
    }

    #[must_use]
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogueQuery {
    Home,
    Workspace { cwd: CapabilityPath },
    Loaded { session: SessionRef },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueView {
    FilesystemCatalogues,
    NativeLoaded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogueRead {
    pub endpoint: SessionEndpointRef,
    pub query: CatalogueQuery,
    pub view: CatalogueView,
    pub refresh_native: bool,
    pub actor: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogueScope {
    pub binding: ProviderBindingIdentity,
    pub query: CatalogueQuery,
    pub view: CatalogueView,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueUnsupported {
    NotAdvertised,
    NativeOperationMissing,
    ScopeUnavailable,
    ConsumerCannotRepresent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CatalogueReadFailure {
    #[error("native catalogue unavailable")]
    Unavailable,
    #[error("native authentication required")]
    AuthenticationRequired,
    #[error("invalid native catalogue data")]
    InvalidNativeData,
    #[error("native catalogue scope unavailable")]
    InvalidScope,
    #[error("unsupported native catalogue filter")]
    UnsupportedConsumerFilter,
    #[error("native capability not found")]
    NotFound,
    #[error("ambiguous native capability selector")]
    AmbiguousSelector,
    #[error("native catalogue read cancelled")]
    Cancelled,
    #[error("native catalogue read timed out")]
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueLimitation {
    PartialNativeExport,
    SessionOnly,
    MissingOrigin,
    MissingScope,
    MissingEnablement,
    AmbiguousNativeIdentity,
    ConsumerCannotRepresent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueCoverage {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogueLimitations {
    first: CatalogueLimitation,
    additional: Vec<CatalogueLimitation>,
}

impl CatalogueLimitations {
    #[must_use]
    pub fn new(first: CatalogueLimitation, additional: Vec<CatalogueLimitation>) -> Self {
        Self { first, additional }
    }

    pub fn iter(&self) -> impl Iterator<Item = &CatalogueLimitation> {
        std::iter::once(&self.first).chain(&self.additional)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceEnumeration {
    Accounted,
    Incomplete { limitations: CatalogueLimitations },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeErrorSource {
    SkillSource { actual_path: CapabilityPath },
    MarketplaceManifest { actual_path: CapabilityPath },
    OtherFilesystem { actual_path: CapabilityPath },
    Unobserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogueSourceError {
    pub source: NativeErrorSource,
    pub reason: CatalogueReadFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogueSection<T> {
    Supported {
        items: Vec<T>,
        enumeration: SourceEnumeration,
        errors: Vec<CatalogueSourceError>,
    },
    Unsupported {
        reason: CatalogueUnsupported,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeOrigin {
    Filesystem { actual_path: CapabilityPath },
    Bundled { native_key: CapabilityName },
    Other { native_label: CapabilityName },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeScope {
    User,
    Workspace,
    System,
    Managed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAvailability {
    Available,
    Disabled,
    Unavailable,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoweringUnsupported {
    MissingProvenance,
    AmbiguousNativeName,
    UnsupportedInputForm,
    NativeOperationMissing,
    ConsumerCannotRepresent,
    ScopeUnqualified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeSkillLowering {
    Qualified {
        canonical_command: NativeCommandName,
    },
    Unqualified {
        reason: LoweringUnsupported,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSkill {
    pub id: NativeSkillId,
    pub name: CapabilityName,
    pub description: String,
    pub origin: NativeOrigin,
    pub native_scope: NativeScope,
    pub availability: NativeAvailability,
    pub display_version: Option<CapabilityName>,
    pub plugin_id: Option<NativePluginId>,
    pub lowering: NativeSkillLowering,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeLoadingState {
    Loaded,
    NotLoaded,
    NotObserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativePluginState {
    InstalledEnabled { loading: NativeLoadingState },
    InstalledDisabled,
    LoadedOnly,
    Unverified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeComponentAvailability {
    Available,
    Unavailable,
    NotObserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeNamedComponent {
    pub name: CapabilityName,
    pub availability: NativeComponentAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeHookEvent {
    Observed { native_name: CapabilityName },
    NotObserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeHook {
    pub native_key: Option<CapabilityName>,
    pub event: NativeHookEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeComponentSection<T> {
    Observed {
        items: Vec<T>,
        coverage: CatalogueCoverage,
    },
    NotObserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePluginComponents {
    pub skills: NativeComponentSection<NativeSkill>,
    pub commands: NativeComponentSection<NativeNamedComponent>,
    pub mcp: NativeComponentSection<NativeNamedComponent>,
    pub hooks: NativeComponentSection<NativeHook>,
    pub other: NativeComponentSection<NativeNamedComponent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMarketplaceSource {
    Filesystem { manifest_path: CapabilityPath },
    NamedCatalog,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMarketplaceObservation {
    Qualified {
        native_name: CapabilityName,
        source: NativeMarketplaceSource,
    },
    Unobserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMarketplaceSelector {
    Filesystem { manifest_path: CapabilityPath },
    NamedCatalog { native_name: CapabilityName },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePluginSelector {
    pub marketplace: NativeMarketplaceSelector,
    pub plugin_name: CapabilityName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePluginRead {
    pub endpoint: SessionEndpointRef,
    pub selector: NativePluginSelector,
    pub actor: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativePolicyObservation {
    Undeclared,
    Declared { value: CapabilityName },
    Unobserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePlugin {
    pub id: NativePluginId,
    pub name: CapabilityName,
    pub description: Option<String>,
    pub origin: NativeOrigin,
    pub state: NativePluginState,
    pub display_version: Option<CapabilityName>,
    pub marketplace: NativeMarketplaceObservation,
    pub components: NativePluginComponents,
    pub installation_policy: NativePolicyObservation,
    pub authentication_policy: NativePolicyObservation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogueObservation {
    pub scope: CatalogueScope,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub skills: CatalogueSection<NativeSkill>,
    pub plugins: CatalogueSection<NativePlugin>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogueResult {
    Ready { report: Box<CatalogueObservation> },
    Unsupported { reason: CatalogueUnsupported },
    Failed { reason: CatalogueReadFailure },
}

pub type CatalogueFuture<'a> = Pin<Box<dyn Future<Output = CatalogueResult> + Send + 'a>>;

pub trait NativeCataloguePort: Send + Sync {
    fn read_catalogue(&self, request: CatalogueRead) -> CatalogueFuture<'_>;
    fn read_plugin(&self, request: NativePluginRead) -> NativePluginReadFuture<'_>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativePluginReadResult {
    Ready {
        scope: Box<CatalogueScope>,
        plugin: Box<NativePlugin>,
    },
    Unsupported {
        reason: CatalogueUnsupported,
    },
    Failed {
        reason: CatalogueReadFailure,
    },
}

pub type NativePluginReadFuture<'a> =
    Pin<Box<dyn Future<Output = NativePluginReadResult> + Send + 'a>>;
