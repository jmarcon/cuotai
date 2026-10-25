//! PRD 0029 RF1: the stable, opaque identity of one `[[accounts]]` table.
//!
//! The legacy key (`<provider slug>:<name>`, see [`crate::Account::key`])
//! changes on rename; an [`AccountId`] does not. New accounts get a random
//! id; an account read from a file without an `id` line gets an *effective*
//! id derived from its legacy key and the configuration identity, so every
//! process reading the same file agrees on it before an interactive front
//! end writes it back (RF1.3). Ids carry no secret (RF1.5).
//!
//! The helpers at the end map state keyed by a legacy key to the id once,
//! with the configuration in hand (RF2.3).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config_file::ConfigIdentity;
use crate::Config;

/// Lowercase Crockford base32 (no `i`, `l`, `o`, `u`).
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
/// Prefix of generated and derived ids.
const PREFIX: &str = "acc_";
/// Characters after [`PREFIX`]: 12 × 5 = 60 bits.
const BODY_LEN: usize = 12;

/// An account id: `acc_` plus 12 lowercase Crockford base32 characters when
/// Cuotai makes it, or any hand-written `[a-z0-9_-]{4,40}` ("work-claude").
/// Valid in URL paths, file names and digests as it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct AccountId(String);

impl AccountId {
    /// `text` when it is `[a-z0-9_-]{4,40}`, otherwise `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let valid = (4..=40).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        valid.then(|| Self(text.to_owned()))
    }

    /// A new random id for an account created by a front end.
    pub fn random() -> Self {
        let bits: u64 = rand::random();
        Self::from_bits(bits)
    }

    /// The effective id of an account without an `id` line: a pure function
    /// of its legacy key and the configuration identity (`None` for a
    /// configuration that did not come from a file nor an integrator id).
    pub fn derive(legacy_key: &str, config: Option<&ConfigIdentity>) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"cuotai.account-id.v1\x1f");
        if let Some(config) = config {
            hash.update(config.digest().as_bytes());
        }
        hash.update(b"\x1f");
        hash.update(legacy_key.as_bytes());
        let digest = hash.finalize();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        Self::from_bits(u64::from_be_bytes(bytes))
    }

    fn from_bits(bits: u64) -> Self {
        let mut id = String::with_capacity(PREFIX.len() + BODY_LEN);
        id.push_str(PREFIX);
        // The top 60 bits, five at a time.
        for index in 0..BODY_LEN {
            let shift = 64 - 5 * (index + 1);
            id.push(CROCKFORD[((bits >> shift) & 0x1f) as usize] as char);
        }
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom("account id must be 4 to 40 characters of a-z, 0-9, _ or -")
        })
    }
}

/// Legacy key → state key ([`crate::Account::state_key`]) of every account
/// of `config`. A state entry keyed by a legacy key found here belongs to
/// that account; one found nowhere is left for the usual pruning.
pub fn legacy_keys(config: &Config) -> BTreeMap<String, String> {
    config
        .accounts
        .iter()
        .map(|account| (account.key(), account.state_key()))
        .collect()
}

/// Rewrite the keys of `map` that are legacy keys of `config` to the
/// account's state key (RF2.3). Values of an id that already exists win: the
/// entry already migrated is newer than a legacy one written by an older
/// release. Idempotent; `true` when something changed.
pub fn migrate_map<V>(map: &mut BTreeMap<String, V>, legacy: &BTreeMap<String, String>) -> bool {
    let moving: Vec<String> = map
        .keys()
        .filter(|key| legacy.get(*key).is_some_and(|id| id != *key))
        .cloned()
        .collect();
    for key in &moving {
        let value = map.remove(key).expect("key listed above");
        map.entry(legacy[key].clone()).or_insert(value);
    }
    !moving.is_empty()
}

/// [`migrate_map`] for a set of keys.
pub fn migrate_set(
    set: &mut std::collections::BTreeSet<String>,
    legacy: &BTreeMap<String, String>,
) -> bool {
    let moving: Vec<String> = set
        .iter()
        .filter(|key| legacy.get(*key).is_some_and(|id| id != *key))
        .cloned()
        .collect();
    for key in &moving {
        set.remove(key);
        set.insert(legacy[key].clone());
    }
    !moving.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_have_the_documented_shape() {
        for id in [
            AccountId::random(),
            AccountId::derive("claude:Work", None),
            AccountId::derive("claude:Work", Some(&ConfigIdentity::Explicit("one".into()))),
        ] {
            let text = id.as_str();
            assert_eq!(text.len(), 16, "{text}");
            assert!(text.starts_with("acc_"), "{text}");
            assert!(text[4..].bytes().all(|b| CROCKFORD.contains(&b)), "{text}");
            assert_eq!(AccountId::parse(text), Some(id.clone()));
        }
    }

    #[test]
    fn derived_ids_depend_only_on_the_legacy_key_and_the_config() {
        let one = ConfigIdentity::Explicit("one".into());
        let two = ConfigIdentity::Explicit("two".into());
        let id = AccountId::derive("claude:Work", Some(&one));
        assert_eq!(id, AccountId::derive("claude:Work", Some(&one)));
        assert_ne!(id, AccountId::derive("claude:Home", Some(&one)));
        assert_ne!(id, AccountId::derive("claude:Work", Some(&two)));
        assert_ne!(id, AccountId::derive("claude:Work", None));
    }

    #[test]
    fn parse_accepts_hand_written_ids_and_rejects_others() {
        for ok in ["work-claude", "acc_0123456789ab", "a_b-", &"x".repeat(40)] {
            assert!(AccountId::parse(ok).is_some(), "{ok}");
        }
        for bad in [
            "",
            "abc",
            "Work",
            "a b c",
            "acc/123",
            "ação1",
            &"x".repeat(41),
        ] {
            assert!(AccountId::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn map_migration_is_idempotent_and_keeps_unknown_keys() {
        let legacy = BTreeMap::from([
            ("claude:Work".to_owned(), "acc_one".to_owned()),
            ("codex:Home".to_owned(), "acc_two".to_owned()),
        ]);
        let mut map = BTreeMap::from([
            ("claude:Work".to_owned(), 1),
            ("codex:Home".to_owned(), 2),
            ("acc_two".to_owned(), 3),
            ("gemini:Gone".to_owned(), 4),
        ]);
        assert!(migrate_map(&mut map, &legacy));
        assert_eq!(
            map,
            BTreeMap::from([
                ("acc_one".to_owned(), 1),
                ("acc_two".to_owned(), 3),
                ("gemini:Gone".to_owned(), 4),
            ])
        );
        let again = map.clone();
        assert!(!migrate_map(&mut map, &legacy));
        assert_eq!(map, again);

        let mut set = std::collections::BTreeSet::from(["claude:Work".to_owned()]);
        assert!(migrate_set(&mut set, &legacy));
        assert!(!migrate_set(&mut set, &legacy));
        assert_eq!(
            set,
            std::collections::BTreeSet::from(["acc_one".to_owned()])
        );
    }
}
