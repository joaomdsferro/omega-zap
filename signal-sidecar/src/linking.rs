//! Linking as a secondary device.
//!
//! presage's own `link_secondary_device` drops the one-time key the phone
//! sends for transferring message history, so the same steps are done here on
//! top of libsignal-service, keeping that key.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures::channel::mpsc;
use futures::{StreamExt, future};
use presage::Manager;
use presage::libsignal_service::configuration::{ServiceCredentials, SignalServers};
use presage::libsignal_service::master_key::MasterKey;
use presage::libsignal_service::protocol::IdentityKeyPair;
use presage::libsignal_service::provisioning::{
    ProvisioningError, SecondaryDeviceProvisioning, link_device,
};
use presage::libsignal_service::push_service::PushService;
use presage::libsignal_service::utils::phonenumber_from_signal;
use presage::manager::RegistrationData;
use presage::store::{StateStore, Store};
use presage_store_sqlite::SqliteStore;
use rand::distr::{Alphanumeric, SampleString};

use crate::history::HistoryAccess;
use crate::{DEVICE_NAME, Event, QR_TTL_SECS, Signal, USER_AGENT, emit};

/// Tells the phone this device can import a history archive, which makes it
/// offer the transfer. These are the values Signal Desktop sends.
const QR_CAPABILITIES: &str = "backup4,backup5";

pub enum LinkError {
    /// The code was not confirmed before Signal dropped it.
    Expired,
    Failed(String),
}

fn failed(e: impl std::fmt::Display) -> LinkError {
    LinkError::Failed(e.to_string())
}

pub struct Linked {
    pub manager: Signal,
    /// Set when the phone chose to transfer its message history.
    pub history: Option<HistoryAccess>,
}

/// Shows one linking code and waits for the phone to confirm it.
pub async fn link_once(mut store: SqliteStore) -> Result<Linked, LinkError> {
    // Starting to link invalidates whatever credentials were stored before.
    store.clear_registration().await.map_err(failed)?;

    let mut rng = rand::rng();
    let password = Alphanumeric.sample_string(&mut rng, 24);
    let push_service = PushService::new(SignalServers::Production, None, USER_AGENT);
    let (tx, mut rx) = mpsc::channel(1);
    let mut aci_store = store.aci_protocol_store();
    let mut pni_store = store.pni_protocol_store();

    let (linked, registration) = future::join(
        link_device(&mut aci_store, &mut pni_store, &mut rng, push_service, &password, DEVICE_NAME, tx),
        async {
            let Some(SecondaryDeviceProvisioning::Url(mut url)) = rx.next().await else {
                return None;
            };
            url.query_pairs_mut().append_pair("capabilities", QR_CAPABILITIES);
            emit(Event::Qr { url: url.to_string(), ttl_secs: QR_TTL_SECS });
            match rx.next().await {
                Some(SecondaryDeviceProvisioning::NewDeviceRegistration(data)) => Some(data),
                _ => None,
            }
        },
    )
    .await;
    match linked {
        Ok(()) => {}
        Err(ProvisioningError::MissingMessage) => return Err(LinkError::Expired),
        Err(e) => return Err(failed(e)),
    }
    let data = registration.ok_or_else(|| failed("Signal did not confirm the new device"))?;

    store
        .set_aci_identity_key_pair(IdentityKeyPair::new(data.aci_public_key, data.aci_private_key))
        .await
        .map_err(failed)?;
    store
        .set_pni_identity_key_pair(IdentityKeyPair::new(data.pni_public_key, data.pni_private_key))
        .await
        .map_err(failed)?;
    let master_key = data
        .account_entropy_pool
        .as_ref()
        .and_then(|pool| MasterKey::from_slice(pool.derive_svr_key().as_slice()).ok());
    store.store_master_key(master_key.as_ref()).await.map_err(failed)?;
    store
        .store_account_entropy_pool(data.account_entropy_pool.as_ref())
        .await
        .map_err(failed)?;

    // The credentials in `RegistrationData` are private to presage; its serde
    // form is the way in. The profile key only parses from borrowed text,
    // hence the round trip through a string.
    let registration = serde_json::json!({
        "signal_servers": SignalServers::Production,
        "device_name": DEVICE_NAME,
        "phone_number": phonenumber_from_signal(&data.phone_number),
        "uuid": data.service_ids.aci,
        "pni": data.service_ids.pni,
        "password": password,
        "device_id": u32::from(data.device_id),
        "registration_id": data.registration_id,
        "pni_registration_id": data.pni_registration_id,
        "profile_key": BASE64.encode(data.profile_key.bytes),
    })
    .to_string();
    let registration: RegistrationData = serde_json::from_str(&registration).map_err(failed)?;
    store.save_registration_data(&registration).await.map_err(failed)?;

    let history = data.ephemeral_backup_key.map(|key| HistoryAccess {
        key,
        aci: data.service_ids.aci,
        service: PushService::new(
            SignalServers::Production,
            Some(ServiceCredentials {
                aci: Some(data.service_ids.aci),
                pni: Some(data.service_ids.pni),
                phonenumber: data.phone_number.clone(),
                password: Some(password.clone()),
                device_id: Some(data.device_id),
            }),
            USER_AGENT,
        ),
    });
    let manager = Manager::load_registered(store).await.map_err(failed)?;
    Ok(Linked { manager, history })
}
