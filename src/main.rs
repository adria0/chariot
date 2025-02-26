#![allow(dead_code)]

mod ben;
mod bitmap;
mod magnet;
mod meta_info;
mod node;

use anyhow::Result;
use ben::Ben;
use clap::{arg, Parser, Subcommand};
use meta_info::Torrent;
use node::TorrentNode;
use std::path::PathBuf;
use tokio::net::TcpStream;

#[derive(Parser)]
#[command(name = "codecrafter-torrent", version = "1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Decode {
        encoded_value: String,
    },
    Info {
        path: String,
    },
    Peers {
        path: String,
    },
    Handshake {
        path: String,
        #[arg(default_value = None)]
        node: Option<String>,
    },
    #[command(name = "download_piece")]
    DownloadPiece {
        path: String,
        piece: u32,
        #[arg(short = 'o')]
        output: PathBuf,
    },

    Download {
        path: String,
        #[arg(short = 'o')]
        output: PathBuf,
    },

    #[command(name = "magnet_parse")]
    MagnetParse {
        magnet_link: String,
    },
    #[command(name = "magnet_handshake")]
    MagnetHandshake{
        magnet_link: String,
    },
    #[command(name = "magnet_info")]
    MagnetInfo{
        magnet_link: String,
    },
    #[command(name = "magnet_download_piece")]
    MagnetDownloadPiece {
        magnet_link: String,
        piece: u32,
        #[arg(short = 'o')]
        output: PathBuf,
    },
    #[command(name = "magnet_download")]
    MagnetDownload {
        magnet_link: String,
        #[arg(short = 'o')]
        output: PathBuf,
    },

}

// Usage: your_bittorrent.sh decode "<encoded_value>"
#[tokio::main]
async fn main() -> Result<()> {
    let me = TorrentNode::new("solveetcoagulasolvee", 2811)?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Decode { encoded_value } => {
            let decoded_value = Ben::decode(encoded_value.as_bytes())?;
            println!("{}", decoded_value);
        }
        Commands::Info { path } => {
            let torrent = Torrent::from_file(&path)?;

            println!("Tracker URL: {}", torrent.tracker_url);
            torrent.info.print();
        }
        Commands::Peers { path } => {
            let torrent = Torrent::from_file(&path)?;
            let peers = me.get_peers(&torrent.tracker_url, &torrent.info.info_hash, torrent.info.length).await?;

            for peer in peers {
                println!("{}", peer);
            }
        }
        Commands::Handshake { path, node } => {
            let torrent = Torrent::from_file(&path)?;
            let node = if let Some(node) = node {
                node.to_string()
            } else {
                let peers = me.get_peers(&torrent.tracker_url, &torrent.info.info_hash, torrent.info.length).await?;
                format!("{}", peers[0])
            };

            let mut stream = TcpStream::connect(&node).await?;
            let peer_id = me.handshake(&mut stream, torrent.info.info_hash, node::DEFAULT_HANDSHAKE).await?;

            println!("Peer ID: {}", peer_id);
        }
        Commands::DownloadPiece {
            path,
            piece,
            output,
        } => {
            let torrent = Torrent::from_file(&path)?;
            me.download_piece(&torrent,piece, output).await?;
        }
        Commands::Download { path, output } => {
            let torrent = Torrent::from_file(&path)?;
            me.download(torrent, output).await?;
        }
        Commands::MagnetParse { magnet_link } => {
            let magnet = magnet::Magnet::parse(&magnet_link)?;
            println!("Tracker URL: {}", magnet.tracker_url);
            println!("Info Hash: {}", hex::encode(magnet.info_hash));
        }
        Commands::MagnetHandshake { magnet_link } => {
            let magnet = magnet::Magnet::parse(&magnet_link)?;
            let (_stream, peer_id, metadata_extension_id) = me.magnet_handshake(&magnet).await?;
            println!("Peer ID: {peer_id}");
            println!("Peer Metadata Extension ID: {metadata_extension_id}")
        }
        Commands::MagnetInfo { magnet_link } => {
            let magnet = magnet::Magnet::parse(&magnet_link)?;
            let (mut stream, _peer_id, metadata_extension_id) = me.magnet_handshake(&magnet).await?;
            let info = me.magnet_info(&mut stream, metadata_extension_id).await?;
            println!("Tracker URL: {}", magnet.tracker_url);
            info.print();
        }
        Commands::MagnetDownloadPiece {
            magnet_link,
            piece,
            output,
        } => {
            let magnet = magnet::Magnet::parse(&magnet_link)?;
            me.magnet_download_piece(&magnet,piece,output).await?;
        }
        Commands::MagnetDownload {
            magnet_link,
            output,
        } => {
            let magnet = magnet::Magnet::parse(&magnet_link)?;
            me.magnet_download(&magnet,output).await?;
        }
    }
    Ok(())
}

