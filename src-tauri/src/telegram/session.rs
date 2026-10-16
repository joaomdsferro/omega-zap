//! The Telegram session: datacenter keys, the peers met so far and where the
//! updates got to. Kept in memory and written, sealed by the vault, as JSON.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use grammers_client::session::types::{DcOption, PeerId, PeerInfo, UpdateState, UpdatesState};
use grammers_client::session::{BoxFuture, Session, SessionData};
use serde::{Deserialize, Serialize};

use crate::account::lock;

#[derive(Serialize, Deserialize)]
struct Saved {
    home_dc: i32,
    dc_options: Vec<DcOption>,
    peers: Vec<PeerInfo>,
    updates: UpdatesState,
}

#[derive(Debug)]
pub struct Infallible;

impl std::fmt::Display for Infallible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unreachable")
    }
}

impl std::error::Error for Infallible {}

pub struct TgSession {
    path: PathBuf,
    data: Mutex<SessionData>,
    dirty: AtomicBool,
}

impl TgSession {
    /// The session saved at `path`, or a new one.
    pub fn load(path: &Path) -> Self {
        let mut data = SessionData::default();
        if path.exists() {
            match crate::vault::get().read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Saved>(&b).map_err(|e| e.to_string())) {
                Ok(saved) => {
                    data.home_dc = saved.home_dc;
                    data.dc_options.extend(saved.dc_options.into_iter().map(|dc| (dc.id, dc)));
                    data.peer_infos = saved.peers.into_iter().map(|p| (p.id(), p)).collect::<HashMap<_, _>>();
                    data.updates_state = saved.updates;
                }
                Err(e) => log::warn!("telegram: session not readable, starting over: {e}"),
            }
        }
        Self { path: path.to_path_buf(), data: Mutex::new(data), dirty: AtomicBool::new(false) }
    }

    /// The signed-in user, when someone is.
    pub fn own_user(&self) -> Option<PeerInfo> {
        own(&lock(&self.data)).cloned()
    }

    /// Writes it out, when something changed since the last time.
    pub fn save(&self) {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        let json = {
            let data = lock(&self.data);
            let saved = Saved {
                home_dc: data.home_dc,
                dc_options: data.dc_options.values().cloned().collect(),
                peers: data.peer_infos.values().cloned().collect(),
                updates: data.updates_state.clone(),
            };
            serde_json::to_vec(&saved)
        };
        let written = json.map_err(|e| e.to_string()).and_then(|json| crate::vault::get().write(&self.path, &json).map_err(|e| e.to_string()));
        if let Err(e) = written {
            self.dirty.store(true, Ordering::Release);
            log::warn!("telegram: session not saved: {e}");
        }
    }

    fn changed(&self) {
        self.dirty.store(true, Ordering::Release);
    }
}

fn own(data: &SessionData) -> Option<&PeerInfo> {
    data.peer_infos.values().find(|p| matches!(p, PeerInfo::User { is_self: Some(true), .. }))
}

impl Session for TgSession {
    type Error = Infallible;

    fn home_dc_id(&self) -> Result<i32, Infallible> {
        Ok(lock(&self.data).home_dc)
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, Result<(), Infallible>> {
        Box::pin(async move {
            lock(&self.data).home_dc = dc_id;
            self.changed();
            Ok(())
        })
    }

    fn dc_option(&self, dc_id: i32) -> Result<Option<DcOption>, Infallible> {
        Ok(lock(&self.data).dc_options.get(&dc_id).cloned())
    }

    fn set_dc_option(&self, dc_option: &DcOption) -> BoxFuture<'_, Result<(), Infallible>> {
        let dc_option = dc_option.clone();
        Box::pin(async move {
            lock(&self.data).dc_options.insert(dc_option.id, dc_option);
            self.changed();
            Ok(())
        })
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, Result<Option<PeerInfo>, Infallible>> {
        Box::pin(async move {
            let data = lock(&self.data);
            // Asked for by a sentinel id, not by the user's own.
            if peer == PeerId::self_user() {
                return Ok(own(&data).cloned());
            }
            Ok(data.peer_infos.get(&peer).cloned())
        })
    }

    fn cache_peer(&self, peer: &PeerInfo) -> BoxFuture<'_, Result<(), Infallible>> {
        let peer = peer.clone();
        Box::pin(async move {
            let mut data = lock(&self.data);
            let changed = match data.peer_infos.get_mut(&peer.id()) {
                Some(known) => {
                    let before = known.clone();
                    known.extend_info(&peer);
                    *known != before
                }
                None => {
                    data.peer_infos.insert(peer.id(), peer);
                    true
                }
            };
            if changed {
                self.changed();
            }
            Ok(())
        })
    }

    fn updates_state(&self) -> BoxFuture<'_, Result<UpdatesState, Infallible>> {
        Box::pin(async move { Ok(lock(&self.data).updates_state.clone()) })
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, Result<(), Infallible>> {
        Box::pin(async move {
            let mut data = lock(&self.data);
            let state = &mut data.updates_state;
            match update {
                UpdateState::All(all) => *state = all,
                UpdateState::Primary { pts, date, seq } => {
                    state.pts = pts;
                    state.date = date;
                    state.seq = seq;
                }
                UpdateState::Secondary { qts } => state.qts = qts,
                UpdateState::Channel { id, pts } => {
                    state.channels.retain(|c| c.id != id);
                    state.channels.push(grammers_client::session::types::ChannelState { id, pts });
                }
            }
            self.changed();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grammers_client::session::types::PeerAuth;

    #[test]
    fn a_saved_session_comes_back_whole() {
        let mut data = SessionData::default();
        data.home_dc = 4;
        data.dc_options.get_mut(&4).unwrap().auth_key = Some([7; 256]);
        let me = PeerInfo::User { id: 42, auth: Some(PeerAuth::from_hash(99)), bot: Some(false), is_self: Some(true) };
        data.peer_infos.insert(me.id(), me.clone());
        data.updates_state.pts = 123;
        let saved = Saved {
            home_dc: data.home_dc,
            dc_options: data.dc_options.values().cloned().collect(),
            peers: data.peer_infos.values().cloned().collect(),
            updates: data.updates_state.clone(),
        };
        let back: Saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(back.home_dc, 4);
        assert_eq!(back.dc_options.iter().find(|dc| dc.id == 4).unwrap().auth_key, Some([7; 256]));
        assert_eq!(back.peers, vec![me.clone()]);
        assert_eq!(back.updates.pts, 123);
        assert_eq!(own(&data), Some(&me));
    }
}
