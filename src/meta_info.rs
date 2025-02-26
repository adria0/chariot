use super::Ben;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::io::Read;


#[derive(Serialize, Deserialize)]
pub struct Torrent {
    pub tracker_url: String,
    pub info : Info
}

#[derive(Serialize, Deserialize)]
pub struct Info {
    pub length: u32,
    pub info_hash: [u8; 20],
    pub piece_length: u32,
    pub piece_hashes: Vec<[u8; 20]>,
}

impl Torrent {
    pub fn from_file(path: &str) -> Result<Self> {
        let mut encoded = Vec::new();
        std::fs::File::open(path)?.read_to_end(&mut encoded)?;
        let ben = Ben::decode(&encoded)?;
        Ok(Self {
            tracker_url : ben.dict("announce")?.str()?, 
            info : Info::from_ben(ben.dict("info")?)?
        })
    }
}

impl Info {
    pub fn from_ben(ben: &Ben) -> Result<Self> {
        let pieces = ben.dict("pieces")?.bytes()?;
        let mut piece_hashes: Vec<[u8; 20]> = Vec::new();
        for hash in pieces.chunks_exact(20) {
            piece_hashes.push(hash.try_into()?);
        }

        Ok(Self {
            length: ben.dict("length")?.u32()?,
            info_hash: Sha1::digest(ben.encode()).into(),
            piece_length: ben.dict("piece length")?.u32()?,
            piece_hashes,
        })
    }
    pub fn print(&self) {
        println!("Length: {}", self.length);
        println!("Info Hash: {}", hex::encode(self.info_hash));
        println!("Piece Length: {}", self.piece_length);
        println!("Piece Hashes:");
        for hash in &self.piece_hashes {
            println!("{}", hex::encode(hash));
        }        
    }
}
