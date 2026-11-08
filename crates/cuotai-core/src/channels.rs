//! PRD 0044: built-in notification channels: a generic webhook, an ntfy
//! topic, a Slack incoming webhook and a Discord webhook.
//!
//! A **channel** is a destination the person owns to which Cuotai delivers
//! the same notifications it shows on the system, after the same policy
//! (PRD 0032: preferences, quiet hours, private texts and the shared
//! ledger). Nothing is sent to anyone until the person adds a channel.
//!
//! * The definitions ([`Channel`]: kind, label, host, filters, switch) live
//!   in the `channels` member of `desktop.json`, the PRD 0032 preferences
//!   file every notifying process reads. Never the URL.
//! * The URL (which usually embeds a token) and the optional webhook
//!   signing secret live in the vault ([`Vault`]), the same store as OAuth
//!   tokens and extension secrets, so the debug-only SQLite store follows
//!   the same rules.
//! * [`validate_url`] is the boundary: HTTPS only (plain HTTP only for an
//!   ntfy server on loopback or a private network, with a warning), no
//!   userinfo, at most [`MAX_URL_BYTES`], host allow-lists for Slack and
//!   Discord and the ntfy topic alphabet.
//! * [`payload`] renders one core notification into the four wire formats;
//!   [`delivery`] sends them in the background with bounded retries, an
//!   hourly cap and a local [`log`] that never holds a body or a URL.

pub mod delivery;
pub mod log;
pub mod payload;

#[cfg(test)]
mod tests;

use crate::oauth::{OAuthToken, TokenStore};
use crate::text::{params, Localized};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

pub use delivery::{Dispatcher, FailingNotice, Outcome};
pub use payload::{Outgoing, Source};

/// At most this many channels (RF1, §3).
pub const MAX_CHANNELS: usize = 5;
/// The longest URL accepted, in bytes (RF1.2).
pub const MAX_URL_BYTES: usize = 2048;
/// The longest label, in characters.
pub const MAX_LABEL_CHARS: usize = 64;
/// The member of `desktop.json` that holds the definitions.
pub const SETTINGS_KEY: &str = "channels";

/// The four kinds of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Webhook,
    Ntfy,
    Slack,
    Discord,
}

impl ChannelKind {
    pub const ALL: [Self; 4] = [Self::Webhook, Self::Ntfy, Self::Slack, Self::Discord];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Webhook => "webhook",
            Self::Ntfy => "ntfy",
            Self::Slack => "slack",
            Self::Discord => "discord",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.slug().eq_ignore_ascii_case(text.trim()))
    }

    /// The product name, never translated.
    pub fn name(self) -> &'static str {
        match self {
            Self::Webhook => "Webhook",
            Self::Ntfy => "ntfy",
            Self::Slack => "Slack",
            Self::Discord => "Discord",
        }
    }
}

/// How serious a notification is (RF1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub const ALL: [Self; 3] = [Self::Info, Self::Warning, Self::Critical];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|severity| severity.slug().eq_ignore_ascii_case(text.trim()))
    }

    pub fn label(self) -> Localized {
        match self {
            Self::Info => Localized::fixed("channels.severity.info", "every level"),
            Self::Warning => {
                Localized::fixed("channels.severity.warning", "warnings and critical only")
            }
            Self::Critical => Localized::fixed("channels.severity.critical", "critical only"),
        }
    }
}

/// The notification kinds of PRD 0032 a channel can filter on (RF1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Reset,
    Threshold,
    Exhausted,
    Recovered,
    Projected,
    Overage,
    Floor,
    SessionExpiring,
    LimitChanged,
    Digest,
}

impl NoticeKind {
    pub const ALL: [Self; 10] = [
        Self::Reset,
        Self::Threshold,
        Self::Exhausted,
        Self::Recovered,
        Self::Projected,
        Self::Overage,
        Self::Floor,
        Self::SessionExpiring,
        Self::LimitChanged,
        Self::Digest,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Reset => "reset",
            Self::Threshold => "threshold",
            Self::Exhausted => "exhausted",
            Self::Recovered => "recovered",
            Self::Projected => "projected",
            Self::Overage => "overage",
            Self::Floor => "floor",
            Self::SessionExpiring => "session_expiring",
            Self::LimitChanged => "limit_changed",
            Self::Digest => "digest",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.slug().eq_ignore_ascii_case(text.trim()))
    }

    /// RF1.3: exhaustion and floors are critical, thresholds and the other
    /// warnings warning, resets, recovery, limit changes and the digest
    /// information.
    pub fn severity(self) -> Severity {
        match self {
            Self::Exhausted | Self::Floor => Severity::Critical,
            Self::Threshold | Self::Projected | Self::Overage | Self::SessionExpiring => {
                Severity::Warning
            }
            Self::Reset | Self::Recovered | Self::LimitChanged | Self::Digest => Severity::Info,
        }
    }

    /// The kind of one usage alert.
    pub fn of_alert(kind: crate::alerts::AlertKind) -> Self {
        use crate::alerts::AlertKind as A;
        match kind {
            A::Threshold => Self::Threshold,
            A::Exhausted | A::PoolExhausted => Self::Exhausted,
            A::Recovered | A::PoolRecovered | A::BalanceRecovered => Self::Recovered,
            A::SessionExpiring => Self::SessionExpiring,
            A::ProjectedExhaustion => Self::Projected,
            A::OverageStarted | A::OverageCapReached => Self::Overage,
            A::BalanceBelowFloor => Self::Floor,
            A::LimitChanged => Self::LimitChanged,
        }
    }

    pub fn label(self) -> Localized {
        match self {
            Self::Reset => Localized::fixed("channels.kind.reset", "Resets"),
            Self::Threshold => Localized::fixed("channels.kind.threshold", "Thresholds"),
            Self::Exhausted => Localized::fixed("channels.kind.exhausted", "Exhaustion"),
            Self::Recovered => Localized::fixed("channels.kind.recovered", "Recovery"),
            Self::Projected => Localized::fixed("channels.kind.projected", "Projected exhaustion"),
            Self::Overage => Localized::fixed("channels.kind.overage", "Extra usage"),
            Self::Floor => Localized::fixed("channels.kind.floor", "Balance floors"),
            Self::SessionExpiring => {
                Localized::fixed("channels.kind.session_expiring", "Session expiry")
            }
            Self::LimitChanged => Localized::fixed("channels.kind.limit_changed", "Limit changes"),
            Self::Digest => Localized::fixed("channels.kind.digest", "Weekly digest"),
        }
    }
}

/// What a channel receives (RF1.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filters {
    /// Default: every kind but `digest` (RF6.1: the digest is off by
    /// default per channel).
    #[serde(default = "default_kinds")]
    pub kinds: BTreeSet<NoticeKind>,
    /// Stable account ids (PRD 0029); empty means every account.
    #[serde(default)]
    pub accounts: BTreeSet<String>,
    #[serde(default = "default_severity")]
    pub min_severity: Severity,
    /// `None` follows PRD 0032's private setting (names hidden by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_names: Option<bool>,
}

fn default_kinds() -> BTreeSet<NoticeKind> {
    NoticeKind::ALL
        .into_iter()
        .filter(|kind| *kind != NoticeKind::Digest)
        .collect()
}

fn default_severity() -> Severity {
    Severity::Info
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            kinds: default_kinds(),
            accounts: BTreeSet::new(),
            min_severity: Severity::Info,
            include_names: None,
        }
    }
}

impl Filters {
    /// Whether a notification of `kind` passes the kind and severity rules.
    pub fn admits(&self, kind: NoticeKind) -> bool {
        self.kinds.contains(&kind) && kind.severity() >= self.min_severity
    }

    /// Whether the account `id` passes.
    pub fn admits_account(&self, id: &str) -> bool {
        self.accounts.is_empty() || self.accounts.contains(id)
    }

    /// Whether account names appear, given PRD 0032's `private` setting.
    pub fn names(&self, private: bool) -> bool {
        self.include_names.unwrap_or(!private)
    }

    /// Apply one `key=value` of the CLI's `--channel-filter`: `kinds`
    /// (comma-separated slugs or `all`), `accounts` (ids or `all`),
    /// `min_severity` (`info`, `warning`, `critical`) or `names` (`on`,
    /// `off`, `default`).
    pub fn apply(&mut self, assignment: &str) -> Result<(), Localized> {
        let invalid = || {
            Localized::with_param(
                "channels.error.filter",
                "filter",
                crate::diagnostics::redact::text(assignment),
                "Unknown filter {filter}. Use kinds=…, accounts=…, min_severity=info|warning|critical or names=on|off|default.",
            )
        };
        let (key, value) = assignment.split_once('=').ok_or_else(invalid)?;
        let value = value.trim();
        match key.trim() {
            "kinds" => {
                if value.eq_ignore_ascii_case("all") {
                    self.kinds = NoticeKind::ALL.into_iter().collect();
                    return Ok(());
                }
                let kinds = value
                    .split(',')
                    .filter(|part| !part.trim().is_empty())
                    .map(NoticeKind::parse)
                    .collect::<Option<BTreeSet<_>>>()
                    .ok_or_else(invalid)?;
                if kinds.is_empty() {
                    return Err(invalid());
                }
                self.kinds = kinds;
            }
            "accounts" => {
                self.accounts = if value.eq_ignore_ascii_case("all") {
                    BTreeSet::new()
                } else {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(str::to_owned)
                        .collect()
                };
            }
            "min_severity" => self.min_severity = Severity::parse(value).ok_or_else(invalid)?,
            "names" => {
                self.include_names = match value {
                    "on" | "true" | "yes" => Some(true),
                    "off" | "false" | "no" => Some(false),
                    "default" => None,
                    _ => return Err(invalid()),
                }
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
}

/// One channel definition, as `desktop.json` keeps it: never the URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    /// 16 random hex characters; the vault key derives from it.
    pub id: String,
    pub kind: ChannelKind,
    pub label: String,
    /// The destination's host name only, for display and consent.
    pub host: String,
    #[serde(default)]
    pub filters: Filters,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    /// A generic webhook with an HMAC signing secret in the vault.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed: bool,
}

fn enabled_default() -> bool {
    true
}

/// A URL that passed [`validate_url`]. Its `Debug` is redacted and it has
/// no `Display` or `Serialize`, so it cannot slip into a log or a view.
#[derive(Clone)]
pub struct ValidUrl {
    url: url::Url,
    host: String,
    warning: Option<Localized>,
}

impl ValidUrl {
    /// The host shown to the person.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// RF1.2: plain HTTP on a private network (ntfy only).
    pub fn warning(&self) -> Option<&Localized> {
        self.warning.as_ref()
    }

    /// `https://hooks.slack.com/…` style masking: scheme, host and a
    /// fixed mask, never the path that carries the token (RF5.1).
    pub fn masked(&self) -> String {
        format!("{}://{}/••••", self.url.scheme(), self.host)
    }

    pub(crate) fn expose(&self) -> &url::Url {
        &self.url
    }
}

impl fmt::Debug for ValidUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValidUrl({}, [redacted])", self.host)
    }
}

/// Hosts of provider APIs Cuotai reads: a channel never posts to one, so a
/// provider credential URL pasted by mistake is refused (RF1.2).
const PROVIDER_HOSTS: [&str; 12] = [
    "api.openai.com",
    "api.anthropic.com",
    "claude.ai",
    "chatgpt.com",
    "openrouter.ai",
    "generativelanguage.googleapis.com",
    "api.x.ai",
    "api.deepseek.com",
    "api.mistral.ai",
    "api.groq.com",
    "api.together.xyz",
    "api.moonshot.ai",
];

fn url_error(id: &'static str, fallback: &'static str) -> Localized {
    Localized::fixed(id, fallback)
}

/// Whether `ip` is loopback or a private (RFC 1918 / unique local) address.
fn private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// The ntfy topic alphabet: `[A-Za-z0-9_-]{1,64}`.
pub fn valid_topic(topic: &str) -> bool {
    (1..=64).contains(&topic.len())
        && topic
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// RF1.2: check the URL of a `kind` channel. Errors never echo the URL.
pub fn validate_url(kind: ChannelKind, raw: &str) -> Result<ValidUrl, Localized> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(url_error("channels.error.url_empty", "Paste the address."));
    }
    if raw.len() > MAX_URL_BYTES {
        return Err(url_error(
            "channels.error.url_long",
            "The address is longer than 2048 bytes.",
        ));
    }
    let url = url::Url::parse(raw).map_err(|_| {
        url_error(
            "channels.error.url_invalid",
            "That is not a web address (it should start with https://).",
        )
    })?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(url_error(
            "channels.error.url_userinfo",
            "The address must not carry a user name or password before the host.",
        ));
    }
    if url.fragment().is_some() {
        return Err(url_error(
            "channels.error.url_fragment",
            "The address must not end with a # fragment.",
        ));
    }
    let host = match url.host() {
        Some(url::Host::Domain(domain)) => domain.to_ascii_lowercase(),
        Some(url::Host::Ipv4(ip)) => ip.to_string(),
        Some(url::Host::Ipv6(ip)) => format!("[{ip}]"),
        None => {
            return Err(url_error(
                "channels.error.url_invalid",
                "That is not a web address (it should start with https://).",
            ))
        }
    };
    let ip = match url.host() {
        Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        _ => None,
    };
    let local = ip.is_some_and(private_ip) || host == "localhost";
    let mut warning = None;
    match url.scheme() {
        "https" => {}
        "http" if kind == ChannelKind::Ntfy && local => {
            warning = Some(Localized::fixed(
                "channels.warning.plain_http",
                "This address uses plain HTTP: anyone on your network can read the notifications. Use it only for a server on this computer or your private network.",
            ));
        }
        "http" => {
            return Err(url_error(
                "channels.error.url_http",
                "Only https:// addresses are accepted (plain http:// only for an ntfy server on this computer or a private network).",
            ))
        }
        _ => {
            return Err(url_error(
                "channels.error.url_invalid",
                "That is not a web address (it should start with https://).",
            ))
        }
    }
    if PROVIDER_HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
    {
        return Err(url_error(
            "channels.error.url_provider",
            "That is a provider's address, not a notification destination.",
        ));
    }
    match kind {
        ChannelKind::Webhook => {}
        ChannelKind::Slack => {
            if host != "hooks.slack.com" || !url.path().starts_with("/services/") {
                return Err(url_error(
                    "channels.error.url_slack",
                    "A Slack incoming webhook starts with https://hooks.slack.com/services/.",
                ));
            }
        }
        ChannelKind::Discord => {
            let allowed = host == "discord.com" || host == "discordapp.com";
            if !allowed || !url.path().starts_with("/api/webhooks/") {
                return Err(url_error(
                    "channels.error.url_discord",
                    "A Discord webhook starts with https://discord.com/api/webhooks/.",
                ));
            }
        }
        ChannelKind::Ntfy => {
            let topic = url.path().trim_start_matches('/');
            if url.query().is_some() || !valid_topic(topic) {
                return Err(url_error(
                    "channels.error.url_ntfy",
                    "An ntfy address is the server followed by one topic of letters, digits, - or _ (up to 64), such as https://ntfy.sh/my-topic.",
                ));
            }
        }
    }
    Ok(ValidUrl { url, host, warning })
}

/// Check a label: 1 to [`MAX_LABEL_CHARS`] characters, no control
/// characters; returns it trimmed.
pub fn validate_label(label: &str) -> Result<String, Localized> {
    let label = label.trim();
    let chars = label.chars().count();
    if chars == 0 || chars > MAX_LABEL_CHARS || label.chars().any(char::is_control) {
        return Err(Localized::fixed(
            "channels.error.label",
            "Give the channel a name of 1 to 64 characters.",
        ));
    }
    Ok(label.to_owned())
}

/// A new random channel id.
fn new_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// The vault.

/// The vault entries of the channels: `channel_<id>` (the URL) and
/// `channel_<id>_secret` (the webhook signing key), in the same store as
/// OAuth tokens and extension secrets.
#[derive(Clone)]
pub struct Vault(Arc<dyn TokenStore>);

impl Vault {
    pub fn new(store: Arc<dyn TokenStore>) -> Self {
        Self(store)
    }

    pub fn url_key(id: &str) -> String {
        format!("channel_{id}")
    }

    pub fn secret_key(id: &str) -> String {
        format!("channel_{id}_secret")
    }

    fn put(&self, key: &str, value: &str) -> Result<(), Localized> {
        let token = OAuthToken {
            access_token: value.to_owned(),
            refresh_token: None,
            expires_at: None,
            metadata: BTreeMap::new(),
        };
        self.0.save(key, &token).map_err(|_| vault_error())
    }

    fn get(&self, key: &str) -> Result<Option<String>, Localized> {
        self.0
            .load(key)
            .map(|token| token.map(|token| token.access_token))
            .map_err(|_| vault_error())
    }

    /// The URL of `channel`, validated again (an entry edited outside
    /// Cuotai is refused, never sent to).
    pub fn url(&self, channel: &Channel) -> Result<ValidUrl, Localized> {
        let raw = self.get(&Self::url_key(&channel.id))?.ok_or_else(|| {
            Localized::fixed(
                "channels.error.vault_missing",
                "The channel's address is missing from the system vault; edit the channel and paste it again.",
            )
        })?;
        validate_url(channel.kind, &raw)
    }

    /// Tests: the stored URL with no check, for the local mock servers.
    #[cfg(test)]
    pub(crate) fn loose_url(&self, channel: &Channel) -> Option<ValidUrl> {
        let raw = self.get(&Self::url_key(&channel.id)).ok()??;
        let url = url::Url::parse(&raw).ok()?;
        let host = url.host_str()?.to_owned();
        Some(ValidUrl {
            url,
            host,
            warning: None,
        })
    }

    /// The signing key of a signed webhook.
    pub fn secret(&self, channel: &Channel) -> Result<Option<String>, Localized> {
        if !channel.signed {
            return Ok(None);
        }
        self.get(&Self::secret_key(&channel.id))
    }

    fn delete(&self, id: &str) -> Result<(), Localized> {
        self.0
            .delete(&Self::url_key(id))
            .and_then(|()| self.0.delete(&Self::secret_key(id)))
            .map_err(|_| vault_error())
    }
}

fn vault_error() -> Localized {
    Localized::fixed(
        "channels.error.vault",
        "The system vault is unavailable, so the channel's address could not be read or saved.",
    )
}

// ---------------------------------------------------------------------------
// The definitions in `desktop.json`.

/// The channels in the document `value` (the parsed `desktop.json`); an
/// entry that does not read, or whose id, label or host Cuotai would not
/// have written ([`well_formed`]), is skipped.
pub fn from_document(value: &serde_json::Value) -> Vec<Channel> {
    value
        .get(SETTINGS_KEY)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<Channel>(item.clone()).ok())
                .filter(well_formed)
                .take(MAX_CHANNELS)
                .collect()
        })
        .unwrap_or_default()
}

/// A hand-edited `desktop.json` is not trusted: the id builds vault keys
/// and HTML attributes, the label and host are shown as they are. The id
/// is [`new_id`]'s 16 lowercase hex characters, the label passes
/// [`validate_label`] and the host holds only host-name characters (an
/// IPv6 address keeps its brackets).
fn well_formed(channel: &Channel) -> bool {
    let id = channel.id.len() == 16
        && channel
            .id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let host = !channel.host.is_empty()
        && channel.host.len() <= 255
        && channel
            .host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:[]".contains(&b));
    id && host && validate_label(&channel.label).is_ok()
}

/// The channels in `desktop.json` at `path`; none when it is missing or
/// unreadable.
pub fn read(path: &Path) -> Vec<Channel> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .map(|document| from_document(&document))
        .unwrap_or_default()
}

/// The `desktop.json` schema major this module writes (PRD 0051).
const SCHEMA_MAJOR: u64 = 1;

fn settings_error() -> Localized {
    Localized::fixed(
        "channels.error.settings",
        "The preferences file could not be saved; nothing was changed.",
    )
}

/// One read-modify-write of the `channels` member of `desktop.json`
/// under the lock every Cuotai process shares (PRD 0051 RF4), keeping
/// every other member. A damaged file or a later schema major is never
/// rewritten.
fn mutate<T>(
    path: &Path,
    change: impl FnOnce(&mut Vec<Channel>) -> Result<T, Localized>,
) -> Result<T, Localized> {
    use crate::private_fs::{update_locked, UpdateError};
    let mut out = None;
    let result = update_locked(path, |bytes| {
        let mut document = match bytes {
            None => serde_json::Value::Object(serde_json::Map::new()),
            Some(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(value) if value.is_object() => value,
                _ => return Err(settings_error()),
            },
        };
        if document
            .get("schema")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|major| major > SCHEMA_MAJOR)
        {
            return Err(settings_error());
        }
        let mut channels = from_document(&document);
        out = Some(change(&mut channels)?);
        let list = serde_json::to_value(&channels).map_err(|_| settings_error())?;
        if let Some(object) = document.as_object_mut() {
            if channels.is_empty() {
                object.remove(SETTINGS_KEY);
            } else {
                object.insert(SETTINGS_KEY.to_owned(), list);
            }
        }
        serde_json::to_vec_pretty(&document).map_err(|_| settings_error())
    });
    match result {
        Ok(()) => out.ok_or_else(settings_error),
        Err(UpdateError::Refused(error)) => Err(error),
        Err(_) => Err(settings_error()),
    }
}

/// What the person chose for a new or edited channel.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Draft {
    pub kind: ChannelKind,
    pub label: String,
    #[serde(default)]
    pub filters: Filters,
    /// Generic webhooks only: sign each body with an HMAC key Cuotai
    /// generates (shown once).
    #[serde(default)]
    pub sign: bool,
}

/// A channel just saved; `signing_secret` is shown once.
pub struct Saved {
    pub channel: Channel,
    pub signing_secret: Option<String>,
}

impl fmt::Debug for Saved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Saved")
            .field("channel", &self.channel)
            .field(
                "signing_secret",
                &self.signing_secret.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

fn generate_secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn too_many() -> Localized {
    Localized::with_param(
        "channels.error.too_many",
        "max",
        MAX_CHANNELS as u64,
        "At most {max} channels; remove one first.",
    )
}

fn unknown_channel() -> Localized {
    Localized::fixed("channels.error.unknown", "There is no such channel.")
}

/// RF4.1: add a channel. Saving is the consent; it starts enabled. The URL
/// goes to the vault first and is removed again when the preferences
/// cannot be written.
pub fn add(
    path: &Path,
    vault: &Vault,
    draft: &Draft,
    url: &str,
    now: DateTime<Utc>,
) -> Result<Saved, Localized> {
    let valid = validate_url(draft.kind, url)?;
    let label = validate_label(&draft.label)?;
    if read(path).len() >= MAX_CHANNELS {
        return Err(too_many());
    }
    let id = new_id();
    let sign = draft.sign && draft.kind == ChannelKind::Webhook;
    vault.put(&Vault::url_key(&id), valid.expose().as_str())?;
    let signing_secret = if sign {
        let secret = generate_secret();
        if let Err(error) = vault.put(&Vault::secret_key(&id), &secret) {
            let _ = vault.delete(&id);
            return Err(error);
        }
        Some(secret)
    } else {
        None
    };
    let channel = Channel {
        id: id.clone(),
        kind: draft.kind,
        label,
        host: valid.host().to_owned(),
        filters: draft.filters.clone(),
        enabled: true,
        created_at: now,
        signed: sign,
    };
    let saved = mutate(path, |channels| {
        if channels.len() >= MAX_CHANNELS {
            return Err(too_many());
        }
        channels.push(channel.clone());
        Ok(())
    });
    if let Err(error) = saved {
        let _ = vault.delete(&id);
        return Err(error);
    }
    Ok(Saved {
        channel,
        signing_secret,
    })
}

/// RF4.3: edit a channel. A new `url` replaces the vault entry (the front
/// end asked consent again); `None` keeps it. The kind cannot change.
/// Turning signing on generates a new key, shown once.
pub fn edit(
    path: &Path,
    vault: &Vault,
    id: &str,
    draft: &Draft,
    url: Option<&str>,
) -> Result<Saved, Localized> {
    let label = validate_label(&draft.label)?;
    let current = read(path)
        .into_iter()
        .find(|channel| channel.id == id)
        .ok_or_else(unknown_channel)?;
    if current.kind != draft.kind {
        return Err(Localized::fixed(
            "channels.error.kind_change",
            "A channel's kind cannot change; add a new channel instead.",
        ));
    }
    let valid = url.map(|url| validate_url(draft.kind, url)).transpose()?;
    let sign = draft.sign && draft.kind == ChannelKind::Webhook;
    let new_secret = sign && !current.signed;
    // As `add` does: a failed edit leaves the vault as it found it.
    let (url_key, secret_key) = (Vault::url_key(id), Vault::secret_key(id));
    let previous_url = valid.as_ref().map(|_| vault.get(&url_key)).transpose()?;
    let previous_secret = new_secret.then(|| vault.get(&secret_key)).transpose()?;
    let restore = || {
        for (key, previous) in [(&url_key, &previous_url), (&secret_key, &previous_secret)] {
            match previous {
                Some(Some(value)) => {
                    let _ = vault.put(key, value);
                }
                Some(None) => {
                    let _ = vault.0.delete(key);
                }
                None => {}
            }
        }
    };
    if let Some(valid) = &valid {
        vault.put(&url_key, valid.expose().as_str())?;
    }
    let signing_secret = if new_secret {
        let secret = generate_secret();
        if let Err(error) = vault.put(&secret_key, &secret) {
            restore();
            return Err(error);
        }
        Some(secret)
    } else {
        None
    };
    let saved = mutate(path, |channels| {
        let channel = channels
            .iter_mut()
            .find(|channel| channel.id == id)
            .ok_or_else(unknown_channel)?;
        channel.label = label;
        channel.filters = draft.filters.clone();
        channel.signed = sign;
        if let Some(valid) = &valid {
            channel.host = valid.host().to_owned();
        }
        Ok(Saved {
            channel: channel.clone(),
            signing_secret,
        })
    });
    match saved {
        Ok(saved) => {
            // Signing turned off: the key goes once the channel no longer
            // needs it.
            if !sign && current.signed {
                let _ = vault.0.delete(&secret_key);
            }
            Ok(saved)
        }
        Err(error) => {
            restore();
            Err(error)
        }
    }
}

/// RF4.3: turn a channel on or off (no dialog).
pub fn set_enabled(path: &Path, id: &str, enabled: bool) -> Result<Channel, Localized> {
    mutate(path, |channels| {
        let channel = channels
            .iter_mut()
            .find(|channel| channel.id == id)
            .ok_or_else(unknown_channel)?;
        channel.enabled = enabled;
        Ok(channel.clone())
    })
}

/// RF5.1: remove a channel and its vault entries.
pub fn remove(path: &Path, vault: &Vault, id: &str) -> Result<(), Localized> {
    mutate(path, |channels| {
        let before = channels.len();
        channels.retain(|channel| channel.id != id);
        if channels.len() == before {
            return Err(unknown_channel());
        }
        Ok(())
    })?;
    vault.delete(id)
}

/// The channel whose id is `id` or, when no id matches, whose label is it
/// (case-insensitively): what the CLI accepts.
pub fn find(path: &Path, id_or_label: &str) -> Option<Channel> {
    let channels = read(path);
    channels
        .iter()
        .find(|channel| channel.id == id_or_label)
        .or_else(|| {
            channels
                .iter()
                .find(|channel| channel.label.eq_ignore_ascii_case(id_or_label.trim()))
        })
        .cloned()
}

// ---------------------------------------------------------------------------
// Consent (RF4.1).

/// The filters in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilterWords {
    /// The kinds, in [`NoticeKind::ALL`] order; one "every notification"
    /// item when all of them are on.
    pub kinds: Vec<Localized>,
    /// "of every account" / "of 3 accounts".
    pub accounts: Localized,
    /// "names hidden" / "names shown".
    pub names: Localized,
    /// `None` when every severity passes.
    pub severity: Option<Localized>,
}

impl FilterWords {
    /// "Thresholds, Exhaustion, Balance floors · of 3 accounts · names
    /// hidden".
    pub fn resolve(&self, locale: &str) -> String {
        let kinds: Vec<String> = self.kinds.iter().map(|kind| kind.resolve(locale)).collect();
        let mut parts = vec![kinds.join(", "), self.accounts.resolve(locale)];
        if let Some(severity) = &self.severity {
            parts.push(severity.resolve(locale));
        }
        parts.push(self.names.resolve(locale));
        parts.join(" · ")
    }
}

/// RF4.1: the filters of a channel in words, with PRD 0032's `private`.
pub fn filter_words(filters: &Filters, private: bool) -> FilterWords {
    let kinds = if filters.kinds.len() == NoticeKind::ALL.len() {
        vec![Localized::fixed(
            "channels.words.every_kind",
            "Every notification",
        )]
    } else {
        NoticeKind::ALL
            .into_iter()
            .filter(|kind| filters.kinds.contains(kind))
            .map(NoticeKind::label)
            .collect()
    };
    let accounts = if filters.accounts.is_empty() {
        Localized::fixed("channels.words.every_account", "of every account")
    } else {
        Localized::plural(
            "channels.words.accounts",
            params([("count", filters.accounts.len().into())]),
            "of {count} account",
            "of {count} accounts",
        )
    };
    let names = if filters.names(private) {
        Localized::fixed("channels.words.names_shown", "names shown")
    } else {
        Localized::fixed("channels.words.names_hidden", "names hidden")
    };
    let severity = (filters.min_severity != Severity::Info).then(|| filters.min_severity.label());
    FilterWords {
        kinds,
        accounts,
        names,
        severity,
    }
}

/// RF4.1: the sentence every consent shows.
pub fn consent_sentence() -> Localized {
    Localized::fixed(
        "channels.consent.sentence",
        "Cuotai will send these texts to this address when a notification is due. Nothing else leaves this computer.",
    )
}

/// RF4.1: what the dialog shows before **Save**.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Consent {
    pub kind: ChannelKind,
    pub host: String,
    pub masked: String,
    pub warning: Option<Localized>,
    pub filters: FilterWords,
    /// The body a sample notification would have, as sent.
    pub example: String,
    pub sentence: Localized,
}

/// RF4.1: the consent of a draft whose URL validates.
pub fn consent(
    draft: &Draft,
    url: &str,
    private: bool,
    locale: &str,
    now: DateTime<Utc>,
) -> Result<Consent, Localized> {
    let valid = validate_url(draft.kind, url)?;
    Ok(Consent {
        kind: draft.kind,
        host: valid.host().to_owned(),
        masked: valid.masked(),
        warning: valid.warning().cloned(),
        filters: filter_words(&draft.filters, private),
        example: payload::example(draft.kind, &draft.filters, private, locale, now),
        sentence: consent_sentence(),
    })
}

/// RF4.3: whether an edit needs consent again: a new URL whose host or
/// address differs.
pub fn needs_consent(vault: &Vault, channel: &Channel, url: Option<&str>) -> bool {
    let Some(url) = url else {
        return false;
    };
    match (validate_url(channel.kind, url), vault.url(channel)) {
        (Ok(new), Ok(old)) => new.expose() != old.expose(),
        _ => true,
    }
}
