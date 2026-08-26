//! Persistent Iroh secret-key identity, stored in the OS credential store (spec §7.1, §14.4).

use thiserror::Error;
use zeroize::Zeroize;

const KEYRING_SERVICE: &str = "lcp";
const KEYRING_ACCOUNT: &str = "identity-secret-key";

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("credential store error: {0}")]
    CredentialStore(String),
    #[error("stored identity secret is corrupt")]
    CorruptSecret,
    #[error(
        "identity secret is missing but trusted peers exist in config -- identity recovery or re-pairing is required"
    )]
    MissingSecretWithExistingPeers,
}

pub struct LocalIdentity {
    secret_key: iroh::SecretKey,
}

impl LocalIdentity {
    pub fn endpoint_id(&self) -> iroh::PublicKey {
        self.secret_key.public()
    }

    pub fn secret_key(&self) -> &iroh::SecretKey {
        &self.secret_key
    }
}

fn open_entry() -> Result<keyring::Entry, IdentityError> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| IdentityError::CredentialStore(e.to_string()))
}

/// Loads the persisted identity secret, or generates and persists a new one if none exists yet.
///
/// `has_trusted_peers` must reflect whether the on-disk config already lists trusted peers --
/// if the secret is missing but peers exist, this fails rather than silently minting a new,
/// unrelated identity that those peers never agreed to trust (spec §7.1 step 5).
pub fn load_or_create(has_trusted_peers: bool) -> Result<LocalIdentity, IdentityError> {
    let entry = open_entry()?;
    match entry.get_secret() {
        Ok(mut bytes) => {
            let result = <[u8; 32]>::try_from(bytes.as_slice())
                .map(|array| iroh::SecretKey::from_bytes(&array))
                .map_err(|_| IdentityError::CorruptSecret);
            bytes.zeroize();
            let secret_key = result?;
            Ok(LocalIdentity { secret_key })
        }
        Err(keyring::Error::NoEntry) => {
            if has_trusted_peers {
                return Err(IdentityError::MissingSecretWithExistingPeers);
            }
            let secret_key = iroh::SecretKey::generate();
            let mut bytes = secret_key.to_bytes();
            let store_result = entry
                .set_secret(&bytes)
                .map_err(|e| IdentityError::CredentialStore(e.to_string()));
            bytes.zeroize();
            store_result?;
            Ok(LocalIdentity { secret_key })
        }
        Err(e) => Err(IdentityError::CredentialStore(e.to_string())),
    }
}
