//! Fixed-source data-only manifest updates, performed on a background worker.
use gamenode_core::drm_free::{identity::ManifestIndex,LocalError};
use sha2::{Digest,Sha256};
use std::{path::{Path,PathBuf},sync::Arc,io::Read};
use serde::{Serialize,Deserialize};
pub const SOURCE:&str="https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";
#[derive(Clone)]
pub struct ManifestSnapshot { pub hash:String,pub index:Arc<ManifestIndex> }
#[derive(Default,Serialize,Deserialize)]
struct Metadata { etag:Option<String>,checked:i64,hash:String }
pub struct ManifestCache { pub directory:PathBuf,pub baseline:PathBuf }
fn failed(e:impl std::fmt::Display)->LocalError { LocalError::Failed(e.to_string()) }
impl ManifestCache {
    pub fn bundled(data:&Path)->Result<Self,LocalError> {
        let exe=std::env::current_exe().map_err(failed)?;
        Ok(Self {directory:data.join("manifest"),baseline:exe.parent().ok_or(LocalError::InvalidTarget)?.join("backup-assets/manifest.yaml")})
    }
    fn snapshot(bytes:&[u8])->Result<ManifestSnapshot,LocalError> {
        let index=ManifestIndex::parse(bytes).map_err(failed)?;
        Ok(ManifestSnapshot {hash:format!("{:x}",Sha256::digest(bytes)),index:Arc::new(index)})
    }
    pub fn load(&self)->Result<ManifestSnapshot,LocalError> {
        let cached=self.directory.join("manifest.yaml");
        if let Ok(bytes)=read_bounded(&cached) { if let Ok(snapshot)=Self::snapshot(&bytes) { return Ok(snapshot); } }
        Self::snapshot(&read_bounded(&self.baseline)?)
    }
    /// Failed updates always leave the last usable bytes intact. Jobs retain their own Arc snapshot.
    pub fn update(&self,now:i64)->Result<ManifestSnapshot,LocalError> {
        let mut metadata:Metadata=std::fs::read(self.directory.join("metadata.json")).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default();
        if now.saturating_sub(metadata.checked)<86400 { return self.load(); }
        let current=self.load()?;
        let client=reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(60)).build().map_err(failed)?;
        let mut request=client.get(SOURCE);
        if let Some(etag)=&metadata.etag { request=request.header(reqwest::header::IF_NONE_MATCH,etag); }
        // Persist the attempt time as well, so offline startup cannot create an update storm.
        metadata.checked=now;std::fs::create_dir_all(&self.directory).map_err(failed)?;
        atomic_write(&self.directory.join("metadata.json"),&serde_json::to_vec(&metadata).map_err(failed)?)?;
        let response=match request.send() { Ok(response)=>response,Err(_)=>return Ok(current) };
        if response.status()==reqwest::StatusCode::NOT_MODIFIED { return Ok(current); }
        if response.status()!=reqwest::StatusCode::OK || response.content_length().is_some_and(|n|n>32*1024*1024) { return Ok(current); }
        let etag=response.headers().get(reqwest::header::ETAG).and_then(|v|v.to_str().ok()).map(str::to_owned);
        let mut bytes=Vec::new();response.take(32*1024*1024+1).read_to_end(&mut bytes).map_err(failed)?;
        let snapshot=match Self::snapshot(&bytes) { Ok(snapshot)=>snapshot,Err(_)=>return Ok(current) };
        atomic_write(&self.directory.join("manifest.yaml"),&bytes)?;
        metadata.etag=etag;metadata.hash=snapshot.hash.clone();
        atomic_write(&self.directory.join("metadata.json"),&serde_json::to_vec(&metadata).map_err(failed)?)?;
        Ok(snapshot)
    }
}
fn read_bounded(path:&Path)->Result<Vec<u8>,LocalError> {
    let file=std::fs::File::open(path).map_err(failed)?;
    let mut bytes=Vec::new();file.take(32*1024*1024+1).read_to_end(&mut bytes).map_err(failed)?;
    if bytes.len()>32*1024*1024 { return Err(failed("manifest exceeds size limit")); } Ok(bytes)
}
fn atomic_write(path:&Path,bytes:&[u8])->Result<(),LocalError> {
    let temporary=path.with_extension("tmp");std::fs::write(&temporary,bytes).map_err(failed)?;
    // std::fs::rename replaces a file atomically on supported Windows and Unix filesystems.
    std::fs::rename(temporary,path).map_err(failed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_cache_falls_back_and_snapshot_is_immutable() {
        let directory=tempfile::tempdir().unwrap();let baseline=directory.path().join("baseline.yaml");std::fs::write(&baseline,b"Game:\n  files: {}\n").unwrap();
        let cache=ManifestCache {directory:directory.path().join("cache"),baseline};
        std::fs::create_dir_all(&cache.directory).unwrap();std::fs::write(cache.directory.join("manifest.yaml"),b"invalid").unwrap();
        let old=cache.load().unwrap();std::fs::write(cache.directory.join("manifest.yaml"),b"Other:\n  files: {}\n").unwrap();
        assert!(old.index.games.contains_key("Game"));assert!(cache.load().unwrap().index.games.contains_key("Other"));
    }
}
