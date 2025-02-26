use crate::magnet::Magnet;
use crate::meta_info::Torrent;
use super::meta_info::Info;
use super::Ben;
use anyhow::{bail, Result};

use reqwest::Url;
use sha1::Digest;
use std::fmt;
use std::io::Write;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const BLOCK_LEN: u32 = 16 * 1024;

pub const DEFAULT_HANDSHAKE: [u8; 8] = [0u8; 8];
pub const MAGNET_HANDSHAKE: [u8; 8] = [0, 0, 0, 0, 0, 0x10, 0, 0];

const EXTENSION_HANDSHAKE: u8 = 0;

const MY_EXTENSION_ID: u8 = 93;
const METADATA_REQUEST_MSG: u8 = 0;
const METADATA_DATA_MSG: u8 = 1;

#[derive(Clone, Debug)]
pub struct PeerId([u8; 20]);
impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}
#[derive(Debug)]
pub struct Peer {
    ip: Ipv4Addr,
    port: u16,
}
impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.ip, self.port)
    }
}

fn url_encode_hash(hash: &[u8; 20]) -> String {
    use std::fmt::Write;
    let encoded = hash.iter().fold(String::new(), |mut out, e| {
        let _ = write!(out, "%{}", hex::encode([*e]));
        out
    });
    encoded
}

fn chunk_div(len: u32, chunk: u32) -> (u32, u32) {
    if len % chunk == 0 {
        (len / chunk, chunk)
    } else {
        (1 + len / chunk, len % chunk)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Message {
    Keepalive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have,
    Bitfield(Vec<u8>),
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        block: Vec<u8>,
    },
    Cancel,
    Extension(u8, Ben, Option<Ben>),
}

impl Message {
    async fn read(stream: &mut TcpStream) -> Result<Self> {
        let len = stream.read_u32().await?;
        if len == 0 {
            return Ok(Message::Keepalive);
        }

        let typ = stream.read_u8().await?;

        let msg = match typ {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => Message::Have,
            5 => {
                let mut data = vec![0u8; (len - 1) as usize];
                stream.read_exact(&mut data).await?;
                Message::Bitfield(data)
            }
            6 => {
                assert_eq!(len, 1 + 4 * 3);
                let index = stream.read_u32().await?;
                let begin = stream.read_u32().await?;
                let length = stream.read_u32().await?;
                Message::Request {
                    index,
                    begin,
                    length,
                }
            }
            7 => {
                let index = stream.read_u32().await?;
                let begin = stream.read_u32().await?;
                let mut block = vec![0u8; (len - 9) as usize];
                stream.read_exact(&mut block).await?;

                Message::Piece {
                    index,
                    begin,
                    block,
                }
            }
            8 => Message::Cancel,
            20 => {
                let extension_message_id = stream.read_u8().await?;
                let mut data = vec![0u8; (len - 2) as usize];
                stream.read_exact(&mut data).await?;
                let (ben, len) = Ben::decode_partial(&data)?;
                let extra = {
                    let extra = &data[len..];
                    if !extra.is_empty() {
                        Some(Ben::decode(extra)?)
                    } else {
                        None
                    }
                };
                Message::Extension(extension_message_id, ben, extra)
            }
            _ => bail!("Unknown message type"),
        };
        Ok(msg)
    }
    async fn write(&self, stream: &mut TcpStream) -> Result<()> {
        if self == &Message::Keepalive {
            stream.write_all(&[0u8; 3]).await?;
            return Ok(());
        }
        let data: Vec<u8> = match self {
            Message::Keepalive => vec![],
            Message::Interested => vec![2],
            Message::Request {
                index,
                begin,
                length,
            } => [
                &[6u8][..],
                &index.to_be_bytes()[..],
                &begin.to_be_bytes()[..],
                &length.to_be_bytes()[..],
            ]
            .concat(),
            Message::Extension(id, ben, extra) => {
                let extra = if let Some(extra) = extra {
                    extra.encode().to_vec()
                } else {
                    vec![]
                };
                [&[20u8, *id][..], &ben.encode()[..], &extra[..]].concat()
            }
            _ => unimplemented!(),
        };
        stream.write_u32(data.len() as u32).await?;
        stream.write_all(&data).await?;

        Ok(())
    }
}

pub struct TorrentNode {
    id: PeerId,
    port: u16,
}

impl TorrentNode {
    pub fn new(id: &str, port: u16) -> Result<Self> {
        let id: [u8; 20] = id.as_bytes().try_into()?;
        Ok(Self {
            id: PeerId(id),
            port,
        })
    }
    pub async fn get_peers(
        &self,
        tracker_url: &str,
        info_hash: &[u8; 20],
        len: u32,
    ) -> Result<Vec<Peer>> {
        let uploaded: u32 = 0;
        let downloaded: u32 = 0;
        let left = len;
        let compact = 1;

        let url = Url::parse_with_params(
            tracker_url,
            &[
                ("peer_id", String::from_utf8(self.id.0.to_vec())?),
                ("port", self.port.to_string()),
                ("uploaded", uploaded.to_string()),
                ("downloaded", downloaded.to_string()),
                ("left", left.to_string()),
                ("compact", compact.to_string()),
            ],
        )?;
        let url = format!("{}&info_hash={}", url, url_encode_hash(info_hash));
        let body = reqwest::get(url).await?.bytes().await?;
        let response = Ben::decode(&body)?;

        let peers: Vec<_> = response
            .dict("peers")?
            .bytes()?
            .chunks_exact(6)
            .map(|b| Peer {
                ip: Ipv4Addr::new(b[0], b[1], b[2], b[3]),
                port: b[4] as u16 * 256 + b[5] as u16,
            })
            .collect();

        Ok(peers)
    }

    pub async fn handshake(
        &self,
        stream: &mut TcpStream,
        info_hash: [u8; 20],
        extension_protocol: [u8; 8],
    ) -> Result<PeerId> {
        // The peer wire protocol consists of a handshake followed by a never-ending stream of length-prefixed messages.

        let msg = [
            &[19u8][..],
            &b"BitTorrent protocol"[..],
            &extension_protocol[..],
            &info_hash[..],
            &self.id.0[..],
        ]
        .concat();

        stream.write_all(&msg).await?;

        // The handshake starts with character ninteen (decimal) followed by the string 'BitTorrent protocol'.
        // The leading character is a length prefix, put there in the hope that other new protocols may do the same and
        //   thus be trivially distinguishable from each other.
        let protocol_length = stream.read_u8().await?;
        let mut protocol = vec![0u8; protocol_length as usize];
        stream.read_exact(&mut protocol).await?;
        if protocol != b"BitTorrent protocol" {
            bail!("Invalid protocol (1)");
        }

        // After the fixed headers come eight reserved bytes, which are all zero in all current implementations.
        // If you wish to extend the protocol using these bytes, please coordinate with Bram Cohen to make sure all
        // extensions are done compatibly.
        let mut zeroes = [0u8; 8];
        if zeroes.len() != stream.read_exact(&mut zeroes).await? {
            bail!("Invalid protocol (2)");
        }

        // Next comes the 20 byte sha1 hash of the bencoded form of the info value from the metainfo file.
        // (This is the same value which is announced as info_hash to the tracker, only here it's raw instead of quoted here).
        // If both sides don't send the same value, they sever the connection. The one possible exception is if a downloader
        // wants to do multiple downloads over a single port, they may wait for incoming connections to give a download hash
        // first, and respond with the same one if it's in their list.

        let mut info_hash_rec = [0u8; 20];
        stream.read_exact(&mut info_hash_rec).await?;
        if info_hash_rec != info_hash {
            bail!("Invalid protocol (4)");
        }

        // After the download hash comes the 20-byte peer id which is reported in tracker requests and contained in peer lists
        // in tracker responses. If the receiving side's peer id doesn't match the one the initiating side expects,
        // it severs the connection.

        let mut peer_id = [0u8; 20];
        if peer_id.len() != stream.read_exact(&mut peer_id).await? {
            bail!("Invalid message (5)");
        }

        Ok(PeerId(peer_id))
    }

    pub async fn bitfield(&self, stream: &mut TcpStream) -> Result<Vec<u8>> {
        let mut bit_field = None;
        while bit_field.is_none() {
            if let Message::Bitfield(bf) = Message::read(stream).await? {
                bit_field = Some(bf)
            }
        }
        Ok(bit_field.unwrap())
    }

    pub async fn interested(&self, stream: &mut TcpStream) -> Result<()> {
        Message::Interested.write(stream).await?;
        while !matches!(Message::read(stream).await?, Message::Unchoke) {}
        Ok(())
    }

    pub async fn download_piece_from_stream(
        &self,
        stream: &mut TcpStream,
        info: &Info,
        piece: u32,
    ) -> Result<Vec<u8>> {
        let (number_of_pieces, bytes_of_last_piece) =
            chunk_div(info.length, info.piece_length);

        assert!(piece < number_of_pieces);

        let piece_length = if piece == number_of_pieces - 1 {
            bytes_of_last_piece
        } else {
            info.piece_length
        };

        let (num_blocks, last_block_length) = chunk_div(piece_length, BLOCK_LEN);

        for block_no in 0..num_blocks {
            let block_length = if block_no == num_blocks - 1 {
                last_block_length
            } else {
                BLOCK_LEN
            };

            println!("Requesting piece {piece} block {block_no}");
            Message::Request {
                index: piece,
                begin: block_no * BLOCK_LEN,
                length: block_length,
            }
            .write(stream)
            .await?;
        }
        let mut blocks: Vec<Option<Vec<u8>>> = vec![None; num_blocks as usize];
        while blocks.iter().any(|b| b.is_none()) {
            if let Message::Piece {
                index,
                begin,
                block,
            } = Message::read(stream).await?
            {
                if index == piece {
                    println!(
                        "** downloading piece {} block {} len {}",
                        piece,
                        begin / BLOCK_LEN,
                        block.len()
                    );
                    blocks[(begin / BLOCK_LEN) as usize] = Some(block);
                }
            }
        }
        let mut sha1 = sha1::Sha1::default();

        let mut content = Vec::new();
        for block in blocks {
            let mut block = block.unwrap();
            sha1.update(&block);
            content.append(&mut block);
        }
        let digest = sha1.finalize().to_vec();

        if digest != info.piece_hashes[piece as usize] {
            bail!("Invalid hash")
        }

        Ok(content)
    }

    pub async fn download_piece(
        &self,
        torrent: &Torrent,
        piece: u32,
        output: PathBuf,
    ) -> Result<()> {
        let peers = self
            .get_peers(
                &torrent.tracker_url,
                &torrent.info.info_hash,
                torrent.info.length,
            )
            .await?;

        let mut stream = TcpStream::connect(&peers[0].to_string()).await?;
        let _peer_id = self
            .handshake(&mut stream, torrent.info.info_hash, DEFAULT_HANDSHAKE)
            .await?;

        self.bitfield(&mut stream).await?;
        self.interested(&mut stream).await?;

        let content = self
            .download_piece_from_stream(&mut stream, &torrent.info, piece)
            .await?;

        let mut file = std::fs::File::create(&output)?;
        file.write_all(&content)?;

        Ok(())
    }
    pub async fn download(&self, torrent: Torrent, output: PathBuf) -> Result<()> {
        let peers = self
            .get_peers(
                &torrent.tracker_url,
                &torrent.info.info_hash,
                torrent.info.length,
            )
            .await?;
        let (number_of_pieces, _bytes_of_last_piece) =
            chunk_div(torrent.info.length, torrent.info.piece_length);
        // RAII
        let mut stream = TcpStream::connect(&peers[0].to_string()).await?;
        let _peer_id = self
            .handshake(&mut stream, torrent.info.info_hash, DEFAULT_HANDSHAKE)
            .await?;

        self.bitfield(&mut stream).await?;
        self.interested(&mut stream).await?;

        let mut file = std::fs::File::create(&output)?;
        for piece_no in 0..number_of_pieces {
            let content = self
                .download_piece_from_stream(&mut stream, &torrent.info, piece_no)
                .await?;
            file.write_all(&content)?;
        }
        Ok(())
    }
    pub async fn magnet_handshake(&self, magnet: &Magnet) -> Result<(TcpStream, PeerId, u8)> {
        let peers = self
            .get_peers(&magnet.tracker_url, &magnet.info_hash, 999)
            .await?;

        let mut stream = TcpStream::connect(peers[0].to_string()).await?;
        let peer_id = self
            .handshake(&mut stream, magnet.info_hash, MAGNET_HANDSHAKE)
            .await?;

        let peer_extension_handshake = {
            let mut extension_handshake = None;
            while extension_handshake.is_none() {
                if let Message::Extension(id, eh, None) = Message::read(&mut stream).await? {
                    if id == EXTENSION_HANDSHAKE {
                        extension_handshake = Some(eh)
                    }
                }
            }
            extension_handshake.unwrap()
        };

        let my_extension_handshake = Ben::mk_dict(&[(
            Ben::mk_str("m"),
            Ben::mk_dict(&[(Ben::mk_str("ut_metadata"), Ben::mk_u8(MY_EXTENSION_ID))]),
        )]);
        Message::Extension(EXTENSION_HANDSHAKE, my_extension_handshake, None)
            .write(&mut stream)
            .await?;

        let metadata_extension_id = peer_extension_handshake
            .dict("m")?
            .dict("ut_metadata")?
            .u32()? as u8;

        Ok((stream, peer_id, metadata_extension_id))
    }

    pub async fn magnet_info(
        &self,
        stream: &mut TcpStream,
        metadata_extension_id: u8,
    ) -> Result<Info> {
        let metadata_request_extension = Ben::mk_dict(&[
            (
                Ben::mk_str("msg_type"),
                Ben::mk_u8(METADATA_REQUEST_MSG),
            ),
            (Ben::mk_str("piece"), Ben::mk_u32(0)),
        ]);
        Message::Extension(metadata_extension_id, metadata_request_extension, None)
            .write(stream)
            .await?;

        let metadata_data_msg = {
            let mut metadata_data_msg = None;

            while metadata_data_msg.is_none() {
                let msg = Message::read(stream).await?;
                if let Message::Extension(id, data, extra) = msg {
                    if id == MY_EXTENSION_ID
                        && data.dict("msg_type")?.u32()? == METADATA_DATA_MSG as u32
                    {
                        metadata_data_msg = extra
                    }
                }
            }
            metadata_data_msg.unwrap()
        };

        Info::from_ben(&metadata_data_msg)
    }

    pub async fn magnet_download_piece(
        &self,
        magnet: &Magnet,
        piece: u32,
        output: PathBuf,
    ) -> Result<()> {
        let (mut stream, _peer_id, metadata_extension_id) = self.magnet_handshake(magnet).await?;
        let info = self.magnet_info(&mut stream, metadata_extension_id).await?;
        self.interested(&mut stream).await?;
        let content = self
            .download_piece_from_stream(&mut stream, &info, piece)
            .await?;
        let mut file = std::fs::File::create(&output)?;
        file.write_all(&content)?;
        Ok(())
    }

    pub async fn magnet_download(&self, magnet: &Magnet, output: PathBuf) -> Result<()> {
        let (mut stream, _peer_id, metadata_extension_id) = self.magnet_handshake(magnet).await?;
        let info = self.magnet_info(&mut stream, metadata_extension_id).await?;
        self.interested(&mut stream).await?;

        let (number_of_pieces, _bytes_of_last_piece) =
            chunk_div(info.length, info.piece_length);

        let mut file = std::fs::File::create(&output)?;

        for piece_no in 0..number_of_pieces {
            let content = self
                .download_piece_from_stream(&mut stream, &info, piece_no)
                .await?;
            file.write_all(&content)?;
        }

        Ok(())
    }
}
