use crate::config::{
    Config, ANNOUNCE_INTERVAL_SECS, KDROP_PORT, MULTICAST_ADDR, PEER_TTL_SECS, PROTOCOL_VERSION,
};
use crate::peer::{Announcement, Peer, PeerRegistry};
use anyhow::{Context, Result};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

/// Set up the multicast UDP socket using socket2 for cross-platform reuse flags.
fn create_multicast_socket(addr: Ipv4Addr, port: u16) -> Result<std::net::UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("Failed to create UDP socket")?;

    socket
        .set_reuse_address(true)
        .context("Failed to set SO_REUSEADDR")?;

    socket
        .set_nonblocking(true)
        .context("Failed to set non-blocking")?;

    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    socket
        .bind(&bind_addr.into())
        .context("Failed to bind UDP socket")?;

    socket
        .join_multicast_v4(&addr, &Ipv4Addr::UNSPECIFIED)
        .context("Failed to join multicast group")?;

    socket.set_multicast_loop_v4(true)?;

    Ok(socket.into())
}

/// Spawns the discovery background tasks:
/// 1. Broadcasts announcements every 5 seconds.
/// 2. Listens for announcements from other peers and updates PeerRegistry.
/// 3. Periodically evicts stale peers.
pub fn start_discovery(config: Arc<Config>, registry: PeerRegistry) -> Result<()> {
    let multicast_ip: Ipv4Addr = MULTICAST_ADDR.parse()?;
    let std_sock = create_multicast_socket(multicast_ip, KDROP_PORT)?;
    let socket = Arc::new(UdpSocket::from_std(std_sock)?);

    let send_socket = socket.clone();
    let send_config = config.clone();
    let target_addr: SocketAddr = format!("{}:{}", MULTICAST_ADDR, KDROP_PORT).parse()?;

    // Task 1: Periodic announcement broadcast
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(ANNOUNCE_INTERVAL_SECS));
        loop {
            interval.tick().await;

            let announcement = Announcement {
                alias: send_config.alias.clone(),
                device_model: send_config.device_model.clone(),
                device_type: send_config.device_type.clone(),
                fingerprint: send_config.fingerprint.clone(),
                port: send_config.port,
                protocol: "http".to_string(),
                version: PROTOCOL_VERSION.to_string(),
                announce: true,
            };

            if let Ok(bytes) = serde_json::to_vec(&announcement) {
                if let Err(e) = send_socket.send_to(&bytes, target_addr).await {
                    debug!("Failed to broadcast announcement: {}", e);
                }
            }
        }
    });

    // Task 2: Incoming announcement listener
    let recv_socket = socket.clone();
    let recv_config = config.clone();
    let recv_registry = registry.clone();

    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        info!(
            "Discovery listener active on {}:{}",
            MULTICAST_ADDR, KDROP_PORT
        );

        loop {
            match recv_socket.recv_from(&mut buf).await {
                Ok((len, src_addr)) => {
                    if let Ok(announcement) = serde_json::from_slice::<Announcement>(&buf[..len]) {
                        // Ignore announcements from ourselves
                        if announcement.fingerprint == recv_config.fingerprint {
                            continue;
                        }

                        let ip = match src_addr {
                            SocketAddr::V4(v4) => v4.ip().to_string(),
                            SocketAddr::V6(v6) => v6.ip().to_string(),
                        };

                        if announcement.announce {
                            debug!(
                                "Discovered peer: {} ({}) at {}:{}",
                                announcement.alias,
                                announcement.device_model,
                                ip,
                                announcement.port
                            );
                            recv_registry.upsert(Peer {
                                alias: announcement.alias,
                                device_model: announcement.device_model,
                                device_type: announcement.device_type,
                                fingerprint: announcement.fingerprint,
                                port: announcement.port,
                                ip,
                                protocol: announcement.protocol,
                            });
                        } else {
                            debug!("Peer departed: {}", announcement.alias);
                            recv_registry.remove(&announcement.fingerprint);
                        }
                    }
                }
                Err(e) => {
                    warn!("Error receiving multicast packet: {}", e);
                }
            }
        }
    });

    // Task 3: Evict stale peers
    let evict_registry = registry;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            evict_registry.evict_stale(PEER_TTL_SECS);
        }
    });

    Ok(())
}
