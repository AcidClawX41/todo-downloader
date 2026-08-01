//! Motor BitTorrent nativo (magnet + .torrent) sobre librqbit — By Eric V. Gramunt
//!
//! librqbit es un cliente BitTorrent en Rust puro (Apache-2.0), embebido como
//! librería: DHT (para que los magnet funcionen sin tracker), trackers UDP/HTTP,
//! uTP, PEX y UPnP para abrir el puerto en el router. No hay proceso externo ni
//! Python — encaja con la filosofía «todo integrado» de la app.
//!
//! Diseño deliberadamente aislado del resto del motor de descargas: los torrents
//! tienen su propio ciclo de vida (enjambre, seeding, ratio) que no cabe en el
//! modelo «una URL → un archivo» de la cola HTTP. Aquí se envuelve librqbit en
//! una fachada mínima que la UI consulta cada frame.
//!
//! `stats()` de librqbit es SÍNCRONO (devuelve una instantánea), así que el
//! progreso se lee directo en el pintado, sin canal de eventos.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use librqbit::limits::LimitsConfig;
use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions, TorrentStatsState};

/// Un torrent gestionado, tal como lo necesita la UI.
#[derive(Clone)]
pub struct Handle {
    /// Identificador interno asignado por la app (para la lista de la UI)
    pub id: u64,
    /// Handle real de librqbit
    pub inner: Arc<librqbit::ManagedTorrent>,
    /// Nombre mostrado (se rellena en cuanto se conocen los metadatos)
    pub name: String,
}

/// Instantánea de estado para pintar una fila. Todo derivado de `stats()`.
pub struct Snapshot {
    pub state: State,
    pub error: Option<String>,
    pub progress: f32, // 0.0..=1.0
    pub downloaded: u64,
    pub total: u64,
    pub uploaded: u64,
    pub finished: bool,
    /// Peers conectados en este momento (0 si el torrent no está «vivo»).
    /// BitTorrent no distingue seeders de leechers a nivel agregado sin
    /// inspeccionar cada peer, así que se muestra el total conectado.
    pub peers: usize,
}

#[derive(PartialEq, Clone, Copy)]
pub enum State {
    Initializing,
    Live,
    Paused,
    Error,
}

impl Handle {
    /// Lee el estado actual (barato y síncrono).
    pub fn snapshot(&self) -> Snapshot {
        let s = self.inner.stats();
        let total = s.total_bytes.max(1);
        // Peers conectados: solo disponibles cuando el torrent está «vivo».
        let peers = s.live.as_ref().map(|l| l.snapshot.peer_stats.live).unwrap_or(0);
        Snapshot {
            state: match s.state {
                TorrentStatsState::Initializing => State::Initializing,
                TorrentStatsState::Live => State::Live,
                TorrentStatsState::Paused => State::Paused,
                TorrentStatsState::Error => State::Error,
            },
            error: s.error,
            progress: (s.progress_bytes as f32 / total as f32).clamp(0.0, 1.0),
            downloaded: s.progress_bytes,
            total: s.total_bytes,
            uploaded: s.uploaded_bytes,
            finished: s.finished,
            peers,
        }
    }

    /// Nombre real del torrent si ya se conocen los metadatos, o el provisional.
    pub fn display_name(&self) -> String {
        // El nombre definitivo aparece en los metadatos una vez resueltos.
        let n = self.inner.name();
        match n {
            Some(name) if !name.is_empty() => name,
            _ => self.name.clone(),
        }
    }
}

/// Fachada del cliente BitTorrent. Se crea de forma perezosa (la primera vez
/// que el usuario añade un torrent) para no arrancar DHT si no se usa.
pub struct Client {
    session: Arc<Session>,
}

/// Límites de velocidad en KiB/s (0 = sin límite), como los guarda la app.
#[derive(Clone, Copy, Default)]
pub struct Limits {
    pub download_kbps: u32,
    pub upload_kbps: u32,
}

impl Limits {
    fn to_config(self) -> LimitsConfig {
        let bps = |kb: u32| NonZeroU32::new(kb.saturating_mul(1024));
        LimitsConfig {
            download_bps: bps(self.download_kbps),
            upload_bps: bps(self.upload_kbps),
        }
    }
}

impl Client {
    /// Crea la sesión (arranca DHT, escucha en el puerto) con la carpeta base y
    /// los límites de velocidad elegidos por el usuario. Operación async.
    pub async fn new(download_dir: PathBuf, limits: Limits) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&download_dir).await.ok();
        let opts = SessionOptions {
            ratelimits: limits.to_config(),
            ..Default::default()
        };
        let session = Session::new_with_opts(download_dir, opts).await?;
        Ok(Self { session })
    }

    /// Añade un magnet, una URL http(s) a un .torrent, o una ruta local .torrent.
    /// `output_folder` fija la carpeta de destino de ESTE torrent (opcional).
    /// Devuelve el handle de librqbit; el `id` lo asigna quien llama.
    pub async fn add(
        &self,
        source: &str,
        output_folder: Option<String>,
    ) -> anyhow::Result<Arc<librqbit::ManagedTorrent>> {
        let src = source.trim();
        let add = if src.starts_with("magnet:") || src.starts_with("http://") || src.starts_with("https://") {
            AddTorrent::from_url(src)
        } else {
            // Ruta local a un archivo .torrent
            let bytes = tokio::fs::read(src).await?;
            AddTorrent::from_bytes(bytes)
        };
        let resp = self
            .session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    overwrite: true,
                    output_folder,
                    ..Default::default()
                }),
            )
            .await?;
        resp.into_handle()
            .ok_or_else(|| anyhow::anyhow!("no se pudo iniciar el torrent (¿lista solo?)"))
    }

    pub async fn pause(&self, h: &Arc<librqbit::ManagedTorrent>) {
        let _ = self.session.pause(h).await;
    }

    pub async fn resume(&self, h: &Arc<librqbit::ManagedTorrent>) {
        let _ = self.session.unpause(h).await;
    }

    /// Elimina el torrent de la sesión. `delete_files` decide si borra lo bajado.
    pub async fn remove(&self, h: &Arc<librqbit::ManagedTorrent>, delete_files: bool) {
        let id = h.id();
        let _ = self.session.delete(id.into(), delete_files).await;
    }
}
