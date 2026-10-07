//! Profile and group pictures.
//!
//! presage keeps the pictures it downloads for good, so a changed one would
//! never show. They are fetched here instead: a picture's path on the CDN
//! changes whenever the picture does, which makes it the id the app compares.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use presage::libsignal_service::configuration::{Endpoint, ServiceCredentials, SignalServers};
use presage::libsignal_service::content::GroupContextV2;
use presage::libsignal_service::StorageService;
use presage::libsignal_service::groups_v2::GroupOperations;
use presage::libsignal_service::master_key::StorageServiceKey;
use presage::libsignal_service::prelude::{GroupMasterKey, GroupSecretParams, ProfileKey, Uuid};
use presage::libsignal_service::profile_cipher::ProfileCipher;
use presage::libsignal_service::proto::manifest_record::identifier::Type as RecordType;
use presage::libsignal_service::proto::storage_record::Record;
use presage::libsignal_service::protocol::{Aci, ServiceId};
use presage::libsignal_service::push_service::{HttpAuthOverride, PushService};
use presage::libsignal_service::utils::TryIntoE164;
use presage::manager::RegistrationData;
use presage::store::{ContentsStore, StateStore};
use presage_store_sqlite::SqliteStore;
use serde::Deserialize;

use crate::{Event, GROUP_PREFIX, READ_RECEIPTS, Signal, USER_AGENT, emit};

const TIMEOUT: Duration = Duration::from_secs(30);
/// How many records the storage service hands out per request.
const STORAGE_BATCH: usize = 1000;

/// The one field of a profile that is not encrypted.
#[derive(Deserialize)]
struct Profile {
    avatar: Option<String>,
}

enum Key {
    Profile(ProfileKey),
    Group([u8; 32]),
}

/// A service client signed in as this device, for requests presage has no
/// call for.
pub fn service(data: &RegistrationData) -> Option<PushService> {
    // The password is private to presage; its serde form is the way in.
    let password = serde_json::to_value(data).ok()?.get("password")?.as_str()?.to_string();
    let credentials = ServiceCredentials {
        aci: Some(data.service_ids.aci),
        pni: Some(data.service_ids.pni),
        phonenumber: (&data.phone_number).try_into_e164().ok()?,
        password: Some(password),
        device_id: data.device_id.and_then(|d| d.try_into().ok()),
    };
    Some(PushService::new(SignalServers::Production, Some(credentials), USER_AGENT))
}

/// Learns the contacts' profile keys from the account's storage service, where
/// the phone keeps them: the contact sync no longer carries any. Without its
/// key, a person's picture cannot be found or decrypted. The account's own
/// record says whether it sends read receipts.
pub async fn sync_keys(mut store: SqliteStore, service: PushService) -> Result<(), String> {
    let Some(master) = store.fetch_master_key().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let storage = StorageService::new(service, StorageServiceKey::from_master_key(&master))
        .await
        .map_err(|e| causes(&e))?;
    let manifest = storage.manifest().await.map_err(|e| causes(&e))?;
    let contacts: Vec<Vec<u8>> = manifest
        .identifiers
        .iter()
        .filter(|i| i.r#type == RecordType::Contact as i32 || i.r#type == RecordType::Account as i32)
        .map(|i| i.raw.clone())
        .collect();
    let ikm = Some(manifest.record_ikm.as_slice()).filter(|ikm| !ikm.is_empty());
    for batch in contacts.chunks(STORAGE_BATCH) {
        let records = storage.read_items(batch.to_vec(), ikm).await.map_err(|e| causes(&e))?;
        for record in records {
            let contact = match record.record {
                Some(Record::Contact(contact)) => contact,
                Some(Record::Account(account)) => {
                    READ_RECEIPTS.store(account.read_receipts, Ordering::Relaxed);
                    continue;
                }
                _ => continue,
            };
            // Newer phones only fill in the binary form of the id.
            let uuid = Uuid::from_slice(&contact.aci_binary).ok().or_else(|| contact.aci.parse().ok());
            let key = <[u8; 32]>::try_from(contact.profile_key.as_slice()).ok();
            let (Some(uuid), Some(key)) = (uuid, key) else { continue };
            let known = store.profile_key(&ServiceId::Aci(uuid.into())).await;
            if !matches!(known, Ok(Some(known)) if known.bytes == key) {
                let _ = store.upsert_profile_key(&uuid, ProfileKey::create(key)).await;
                emit(Event::ProfileKey { id: uuid.to_string() });
            }
        }
    }
    Ok(())
}

/// An error with what caused it: reqwest's own message never says why a
/// request could not be sent.
fn causes(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(e) = source {
        text.push_str(&format!(": {e}"));
        source = e.source();
    }
    text
}

/// A failure worth trying again: anything else means there is no picture.
fn transient(status: u16) -> bool {
    matches!(status, 401 | 429) || status >= 500
}

/// Where a person's picture is. `None` when they have none, or when their
/// profile key never reached this device.
async fn profile_picture(
    manager: &Signal,
    service: &PushService,
    uuid: Uuid,
) -> Result<Option<(String, Key)>, String> {
    let data = manager.registration_data();
    let key = if uuid == data.service_ids.aci {
        Some(data.profile_key())
    } else {
        manager
            .store()
            .profile_key(&ServiceId::Aci(uuid.into()))
            .await
            .map_err(|e| e.to_string())?
    };
    let Some(key) = key else { return Ok(None) };
    let version = key.get_profile_key_version(Aci::from(uuid));
    let path = format!("/v1/profile/{uuid}/{}", AsRef::<str>::as_ref(&version));
    let response = service
        .request(reqwest::Method::GET, Endpoint::service(path), HttpAuthOverride::NoOverride)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    match response.status().as_u16() {
        200 => {
            let profile: Profile = response.json().await.map_err(|e| e.to_string())?;
            Ok(profile.avatar.filter(|a| !a.is_empty()).map(|a| (a, Key::Profile(key))))
        }
        status if transient(status) => Err(format!("Signal answered {status}")),
        _ => Ok(None),
    }
}

async fn group_picture(manager: &mut Signal, key: [u8; 32]) -> Result<Option<(String, Key)>, String> {
    let mut group = manager.store().group(key).await.map_err(|e| e.to_string())?;
    if group.is_none() {
        // Only known from the imported history so far: this fetches and keeps it.
        let context = GroupContextV2 {
            master_key: Some(key.to_vec()),
            revision: Some(0),
            ..Default::default()
        };
        let _ = manager.retrieve_group_avatar(context).await;
        group = manager.store().group(key).await.map_err(|e| e.to_string())?;
    }
    Ok(group.map(|g| g.avatar).filter(|a| !a.is_empty()).map(|a| (a, Key::Group(key))))
}

async fn download(service: &PushService, picture: &str) -> Result<Option<Vec<u8>>, String> {
    let response = service
        .request(reqwest::Method::GET, Endpoint::cdn(0, picture), HttpAuthOverride::Unidentified)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    match response.status().as_u16() {
        200 => Ok(Some(response.bytes().await.map_err(|e| e.to_string())?.to_vec())),
        status if transient(status) => Err(format!("Signal's CDN answered {status}")),
        _ => Ok(None),
    }
}

/// Finds the picture of a person (ourselves included) or a group, and writes
/// it to `path` unless it is the one the app already `have`s. Returns the id
/// of the current picture, `None` when there is none.
pub async fn fetch(
    manager: &mut Signal,
    service: &PushService,
    target: &str,
    have: Option<&str>,
    path: &Path,
) -> Result<Option<String>, String> {
    let fetching = async {
        let found = if let Some(key) = target.strip_prefix(GROUP_PREFIX) {
            let key: [u8; 32] = hex::decode(key)
                .ok()
                .and_then(|k| k.try_into().ok())
                .ok_or("Unknown group.")?;
            group_picture(manager, key).await?
        } else {
            let uuid: Uuid = target.parse().map_err(|_| "Unknown chat.")?;
            profile_picture(manager, service, uuid).await?
        };
        let Some((picture, key)) = found else { return Ok(None) };
        if have == Some(picture.as_str()) {
            return Ok(Some(picture));
        }
        let Some(sealed) = download(service, &picture).await? else { return Ok(None) };
        let bytes = match key {
            Key::Profile(key) => ProfileCipher::new(key).decrypt_avatar(&sealed).ok(),
            Key::Group(key) => {
                let params = GroupSecretParams::derive_from_master_key(GroupMasterKey::new(key));
                GroupOperations::new(params).decrypt_avatar(&sealed)
            }
        };
        // Sealed with a key we no longer have, most likely.
        let Some(bytes) = bytes else { return Ok(None) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
        Ok(Some(picture))
    };
    tokio::time::timeout(TIMEOUT, fetching)
        .await
        .map_err(|_| "Signal did not answer in time.".to_string())?
}
