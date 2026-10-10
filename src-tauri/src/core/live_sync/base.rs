//! The sync BASE (plan §2.6): what rexenv last saw on the live site — every
//! table's change stamp and every file's `size:mtime` — recorded after each pull
//! or push, so the next push can tell the plugin what it is allowed to overwrite
//! and the plugin can refuse what changed on live since. Plain JSON beside the
//! pairing file; nothing in it is secret.

use crate::error::Result;
use crate::platform::traits::Platform;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncBase {
    pub tables: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    /// Unix seconds of the pull/push that recorded it.
    pub at: i64,
}

fn file(platform: &dyn Platform, site_id: &str) -> Result<PathBuf> {
    Ok(super::secrets::dir(platform)?.join(format!("{site_id}.base.json")))
}

pub fn get(platform: &dyn Platform, site_id: &str) -> Result<Option<SyncBase>> {
    match std::fs::read(file(platform, site_id)?) {
        Ok(b) => Ok(serde_json::from_slice(&b).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn put(platform: &dyn Platform, site_id: &str, base: &SyncBase) -> Result<()> {
    let p = file(platform, site_id)?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_vec(base).expect("json"))?;
    Ok(())
}

pub fn delete(platform: &dyn Platform, site_id: &str) -> Result<()> {
    match std::fs::remove_file(file(platform, site_id)?) {
        Ok(()) | Err(_) => Ok(()),
    }
}
