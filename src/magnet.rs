use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use reqwest::Url;

pub struct Magnet {
    pub info_hash: [u8; 20],
    pub file_name: String,
    pub tracker_url: String,
}

impl Magnet {
    pub fn parse(url: &str) -> Result<Magnet> {
        let url = Url::parse(url)?;
        if url.scheme() != "magnet" {
            bail!("bad magned scheme");
        }
        let query: HashMap<_, _> = url.query_pairs().collect();
        let Some(info_hash) = query.get("xt") else {
            bail!("no xt param");
        };
        let Some(info_hash) = info_hash.strip_prefix("urn:btih:") else {
            bail!("no urn:btih");
        };
        let info_hash: [u8; 20] = hex::decode(info_hash)?
            .try_into()
            .map_err(|_| anyhow!("bad info_hash"))?;
        let Some(file_name) = query.get("dn") else {
            bail!("no dn param");
        };
        let Some(tracker_url) = query.get("tr") else {
            bail!("no tr param");
        };
        Ok(Self {
            info_hash,
            file_name: file_name.to_string(),
            tracker_url: tracker_url.to_string(),
        })
    }
}

#[test]
fn test_parse_magnet() {
    let url = "magnet:?xt=urn:btih:ad42ce8109f54c99613ce38f9b4d87e70f24a165&dn=magnet1.gif&tr=http%3A%2F%2Fbittorrent-test-tracker.codecrafters.io%2Fannounce";
    let magnet = Magnet::parse(url).unwrap();
    assert_eq!(
        hex::encode(magnet.info_hash),
        "ad42ce8109f54c99613ce38f9b4d87e70f24a165"
    );
    assert_eq!(magnet.file_name, "magnet1.gif");
    assert_eq!(
        magnet.tracker_url,
        "http://bittorrent-test-tracker.codecrafters.io/announce"
    );
}
