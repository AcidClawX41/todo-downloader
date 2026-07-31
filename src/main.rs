//! Todo Downloader — By Eric V. Gramunt
//! Gestor de descargas tipo JDownloader2 en Rust + egui. Ligero, moderno, sin bloatware.
//! yt-dlp integrado: se instala automáticamente desde Ajustes (release oficial de GitHub).
//!
//! Funciones:
//! - Cola con descargas concurrentes (tokio + reqwest), progreso y velocidad en vivo
//! - Pausa / reanudación real (HTTP Range sobre archivos .part)
//! - LinkGrabber: captura automática de enlaces TikTok desde el portapapeles
//! - Importación de TXT / JSON exportados por "TikTok Video Downloader HQ.js"
//! - Arrastrar y soltar archivos TXT/JSON sobre la ventana
//! - Fallback automático a yt-dlp cuando un enlace CDN ha caducado (403/404)
//! - Subcarpeta por autor, nombres sanitizados, reintentos con backoff
//! - UI moderna: sidebar, tarjetas de estadísticas, tema oscuro con acento TikTok

#![cfg_attr(windows, windows_subsystem = "windows")]

mod i18n;
mod receiver;
mod scripts;
use i18n::{t, Lang};

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Color32, Margin, RichText, Rounding, Stroke};
use egui_extras::{Column, TableBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Semaphore;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const URL_RE: &str = r#"https?://[^\s"'<>]+"#;
const MAX_RETRIES: u32 = 3;

// ---------- Paleta (dark, acento TikTok) ----------
const BG: Color32 = Color32::from_rgb(15, 17, 21); // fondo ventana
const PANEL: Color32 = Color32::from_rgb(21, 24, 31); // sidebar
const CARD: Color32 = Color32::from_rgb(30, 34, 45); // tarjetas / inputs
const CARD_HOVER: Color32 = Color32::from_rgb(38, 43, 57);
const ACCENT: Color32 = Color32::from_rgb(254, 44, 85); // rosa TikTok
const CYAN: Color32 = Color32::from_rgb(37, 244, 238); // cian TikTok
const TEXT: Color32 = Color32::from_rgb(232, 234, 240);
const MUTED: Color32 = Color32::from_rgb(138, 144, 160);
const GREEN: Color32 = Color32::from_rgb(61, 220, 132);
const AMBER: Color32 = Color32::from_rgb(255, 180, 84);
const RED: Color32 = Color32::from_rgb(255, 84, 112);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1160.0, 700.0])
            // Mínimo calculado para que la tabla nunca recorte la columna de acciones
            .with_min_inner_size([1000.0, 520.0])
            .with_title("Todo Downloader"),
        ..Default::default()
    };
    eframe::run_native(
        "todo-downloader-evg",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

// ============================= Modelo =============================

#[derive(Clone, PartialEq)]
enum Status {
    Queued,
    Waiting,
    Downloading,
    Resolving, // yt-dlp
    Paused,
    Done,
    Error(String),
}

impl Status {
    /// `engine` permite mostrar el motor real que está trabajando: antes la
    /// etiqueta decía siempre «yt-dlp», incluso cuando corría gallery-dl.
    fn label(&self, lang: Lang, engine: Engine) -> String {
        match self {
            Status::Queued => t(lang, "status.queued").into(),
            Status::Waiting => t(lang, "status.waiting").into(),
            Status::Downloading => t(lang, "status.downloading").into(),
            Status::Resolving => match engine {
                Engine::GalleryDl => "gallery-dl".into(),
                _ => t(lang, "status.resolving").into(),
            },
            Status::Paused => t(lang, "status.paused").into(),
            Status::Done => t(lang, "status.done").into(),
            // Etiqueta corta: el mensaje íntegro se ve al pasar el ratón, así
            // la pill nunca se sale de su columna.
            Status::Error(_) => t(lang, "status.error").into(),
        }
    }

    /// Mensaje resumido para el tooltip cuando no hay detalle completo
    fn brief(&self) -> String {
        match self {
            Status::Error(e) => e.clone(),
            _ => String::new(),
        }
    }
    fn color(&self) -> Color32 {
        match self {
            Status::Done => GREEN,
            Status::Downloading | Status::Resolving => CYAN,
            Status::Paused => AMBER,
            Status::Error(_) => RED,
            _ => MUTED,
        }
    }
    fn is_active(&self) -> bool {
        matches!(self, Status::Waiting | Status::Downloading | Status::Resolving)
    }
}

struct Row {
    id: u64,
    filename: String,
    url: String,
    page_url: String,
    author: String,
    engine: Engine,
    size: u64,
    downloaded: u64,
    speed: f64,
    status: Status,
    gal_files: u64,
    error_full: String,
    cancel: Arc<AtomicBool>,
}

// Algunas variantes solo se emiten en Windows (instalador de ffmpeg)
#[cfg_attr(not(windows), allow(dead_code))]
enum Ev {
    Status(u64, Status),
    Size(u64, u64),
    Progress(u64, u64, f64),
    Clipboard(Vec<String>),
    Received(Vec<receiver::Incoming>),
    GalFiles(u64, u64),
    ErrorDetail(u64, String),
    CookieFallback,
    DisableCookies,
    YtDlp(Option<String>),
    YtDlpProgress(f32),
    YtDlpError(String),
    GalDl(Option<String>),
    GalDlProgress(f32),
    GalDlError(String),
    Ffmpeg(Option<String>),
    FfmpegProgress(f32),
    FfmpegError(String),
    ProfileEntries(Vec<ProfileEntry>),
    ProfileError(String),
}

/// Motor de descarga por tarea
#[derive(Clone, Copy, PartialEq)]
enum Engine {
    Http,      // enlace directo a archivo (CDN)
    YtDlp,     // página de vídeo (TikTok, YouTube, Instagram, X…)
    GalleryDl, // post de imágenes (TikTok /photo/, Douyin /note/)
}

/// Entrada detectada al analizar un perfil
#[derive(Clone)]
struct ProfileEntry {
    selected: bool,
    id: String,
    title: String,
    url: String,
    is_image: bool,
}

/// Sitios que el LinkGrabber captura por defecto
const KNOWN_SITES: &[&str] = &[
    "tiktok.com", "douyin.com", "youtube.com", "youtu.be", "instagram.com",
    "twitter.com", "x.com", "reddit.com", "twitch.tv", "vimeo.com",
    "facebook.com", "bilibili.com", "soundcloud.com", "dailymotion.com",
];

/// ¿Es un enlace directo a archivo (descargable por HTTP puro)?
fn is_direct_media(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or("").to_lowercase();
    const EXTS: &[&str] = &[
        ".mp4", ".webm", ".mov", ".mkv", ".avi", ".mp3", ".m4a", ".wav",
        ".jpg", ".jpeg", ".png", ".webp", ".gif", ".zip", ".rar", ".7z", ".pdf",
    ];
    EXTS.iter().any(|e| path.ends_with(e))
        || url.contains("tiktokcdn")
        || url.contains("douyinvod")
        || url.contains("mime_type=video")
}

/// Sitios cuyo contenido es mayoritariamente galerías de imágenes y cuyo
/// listado de perfil yt-dlp no puede enumerar: los gestiona gallery-dl entero.
const GALLERY_SITES: &[&str] = &[
    "instagram.com", "pinterest.com", "pinterest.es", "deviantart.com",
    "flickr.com", "tumblr.com", "artstation.com", "danbooru", "gelbooru",
    // Weibo: gallery-dl trae extractores de perfil, álbum, post y vídeo,
    // y descarga fotos y vídeos del mismo post en una sola pasada.
    "weibo.com", "weibo.cn",
];

fn is_gallery_site(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    GALLERY_SITES.iter().any(|s| u.contains(s))
}

/// Normaliza URLs de perfil que los extractores no reconocen con parámetros.
/// Weibo es el caso claro: `weibo.com/u/123?layerid=…` (la URL que copia el
/// navegador al abrir un post) no la reconoce gallery-dl, pero `weibo.com/u/123` sí.
fn normalize_profile_url(url: &str) -> String {
    let u = url.trim();
    let low = u.to_ascii_lowercase();
    if (low.contains("weibo.com") || low.contains("weibo.cn")) && low.contains("/u/") {
        if let Some(base) = u.split('?').next() {
            return base.trim_end_matches('/').to_string();
        }
    }
    u.to_string()
}

/// Motor capaz de resolver una URL de PÁGINA cuando el enlace directo falla.
/// `None` = ningún motor la soporta (p. ej. douyin.com/note/…: yt-dlp no tiene
/// extractor de notas y gallery-dl no tiene extractor de Douyin).
fn fallback_engine(page_url: &str) -> Option<Engine> {
    let u = page_url.to_ascii_lowercase();

    // Douyin: solo los vídeos sueltos tienen extractor; las notas (carruseles
    // de imágenes) no las soporta ninguno de los dos motores.
    if u.contains("douyin.com") {
        return if u.contains("/video/") { Some(Engine::YtDlp) } else { None };
    }
    // Posts de imágenes de TikTok
    if u.contains("/photo/") {
        return Some(Engine::GalleryDl);
    }
    if is_gallery_site(&u) {
        return Some(Engine::GalleryDl);
    }
    Some(Engine::YtDlp)
}

/// URL de PERFIL de Douyin. Ni yt-dlp ni gallery-dl tienen extractor de perfiles
/// para Douyin (solo vídeos sueltos), así que no se puede enumerar: hay que usar
/// el script de consola del navegador.
fn is_douyin_profile(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("douyin.com") && (u.contains("/user/") || !u.contains("/video/"))
}

fn engine_for_url(url: &str) -> Engine {
    if is_gallery_site(url) {
        return Engine::GalleryDl;
    }
    if is_direct_media(url) {
        Engine::Http
    } else if url.contains("/photo/") || url.contains("/note/") {
        Engine::GalleryDl
    } else {
        Engine::YtDlp
    }
}

/// Variantes de una URL de imagen ordenadas de mayor a menor calidad.
///
/// Los CDN de ByteDance (Douyin/TikTok) codifican el procesado en la ruta con
/// `~tplv-…`: marca de agua, reescalado y recompresión. `~noop` significa «sin
/// procesar», es decir, el original sin marca y a resolución completa.
/// Se devuelve también la URL tal cual como último recurso.
fn quality_variants(url: &str) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(pos) = url.find("~tplv-") {
        // Separar la parte procesada de la extensión y el query
        let (head, rest) = url.split_at(pos);
        let after = &rest[1..]; // sin la '~'
        let ext = after
            .split(['?', '#'])
            .next()
            .and_then(|s| s.rsplit('.').next())
            .filter(|e| (2..=5).contains(&e.len()))
            .unwrap_or("jpeg")
            .to_string();
        let query = url.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();

        // Original sin procesar (sin marca de agua, resolución completa)
        out.push(format!("{head}~noop.{ext}{query}"));
        out.push(format!("{head}~tplv-obj.{ext}{query}"));
    }

    out.push(url.to_string()); // la original tal cual siempre al final
    out.dedup();
    out
}

/// Referer correcto para cada CDN. Los servidores de imágenes de ByteDance y
/// Weibo comprueban el origen (anti-hotlink) y rechazan un referer ajeno.
fn referer_for(url: &str) -> &'static str {
    let u = url.to_ascii_lowercase();
    if u.contains("douyin") || u.contains("douyinpic") || u.contains("douyinvod") {
        "https://www.douyin.com/"
    } else if u.contains("sinaimg") || u.contains("weibo") {
        "https://weibo.com/"
    } else if u.contains("cdninstagram") || u.contains("instagram") {
        "https://www.instagram.com/"
    } else if u.contains("tiktok") || u.contains("bytecdn") || u.contains("ibyteimg") {
        "https://www.tiktok.com/"
    } else {
        ""
    }
}

/// Extensión real del archivo a partir de la URL (sin query ni fragmento)
fn url_extension(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let name = path.rsplit('/').next()?;
    let ext = name.rsplit('.').next()?;
    // Solo extensiones plausibles: evita tomar trozos del hash del CDN
    if (2..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(ext.to_ascii_lowercase())
    } else {
        None
    }
}

/// Hash corto y estable de la URL, para nombrar archivos sin identificador
fn short_hash(url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    format!("{:x}", h.finish() & 0xFFFF_FFFF)
}

enum RowAction {
    Pause,
    Resume,
    OpenDir,
    Remove,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Downloads,
    Profile,
    Capture,
    Done,
    Failed,
    Settings,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Settings {
    dest: String,
    concurrency: usize,
    per_author: bool,
    clipboard_watch: bool,
    auto_start_clipboard: bool,
    grab_any_url: bool,
    use_browser_cookies: bool,
    cookies_browser: String,
    cookies_file: String,
    lang: Lang,
    receiver_enabled: bool,
    receiver_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dest: dirs_download().join("Todo Downloads").to_string_lossy().into_owned(),
            concurrency: 3,
            per_author: true,
            clipboard_watch: true,
            auto_start_clipboard: false,
            grab_any_url: false,
            use_browser_cookies: false,
            cookies_browser: "firefox".into(),
            cookies_file: String::new(),
            lang: Lang::detect(),
            receiver_enabled: true,
            receiver_port: 9777,
        }
    }
}

/// Argumentos de cookies. Un archivo cookies.txt tiene prioridad porque no
/// depende de que el navegador esté cerrado.
fn cookie_args(s: &Settings) -> Vec<String> {
    if !s.cookies_file.trim().is_empty() {
        vec!["--cookies".into(), s.cookies_file.trim().to_string()]
    } else if s.use_browser_cookies {
        vec!["--cookies-from-browser".into(), s.cookies_browser.clone()]
    } else {
        Vec::new()
    }
}

/// Detecta fallos de acceso a las cookies del navegador en Windows:
///  - DB bloqueada porque el navegador está abierto
///  - App-Bound Encryption de Chrome 127+ (DPAPI ya no puede descifrar)
///  - permisos insuficientes
fn is_cookie_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("dpapi")
        || m.contains("failed to decrypt")
        || m.contains("cookie database")
        || (m.contains("could not copy") && m.contains("cookie"))
        || (m.contains("permission denied") && m.contains("cookie"))
        || (m.contains("cookies") && m.contains("decrypt"))
}

fn dirs_download() -> PathBuf {
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        PathBuf::from(home).join("Downloads")
    } else {
        PathBuf::from(".")
    }
}

fn sanitize(name: &str, maxlen: usize) -> String {
    let re = Regex::new(r#"[<>:"/\\|?*\x00-\x1f]"#).unwrap();
    let s = re.replace_all(name, "").trim().to_string();
    let s: String = s.chars().take(maxlen).collect();
    let s = s.trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        return "video".into();
    }
    // Nombres de dispositivo reservados en Windows (CON, NUL, COM1…)
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
        "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        format!("_{s}")
    } else {
        s
    }
}

fn fmt_size(b: f64) -> String {
    let mut b = b;
    for unit in ["B", "KB", "MB", "GB"] {
        if b < 1024.0 || unit == "GB" {
            return format!("{b:.1} {unit}");
        }
        b /= 1024.0;
    }
    unreachable!()
}

// ============================= App =============================

struct App {
    rows: Vec<Row>,
    next_id: u64,
    settings: Settings,
    rt: tokio::runtime::Runtime,
    tx: UnboundedSender<Ev>,
    rx: UnboundedReceiver<Ev>,
    sem: Arc<Semaphore>,
    sem_permits: usize,
    client: reqwest::Client,
    clip_enabled: Arc<AtomicBool>,
    grab_any_flag: Arc<AtomicBool>,
    recv_enabled: Arc<AtomicBool>,
    capture_site: usize, // 0 = TikTok, 1 = Douyin
    ytdlp_ok: Option<bool>,
    ytdlp_cmd: Option<String>,
    ytdlp_installing: bool,
    ytdlp_progress: f32,
    galdl_cmd: Option<String>,
    galdl_installing: bool,
    galdl_progress: f32,
    ffmpeg_cmd: Option<String>,
    ffmpeg_installing: bool,
    ffmpeg_progress: f32,
    profile_url: String,
    profile_want_videos: bool,
    profile_want_images: bool,
    profile_analyzing: bool,
    profile_entries: Vec<ProfileEntry>,
    view: View,
    show_add: bool,
    add_text: String,
    search: String,
    toast: String,
    toast_until: Option<Instant>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();

        // Migración de rutas por defecto antiguas (…/TikTok, …/TodoDownloader)
        // a la nueva carpeta genérica "Todo Downloads", solo si nunca se personalizó.
        for legacy in ["TikTok", "TodoDownloader"] {
            if settings.dest == dirs_download().join(legacy).to_string_lossy().as_ref() {
                settings.dest = Settings::default().dest;
                break;
            }
        }

        load_cjk_font(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("tokio runtime");

        let (tx, rx) = unbounded_channel::<Ev>();

        // Sin Referer fijo: se calcula por petición según el dominio (ver
        // `referer_for`). Mandar el de TikTok a un CDN de Douyin activaba su
        // protección anti-hotlink y tumbaba las descargas de imágenes.
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::USER_AGENT, UA.parse().unwrap());
        headers.insert(
            reqwest::header::ACCEPT,
            "image/avif,image/webp,image/apng,image/*,video/*,*/*;q=0.8".parse().unwrap(),
        );
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");

        let clip_enabled = Arc::new(AtomicBool::new(settings.clipboard_watch));
        let grab_any_flag = Arc::new(AtomicBool::new(settings.grab_any_url));
        spawn_clipboard_watcher(tx.clone(), clip_enabled.clone(), grab_any_flag.clone());

        // Receptor local (Click'n'Load): recibe los enlaces del script del navegador
        let recv_enabled = Arc::new(AtomicBool::new(settings.receiver_enabled));
        {
            let tx_r = tx.clone();
            receiver::spawn(settings.receiver_port, recv_enabled.clone(), move |items| {
                let _ = tx_r.send(Ev::Received(items));
            });
        }
        spawn_ytdlp_check(tx.clone());
        spawn_galdl_check(tx.clone());
        spawn_ffmpeg_check(tx.clone());

        Self {
            rows: Vec::new(),
            next_id: 0,
            sem: Arc::new(Semaphore::new(settings.concurrency)),
            sem_permits: settings.concurrency,
            settings,
            rt,
            tx,
            rx,
            client,
            clip_enabled,
            grab_any_flag,
            recv_enabled,
            capture_site: 0,
            ytdlp_ok: None,
            ytdlp_cmd: None,
            ytdlp_installing: false,
            ytdlp_progress: 0.0,
            galdl_cmd: None,
            galdl_installing: false,
            galdl_progress: 0.0,
            ffmpeg_cmd: None,
            ffmpeg_installing: false,
            ffmpeg_progress: 0.0,
            profile_url: String::new(),
            profile_want_videos: true,
            profile_want_images: true,
            profile_analyzing: false,
            profile_entries: Vec::new(),
            view: View::Downloads,
            show_add: false,
            add_text: String::new(),
            search: String::new(),
            toast: String::new(),
            toast_until: None,
        }
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = msg.into();
        self.toast_until = Some(Instant::now() + Duration::from_secs(4));
    }

    fn idx(&self, id: u64) -> Option<usize> {
        self.rows.iter().position(|r| r.id == id)
    }

    // -------------------- Alta de tareas --------------------

    fn add_url(&mut self, url: &str, author: &str, title: &str, page_url: &str, vid_id: &str) {
        let trimmed = url.trim();
        // Canal seguro: forzar HTTPS en cualquier enlace http:// (TLS obligatorio)
        let upgraded;
        let url: &str = if let Some(rest) = trimmed.strip_prefix("http://") {
            upgraded = format!("https://{rest}");
            &upgraded
        } else {
            trimmed
        };
        if url.is_empty() || self.rows.iter().any(|r| r.url == url) {
            return;
        }
        self.next_id += 1;
        let engine = engine_for_url(url);

        // Identificador del post: el que envía el capturador, el de la URL,
        // o un hash corto y estable de la propia URL (nunca un contador, que
        // producía nombres opacos tipo «100001.mp4»).
        let vid = if !vid_id.is_empty() {
            vid_id.to_string()
        } else if let Some(m) = Regex::new(r"/(?:video|note|photo)/(\d+)").unwrap().captures(url) {
            m[1].to_string()
        } else {
            short_hash(url)
        };

        let mut parts: Vec<&str> = Vec::new();
        if !author.is_empty() {
            parts.push(author);
        }
        parts.push(&vid);
        let t = sanitize(title, 40);
        if !t.is_empty() && !title.trim().is_empty() {
            parts.push(&t);
        }
        let stem = sanitize(&parts.join("_"), 110);

        let filename = match engine {
            // Enlace directo: conservamos la extensión real (.jpeg, .webp, .mp4…)
            // pero con un nombre legible, no el identificador del CDN.
            Engine::Http => {
                let ext = url_extension(url).unwrap_or_else(|| "mp4".into());
                format!("{stem}.{ext}")
            }
            // gallery-dl baja el post/galería completo: puede incluir vídeo,
            // así que la etiqueta no debe prometer solo imágenes.
            Engine::GalleryDl => {
                format!("{stem} {}", i18n::t(self.settings.lang, "label.gallery"))
            }
            Engine::YtDlp => format!("{stem}.mp4"),
        };

        self.rows.push(Row {
            id: self.next_id,
            filename,
            url: url.to_string(),
            page_url: page_url.to_string(),
            author: author.to_string(),
            engine,
            size: 0,
            downloaded: 0,
            speed: 0.0,
            status: Status::Queued,
            gal_files: 0,
            error_full: String::new(),
            cancel: Arc::new(AtomicBool::new(false)),
        });
    }

    fn add_plain_urls(&mut self, text: &str) -> usize {
        let mut n = 0;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("http") {
                self.add_url(line, "", "", "", "");
                n += 1;
            }
        }
        n
    }

    fn import_path(&mut self, path: &std::path::Path) {
        let lang = self.settings.lang;
        let Ok(content) = std::fs::read_to_string(path) else {
            self.toast(i18n::read_error(lang, &path.display().to_string()));
            return;
        };
        if path.extension().map(|e| e.eq_ignore_ascii_case("json")).unwrap_or(false) {
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(v) => {
                    let mut n = 0;
                    if let Some(videos) = v.get("videos").and_then(|x| x.as_array()) {
                        for vd in videos {
                            let g = |k: &str| vd.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                            let url = [g("hqUrl"), g("playAddr"), g("downloadAddr"), g("pageUrl")]
                                .into_iter()
                                .find(|u| !u.is_empty())
                                .unwrap_or_default();
                            if url.is_empty() {
                                continue;
                            }
                            self.add_url(&url, &g("author"), &g("title"), &g("pageUrl"), &g("id"));
                            n += 1;
                        }
                    }
                    self.toast(i18n::imported_json(lang, n));
                }
                Err(e) => self.toast(i18n::invalid_json(lang, &e.to_string())),
            }
        } else {
            let n = self.add_plain_urls(&content);
            self.toast(i18n::imported_txt(lang, n));
        }
    }

    // -------------------- Motor --------------------

    fn refresh_semaphore(&mut self) {
        if self.sem_permits != self.settings.concurrency {
            self.sem = Arc::new(Semaphore::new(self.settings.concurrency));
            self.sem_permits = self.settings.concurrency;
        }
    }

    fn dest_dir(&self, author: &str) -> PathBuf {
        let mut d = PathBuf::from(&self.settings.dest);
        if self.settings.per_author && !author.is_empty() {
            d = d.join(sanitize(author, 60));
        }
        d
    }

    fn start_row(&mut self, i: usize) {
        if self.rows[i].status.is_active() || self.rows[i].status == Status::Done {
            return;
        }
        self.refresh_semaphore();
        let dir = self.dest_dir(&self.rows[i].author);
        let _ = std::fs::create_dir_all(&dir);

        let row = &mut self.rows[i];
        row.cancel = Arc::new(AtomicBool::new(false));
        row.status = Status::Waiting;
        row.speed = 0.0;

        let spec = DlSpec {
            id: row.id,
            url: row.url.clone(),
            page_url: row.page_url.clone(),
            path: dir.join(&row.filename),
            engine: row.engine,
            extra_args: cookie_args(&self.settings),
            ffmpeg: self.ffmpeg_cmd.clone(),
            cancel: row.cancel.clone(),
        };
        let client = self.client.clone();
        let sem = self.sem.clone();
        let tx = self.tx.clone();
        let ytdlp = self.ytdlp_cmd.clone();
        let galdl = self.galdl_cmd.clone();
        self.rt.spawn(async move {
            download_task(client, spec, sem, tx, ytdlp, galdl).await;
        });
    }

    fn start_all(&mut self) {
        let idxs: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r.status, Status::Queued | Status::Paused))
            .map(|(i, _)| i)
            .collect();
        let n = idxs.len();
        for i in idxs {
            self.start_row(i);
        }
        if n > 0 {
            let msg = i18n::starting(self.settings.lang, n);
            self.toast(msg);
        }
    }

    fn pause_all(&mut self) {
        for r in &mut self.rows {
            if r.status.is_active() {
                r.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    fn retry_failed(&mut self) {
        let idxs: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r.status, Status::Error(_)))
            .map(|(i, _)| i)
            .collect();
        for i in idxs {
            self.rows[i].status = Status::Queued;
            self.rows[i].downloaded = 0;
            self.rows[i].error_full.clear();
            self.start_row(i);
        }
    }

    fn clear_done(&mut self) {
        self.rows.retain(|r| r.status != Status::Done);
    }

    // -------------------- Eventos --------------------

    fn drain_events(&mut self) {
        let mut clip_batch: Vec<String> = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Ev::Status(id, st) => {
                    if let Some(i) = self.idx(id) {
                        self.rows[i].status = st;
                        if self.rows[i].status == Status::Done {
                            self.rows[i].speed = 0.0;
                            if self.rows[i].size > 0 {
                                self.rows[i].downloaded = self.rows[i].size;
                            }
                        }
                    }
                }
                Ev::Size(id, sz) => {
                    if let Some(i) = self.idx(id) {
                        self.rows[i].size = sz;
                    }
                }
                Ev::Progress(id, done, speed) => {
                    if let Some(i) = self.idx(id) {
                        self.rows[i].downloaded = done;
                        self.rows[i].speed = speed;
                        // yt-dlp ya está transfiriendo: pasa de "resolviendo" a "descargando"
                        if self.rows[i].status == Status::Resolving && done > 0 {
                            self.rows[i].status = Status::Downloading;
                        }
                    }
                }
                Ev::Clipboard(links) => clip_batch.extend(links),
                Ev::Received(items) => {
                    let before = self.rows.len();
                    for it in &items {
                        self.add_url(&it.url, &it.author, &it.title, &it.page_url, &it.id);
                    }
                    let added = self.rows.len() - before;
                    if added > 0 {
                        let msg = i18n::received(self.settings.lang, added);
                        self.toast(msg);
                        if self.settings.auto_start_clipboard {
                            self.start_all();
                        }
                    }
                }
                Ev::GalFiles(id, n) => {
                    if let Some(i) = self.idx(id) {
                        self.rows[i].gal_files = n;
                        if self.rows[i].status == Status::Resolving {
                            self.rows[i].status = Status::Downloading;
                        }
                    }
                }
                Ev::ErrorDetail(id, detail) => {
                    if let Some(i) = self.idx(id) {
                        self.rows[i].error_full = detail;
                    }
                }
                Ev::CookieFallback => {
                    let msg = t(self.settings.lang, "toast.cookie_fallback");
                    self.toast(msg);
                }
                Ev::DisableCookies => {
                    // Las cookies del navegador no son legibles (App-Bound Encryption
                    // u otro fallo): se desactivan para no reintentar en cada descarga.
                    if self.settings.use_browser_cookies {
                        self.settings.use_browser_cookies = false;
                        let msg = t(self.settings.lang, "toast.cookies_disabled");
                        self.toast(msg);
                    }
                }
                Ev::YtDlp(cmd) => {
                    self.ytdlp_ok = Some(cmd.is_some());
                    self.ytdlp_cmd = cmd;
                    self.ytdlp_installing = false;
                }
                Ev::YtDlpProgress(p) => {
                    self.ytdlp_installing = true;
                    self.ytdlp_progress = p;
                }
                Ev::YtDlpError(e) => {
                    self.ytdlp_installing = false;
                    let msg = i18n::install_error(self.settings.lang, "yt-dlp", &e);
                    self.toast(msg);
                }
                Ev::GalDl(cmd) => {
                    self.galdl_cmd = cmd;
                    self.galdl_installing = false;
                }
                Ev::GalDlProgress(p) => {
                    self.galdl_installing = true;
                    self.galdl_progress = p;
                }
                Ev::GalDlError(e) => {
                    self.galdl_installing = false;
                    let msg = i18n::install_error(self.settings.lang, "gallery-dl", &e);
                    self.toast(msg);
                }
                Ev::Ffmpeg(cmd) => {
                    self.ffmpeg_cmd = cmd;
                    self.ffmpeg_installing = false;
                }
                Ev::FfmpegProgress(p) => {
                    self.ffmpeg_installing = true;
                    self.ffmpeg_progress = p;
                }
                Ev::FfmpegError(e) => {
                    self.ffmpeg_installing = false;
                    let msg = i18n::install_error(self.settings.lang, "ffmpeg", &e);
                    self.toast(msg);
                }
                Ev::ProfileEntries(entries) => {
                    self.profile_analyzing = false;
                    let n = entries.len();
                    self.profile_entries = entries;
                    let msg = i18n::profile_analyzed(self.settings.lang, n);
                    self.toast(msg);
                }
                Ev::ProfileError(e) => {
                    self.profile_analyzing = false;
                    let msg = i18n::profile_error(self.settings.lang, &e);
                    self.toast(msg);
                }
            }
        }
        if !clip_batch.is_empty() {
            let before = self.rows.len();
            for l in &clip_batch {
                self.add_url(l, "", "", "", "");
            }
            let added = self.rows.len() - before;
            if added > 0 {
                let msg = i18n::clip_captured(self.settings.lang, added);
                self.toast(msg);
                if self.settings.auto_start_clipboard {
                    self.start_all();
                }
            }
        }
    }
}

// ============================= Descarga =============================

#[derive(Clone)]
struct DlSpec {
    id: u64,
    url: String,
    page_url: String,
    path: PathBuf,
    engine: Engine,
    extra_args: Vec<String>,
    ffmpeg: Option<String>,
    cancel: Arc<AtomicBool>,
}

async fn download_task(
    client: reqwest::Client,
    spec: DlSpec,
    sem: Arc<Semaphore>,
    tx: UnboundedSender<Ev>,
    ytdlp: Option<String>,
    galdl: Option<String>,
) {
    let Ok(_permit) = sem.acquire_owned().await else { return };
    if spec.cancel.load(Ordering::Relaxed) {
        let _ = tx.send(Ev::Status(spec.id, Status::Paused));
        return;
    }

    match spec.engine {
        Engine::YtDlp => {
            run_ytdlp(&spec, &spec.url.clone(), &tx, ytdlp.as_deref()).await;
            return;
        }
        Engine::GalleryDl => {
            run_gallerydl(&spec, &spec.url.clone(), &tx, galdl.as_deref()).await;
            return;
        }
        Engine::Http => {}
    }

    let _ = tx.send(Ev::Status(spec.id, Status::Downloading));
    let mut last_err = String::new();
    let mut expired = false;

    // Variantes de calidad: se intenta primero el original sin procesar
    // (sin marca de agua, resolución completa) y se va bajando.
    let variants = quality_variants(&spec.url);

    'outer: for (vi, variant) in variants.iter().enumerate() {
        let is_last = vi + 1 == variants.len();
        let vspec = DlSpec { url: variant.clone(), ..spec.clone() };

        for attempt in 1..=MAX_RETRIES {
            match try_http(&client, &vspec, &tx).await {
                Ok(HttpOutcome::Done) => {
                    let _ = tx.send(Ev::Status(spec.id, Status::Done));
                    return;
                }
                Ok(HttpOutcome::Cancelled) => {
                    let _ = tx.send(Ev::Status(spec.id, Status::Paused));
                    return;
                }
                Ok(HttpOutcome::Expired(code)) => {
                    last_err = format!("HTTP {code}");
                    // Variante no disponible: probar la siguiente sin reintentos
                    if !is_last {
                        continue 'outer;
                    }
                    expired = true;
                    break 'outer;
                }
                Err(e) => {
                    last_err = e.to_string();
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(Duration::from_millis(1200 * attempt as u64)).await;
                    }
                }
            }
        }
        if !is_last {
            continue;
        }
    }

    // Respaldo por URL de página, SOLO si algún motor la soporta.
    // Antes se mandaba siempre a yt-dlp, que con douyin.com/note/… responde
    // «Unsupported URL» y ocultaba el error real del enlace directo.
    if !spec.page_url.is_empty() && (expired || !last_err.is_empty()) {
        match fallback_engine(&spec.page_url) {
            Some(Engine::YtDlp) => {
                run_ytdlp(&spec, &spec.page_url.clone(), &tx, ytdlp.as_deref()).await;
                return;
            }
            Some(Engine::GalleryDl) if galdl.is_some() => {
                run_gallerydl(&spec, &spec.page_url.clone(), &tx, galdl.as_deref()).await;
                return;
            }
            _ => {} // ningún motor la soporta: se informa del error original
        }
    }

    let short: String = last_err.chars().take(60).collect();
    let _ = tx.send(Ev::ErrorDetail(spec.id, last_err));
    let _ = tx.send(Ev::Status(spec.id, Status::Error(short)));
}

enum HttpOutcome {
    Done,
    Cancelled,
    Expired(u16),
}

async fn try_http(
    client: &reqwest::Client,
    spec: &DlSpec,
    tx: &UnboundedSender<Ev>,
) -> anyhow::Result<HttpOutcome> {
    let part = PathBuf::from(format!("{}.part", spec.path.display()));
    let offset = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let mut req = client.get(&spec.url);
    // Referer correspondiente al CDN de destino (anti-hotlink)
    let referer = referer_for(&spec.url);
    if !referer.is_empty() {
        req = req.header(reqwest::header::REFERER, referer);
    }
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let mut resp = req.send().await?;
    let code = resp.status().as_u16();

    if code == 403 || code == 404 || code == 410 {
        return Ok(HttpOutcome::Expired(code));
    }
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {code}");
    }

    let resuming = code == 206 && offset > 0;
    let base = if resuming { offset } else { 0 };
    if let Some(len) = resp.content_length() {
        let _ = tx.send(Ev::Size(spec.id, base + len));
    }

    let mut file = if resuming {
        tokio::fs::OpenOptions::new().append(true).open(&part).await?
    } else {
        tokio::fs::File::create(&part).await?
    };

    let mut downloaded = base;
    let t0 = Instant::now();
    let mut session_bytes: u64 = 0;
    let mut last_emit = Instant::now();

    while let Some(chunk) = resp.chunk().await? {
        if spec.cancel.load(Ordering::Relaxed) {
            file.flush().await.ok();
            return Ok(HttpOutcome::Cancelled); // se conserva el .part para reanudar
        }
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        session_bytes += chunk.len() as u64;

        if last_emit.elapsed() >= Duration::from_millis(150) {
            let speed = session_bytes as f64 / t0.elapsed().as_secs_f64().max(0.001);
            let _ = tx.send(Ev::Progress(spec.id, downloaded, speed));
            last_emit = Instant::now();
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&part, &spec.path).await?;
    let _ = tx.send(Ev::Progress(spec.id, downloaded, 0.0));
    Ok(HttpOutcome::Done)
}

/// Lanza yt-dlp leyendo su salida en streaming para reportar progreso real.
/// Devuelve (éxito, stderr completo).
async fn ytdlp_exec(
    program: &str,
    args: &[String],
    id: u64,
    tx: &UnboundedSender<Ev>,
) -> std::io::Result<(bool, String)> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Lector de progreso: líneas "TDPROG|descargado|total|velocidad"
    let tx_prog = tx.clone();
    let progress_task = tokio::spawn(async move {
        let Some(out) = stdout else { return };
        let mut lines = BufReader::new(out).lines();
        let mut last_total: u64 = 0;
        while let Ok(Some(line)) = lines.next_line().await {
            let Some(rest) = line.trim().strip_prefix("TDPROG|") else { continue };
            let f: Vec<&str> = rest.split('|').collect();
            if f.len() < 3 {
                continue;
            }
            let num = |s: &str| s.trim().parse::<f64>().unwrap_or(0.0);
            let done = num(f[0]) as u64;
            let total = num(f[1]) as u64;
            let speed = num(f[2]);
            if total > 0 && total != last_total {
                last_total = total;
                let _ = tx_prog.send(Ev::Size(id, total));
            }
            let _ = tx_prog.send(Ev::Progress(id, done, speed));
        }
    });

    // Recoger stderr completo (mensajes de error)
    let mut err_buf = String::new();
    if let Some(e) = stderr {
        let mut lines = BufReader::new(e).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            err_buf.push_str(&l);
            err_buf.push('\n');
        }
    }

    let status = child.wait().await?;
    let _ = progress_task.await;
    Ok((status.success(), err_buf))
}

async fn run_ytdlp(spec: &DlSpec, url: &str, tx: &UnboundedSender<Ev>, program: Option<&str>) {
    let Some(program) = program else {
        let _ = tx.send(Ev::Status(
            spec.id,
            Status::Error("yt-dlp no instalado (ver Ajustes)".into()),
        ));
        return;
    };
    let _ = tx.send(Ev::Status(spec.id, Status::Resolving));

    let dir = spec.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let stem = spec
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into());
    let tmpl = dir.join(format!("{stem}.%(ext)s"));

    // Argumentos base, con plantilla de progreso legible por la app
    // Calidad adaptativa: con ffmpeg se puede fusionar vídeo y audio por
    // separado (necesario para 1080p+ en YouTube y similares). Sin él, hay que
    // limitarse a un archivo ya fusionado o yt-dlp aborta con OSError [Errno 2].
    let fmt = if spec.ffmpeg.is_some() { "bv*+ba/b" } else { "b/bv*+ba" };

    let mut base: Vec<String> = vec![
        "-f".into(), fmt.into(),
        "--no-playlist".into(),
        "-o".into(), tmpl.to_string_lossy().into_owned(),
        // Progreso en streaming, una línea por actualización
        "--newline".into(),
        "--progress".into(),
        "--progress-template".into(),
        "TDPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes,progress.total_bytes_estimate)s|%(progress.speed)s".into(),
        // Amabilidad con el servidor
        "--sleep-requests".into(), "1.5".into(),
        "--retries".into(), "5".into(),
        "--fragment-retries".into(), "5".into(),
        "--retry-sleep".into(), "exp=2:60".into(),
        "--socket-timeout".into(), "30".into(),
        "--no-warnings".into(),
    ];

    // Indicar a yt-dlp dónde está nuestro ffmpeg integrado
    if let Some(ff) = &spec.ffmpeg {
        if let Some(dir) = std::path::Path::new(ff).parent() {
            base.push("--ffmpeg-location".into());
            base.push(dir.to_string_lossy().into_owned());
        }
    }

    // Throttle global: evita que varios procesos golpeen la API a la vez
    throttle().await;

    let mut args = base.clone();
    args.extend(spec.extra_args.iter().cloned());
    args.push("--".into()); // fin de opciones: la URL nunca se interpreta como flag
    args.push(url.to_string());

    let first = ytdlp_exec(program, &args, spec.id, tx).await;

    let (ok, err) = match first {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("yt-dlp: {e}"))));
            return;
        }
    };

    if ok {
        let _ = tx.send(Ev::Status(spec.id, Status::Done));
        return;
    }

    // Cookies ilegibles (App-Bound Encryption, DB bloqueada…): reintentar sin ellas
    if is_cookie_error(&err) && !spec.extra_args.is_empty() {
        let _ = tx.send(Ev::CookieFallback);
        let _ = tx.send(Ev::DisableCookies);

        let mut retry_args = base;
        retry_args.push("--".into());
        retry_args.push(url.to_string());

        match ytdlp_exec(program, &retry_args, spec.id, tx).await {
            Ok((true, _)) => {
                let _ = tx.send(Ev::Status(spec.id, Status::Done));
            }
            Ok((false, e2)) => {
                let last = e2.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error yt-dlp");
                let _ = tx.send(Ev::ErrorDetail(spec.id, e2.clone()));
                let _ = tx.send(Ev::Status(spec.id, Status::Error(last.chars().take(60).collect())));
            }
            Err(e) => {
                let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("yt-dlp: {e}"))));
            }
        }
        return;
    }

    let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error yt-dlp");
    let short: String = last.chars().take(60).collect();
    let _ = tx.send(Ev::ErrorDetail(spec.id, err));
    let _ = tx.send(Ev::Status(spec.id, Status::Error(short)));
}

// ============================= Hilos auxiliares =============================

fn spawn_clipboard_watcher(tx: UnboundedSender<Ev>, enabled: Arc<AtomicBool>, grab_any: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(mut cb) = arboard::Clipboard::new() else { return };
        let re = Regex::new(URL_RE).unwrap();
        let mut last = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(900));
            if !enabled.load(Ordering::Relaxed) {
                continue;
            }
            if let Ok(text) = cb.get_text() {
                if text != last {
                    last = text.clone();
                    let any = grab_any.load(Ordering::Relaxed);
                    let links: Vec<String> = re
                        .find_iter(&text)
                        .map(|m| m.as_str().to_string())
                        .filter(|u| any || KNOWN_SITES.iter().any(|s| u.contains(s)))
                        .collect();
                    if !links.is_empty() && tx.send(Ev::Clipboard(links)).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

/// Lanza gallery-dl contando en vivo los archivos descargados (una línea = un archivo).
/// Devuelve (éxito, stderr completo).
async fn galdl_exec(
    program: &str,
    args: &[String],
    id: u64,
    tx: &UnboundedSender<Ev>,
) -> std::io::Result<(bool, String)> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let tx_files = tx.clone();
    let counter = tokio::spawn(async move {
        let Some(out) = stdout else { return };
        let mut lines = BufReader::new(out).lines();
        let mut n: u64 = 0;
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            n += 1;
            let _ = tx_files.send(Ev::GalFiles(id, n));
        }
    });

    let mut err_buf = String::new();
    if let Some(e) = stderr {
        let mut lines = BufReader::new(e).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            err_buf.push_str(&l);
            err_buf.push('\n');
        }
    }

    let status = child.wait().await?;
    let _ = counter.await;
    Ok((status.success(), err_buf))
}

/// Descarga un post de imágenes (TikTok /photo/, Douyin /note/) con gallery-dl
async fn run_gallerydl(spec: &DlSpec, url: &str, tx: &UnboundedSender<Ev>, program: Option<&str>) {
    let Some(program) = program else {
        let _ = tx.send(Ev::Status(
            spec.id,
            Status::Error("gallery-dl no instalado (ver Ajustes)".into()),
        ));
        return;
    };
    let _ = tx.send(Ev::Status(spec.id, Status::Resolving));

    throttle().await;

    let dir = spec.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();

    // Primer intento con streaming: gallery-dl imprime una ruta por archivo
    // descargado, así que contamos líneas para dar feedback en vivo.
    let mut args: Vec<String> = vec![
        "-D".into(), dir.to_string_lossy().into_owned(),
        "--sleep-request".into(), "1.5".into(),
    ];
    args.extend(spec.extra_args.iter().cloned());
    args.push("--".into());
    args.push(url.to_string());

    if let Ok((ok, err)) = galdl_exec(program, &args, spec.id, tx).await {
        if ok {
            let _ = tx.send(Ev::Status(spec.id, Status::Done));
            return;
        }
        // Cookies ilegibles: reintentar sin ellas (sirve para perfiles públicos)
        if is_cookie_error(&err) && !spec.extra_args.is_empty() {
            let _ = tx.send(Ev::CookieFallback);
            let _ = tx.send(Ev::DisableCookies);
            let retry: Vec<String> = vec![
                "-D".into(), dir.to_string_lossy().into_owned(),
                "--sleep-request".into(), "1.5".into(),
                "--".into(), url.to_string(),
            ];
            match galdl_exec(program, &retry, spec.id, tx).await {
                Ok((true, _)) => {
                    let _ = tx.send(Ev::Status(spec.id, Status::Done));
                }
                Ok((false, e2)) => {
                    let last = e2.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error gallery-dl");
                    let _ = tx.send(Ev::ErrorDetail(spec.id, e2.clone()));
                    let _ = tx.send(Ev::Status(spec.id, Status::Error(last.chars().take(60).collect())));
                }
                Err(e) => {
                    let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("gallery-dl: {e}"))));
                }
            }
            return;
        }
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error gallery-dl");
        let short: String = last.chars().take(60).collect();
        let _ = tx.send(Ev::ErrorDetail(spec.id, err));
        let _ = tx.send(Ev::Status(spec.id, Status::Error(short)));
        return;
    }

    let mut cmd = tokio::process::Command::new(program);
    cmd.args(["-D", &dir.to_string_lossy(), "--", url]);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }

    match cmd.output().await {
        Ok(out) if out.status.success() => {
            let _ = tx.send(Ev::Status(spec.id, Status::Done));
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).to_string();

            // Cookies ilegibles: reintentar sin ellas (sirve para perfiles públicos)
            if is_cookie_error(&err) && !spec.extra_args.is_empty() {
                let _ = tx.send(Ev::CookieFallback);
                let _ = tx.send(Ev::DisableCookies);
                let mut retry = tokio::process::Command::new(program);
                retry.args(["-D", &dir.to_string_lossy(), "--sleep-request", "1.5", "--", url]);
                #[cfg(windows)]
                {
                    retry.creation_flags(0x0800_0000);
                }
                match retry.output().await {
                    Ok(o) if o.status.success() => {
                        let _ = tx.send(Ev::Status(spec.id, Status::Done));
                    }
                    Ok(o) => {
                        let e2 = String::from_utf8_lossy(&o.stderr).to_string();
                        let last = e2.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error gallery-dl");
                        let _ = tx.send(Ev::ErrorDetail(spec.id, e2.clone()));
                        let _ = tx.send(Ev::Status(spec.id, Status::Error(last.chars().take(60).collect())));
                    }
                    Err(e) => {
                        let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("gallery-dl: {e}"))));
                    }
                }
                return;
            }

            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error gallery-dl");
            let short: String = last.chars().take(60).collect();
            let _ = tx.send(Ev::ErrorDetail(spec.id, err));
            let _ = tx.send(Ev::Status(spec.id, Status::Error(short)));
        }
        Err(e) => {
            let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("gallery-dl: {e}"))));
        }
    }
}

/// Espaciado mínimo entre lanzamientos de yt-dlp/gallery-dl (anti rate-limit).
/// Serializa el arranque de procesos sin serializar las descargas en sí.
async fn throttle() {
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    static GATE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    const MIN_GAP: Duration = Duration::from_millis(1800);

    let gate = GATE.get_or_init(|| Mutex::new(None));
    let mut last = gate.lock().await;
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < MIN_GAP {
            tokio::time::sleep(MIN_GAP - elapsed).await;
        }
    }
    *last = Some(Instant::now());
}

/// Ejecuta yt-dlp en modo enumeración de playlist
async fn run_analyze(
    program: &str,
    url: &str,
    extra_args: &[String],
) -> std::io::Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(["--flat-playlist", "-J", "--no-warnings", "--playlist-end", "2000"]);
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.arg("--"); // fin de opciones: la URL nunca puede interpretarse como flag
    cmd.arg(url);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output().await
}

/// Analiza un perfil (TikTok/Douyin) con yt-dlp --flat-playlist y devuelve sus posts.
/// Si las cookies del navegador fallan (DB bloqueada en Windows), reintenta sin ellas.
async fn analyze_profile(program: String, url: String, extra_args: Vec<String>, tx: UnboundedSender<Ev>) {
    let out = run_analyze(&program, &url, &extra_args).await;

    let out = match out {
        Err(e) => {
            let _ = tx.send(Ev::ProfileError(e.to_string()));
            return;
        }
        Ok(o) if !o.status.success() && !extra_args.is_empty() => {
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            if is_cookie_error(&err) {
                // Reintento sin cookies: la mayoría de perfiles son públicos
                let _ = tx.send(Ev::CookieFallback);
                match run_analyze(&program, &url, &[]).await {
                    Ok(o2) => o2,
                    Err(e) => {
                        let _ = tx.send(Ev::ProfileError(e.to_string()));
                        return;
                    }
                }
            } else {
                o
            }
        }
        Ok(o) => o,
    };

    match Ok::<_, std::io::Error>(out) {
        Ok(out) if out.status.success() => {
            match serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                Ok(v) => {
                    let mut entries = Vec::new();
                    let push = |entries: &mut Vec<ProfileEntry>, e: &serde_json::Value| {
                        let g = |k: &str| e.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let mut u = g("url");
                        if u.is_empty() {
                            u = g("webpage_url");
                        }
                        if u.is_empty() {
                            return;
                        }
                        let title = {
                            let t = g("title");
                            if t.is_empty() { g("id") } else { t }
                        };
                        let is_image = u.contains("/photo/") || u.contains("/note/");
                        entries.push(ProfileEntry {
                            selected: true,
                            id: g("id"),
                            title,
                            url: u,
                            is_image,
                        });
                    };
                    if let Some(arr) = v.get("entries").and_then(|e| e.as_array()) {
                        for e in arr {
                            push(&mut entries, e);
                        }
                    } else {
                        push(&mut entries, &v); // era un post individual
                    }
                    let _ = tx.send(Ev::ProfileEntries(entries));
                }
                Err(e) => {
                    let _ = tx.send(Ev::ProfileError(format!("JSON inválido: {e}")));
                }
            }
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error yt-dlp");
            let _ = tx.send(Ev::ProfileError(last.chars().take(120).collect()));
        }
        Err(e) => {
            let _ = tx.send(Ev::ProfileError(e.to_string()));
        }
    }
}

/// Carpeta donde Todo Downloader guarda su copia integrada de yt-dlp
fn ytdlp_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".local/share"))
            .unwrap_or_else(|| PathBuf::from("."))
    };
    base.join("TodoDownloader").join("bin")
}

fn ytdlp_bundled_path() -> PathBuf {
    ytdlp_dir().join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" })
}

fn ytdlp_release_url() -> &'static str {
    if cfg!(windows) {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
    } else {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp"
    }
}

fn galdl_bundled_path() -> PathBuf {
    ytdlp_dir().join(if cfg!(windows) { "gallery-dl.exe" } else { "gallery-dl.bin" })
}

/// URLs candidatas del binario de gallery-dl, en orden de preferencia.
/// Los builds oficiales viven en gdl-org/builds; mikf/gallery-dl ya no adjunta
/// ejecutables a sus releases (por eso se mantiene solo como respaldo histórico).
fn galdl_release_urls() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[
            "https://github.com/gdl-org/builds/releases/latest/download/gallery-dl_windows.exe",
            "https://github.com/mikf/gallery-dl/releases/latest/download/gallery-dl.exe",
        ]
    }
    #[cfg(target_os = "macos")]
    {
        &["https://github.com/gdl-org/builds/releases/latest/download/gallery-dl_macos"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            "https://github.com/gdl-org/builds/releases/latest/download/gallery-dl_linux",
            "https://github.com/mikf/gallery-dl/releases/latest/download/gallery-dl.bin",
        ]
    }
}

/// Detecta gallery-dl: primero la copia integrada, luego el PATH del sistema
fn spawn_galdl_check(tx: UnboundedSender<Ev>) {
    std::thread::spawn(move || {
        let bundled = galdl_bundled_path();
        if bundled.exists() {
            let _ = tx.send(Ev::GalDl(Some(bundled.to_string_lossy().into_owned())));
            return;
        }
        let mut cmd = std::process::Command::new("gallery-dl");
        cmd.arg("--version");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);
        let _ = tx.send(Ev::GalDl(if ok { Some("gallery-dl".into()) } else { None }));
    });
}

/// Verificación post-descarga: el binario debe responder a --version.
/// Si no, se elimina (protección frente a descargas corruptas o manipuladas).
async fn verify_tool(path: &PathBuf) -> bool {
    let mut cmd = tokio::process::Command::new(path);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    matches!(cmd.output().await, Ok(o) if o.status.success())
}

/// Descarga genérica de un binario de GitHub Releases con progreso
async fn download_binary(
    client: &reqwest::Client,
    url: &str,
    dest: &PathBuf,
    mut on_progress: impl FnMut(f32),
) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status().as_u16());
    }
    let total = resp.content_length().unwrap_or(0);
    let tmp = PathBuf::from(format!("{}.part", dest.display()));
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut done: u64 = 0;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if total > 0 {
            on_progress(done as f32 / total as f32);
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, dest).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(dest)?.permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(dest, perm)?;
    }
    Ok(())
}

// ---------------- ffmpeg integrado ----------------

fn ffmpeg_bundled_path() -> PathBuf {
    ytdlp_dir().join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })
}

/// Builds oficiales que mantiene el propio equipo de yt-dlp: garantiza
/// compatibilidad y evita depender de terceros.
#[cfg_attr(not(windows), allow(dead_code))]
fn ffmpeg_release_url() -> &'static str {
    if cfg!(windows) {
        "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip"
    } else {
        "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-linux64-gpl.tar.xz"
    }
}

fn spawn_ffmpeg_check(tx: UnboundedSender<Ev>) {
    std::thread::spawn(move || {
        let bundled = ffmpeg_bundled_path();
        if bundled.exists() {
            let _ = tx.send(Ev::Ffmpeg(Some(bundled.to_string_lossy().into_owned())));
            return;
        }
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-version");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);
        let _ = tx.send(Ev::Ffmpeg(if ok { Some("ffmpeg".into()) } else { None }));
    });
}

/// Descarga el zip oficial y extrae SOLO ffmpeg y ffprobe (el resto del
/// paquete son cabeceras y librerías que no necesitamos).
#[cfg(windows)]
async fn install_ffmpeg(client: reqwest::Client, tx: UnboundedSender<Ev>) {
    let dir = ytdlp_dir();
    let zip_path = dir.join("ffmpeg_tmp.zip");

    let tx2 = tx.clone();
    if let Err(e) = download_binary(&client, ffmpeg_release_url(), &zip_path, move |p| {
        // El zip es grande: el 90% del progreso es la descarga
        let _ = tx2.send(Ev::FfmpegProgress(p * 0.9));
    })
    .await
    {
        let _ = tx.send(Ev::FfmpegError(e.to_string()));
        return;
    }

    let _ = tx.send(Ev::FfmpegProgress(0.93));
    let dir2 = dir.clone();
    let zip2 = zip_path.clone();
    let extracted = tokio::task::spawn_blocking(move || extract_ffmpeg(&zip2, &dir2)).await;
    let _ = tokio::fs::remove_file(&zip_path).await;

    match extracted {
        Ok(Ok(())) => {
            let path = ffmpeg_bundled_path();
            if verify_tool_arg(&path, "-version").await {
                let _ = tx.send(Ev::Ffmpeg(Some(path.to_string_lossy().into_owned())));
            } else {
                let _ = tokio::fs::remove_file(&path).await;
                let _ = tx.send(Ev::FfmpegError(
                    "el binario extraído no superó la verificación".into(),
                ));
            }
        }
        Ok(Err(e)) => {
            let _ = tx.send(Ev::FfmpegError(e.to_string()));
        }
        Err(e) => {
            let _ = tx.send(Ev::FfmpegError(e.to_string()));
        }
    }
}

/// Extrae ffmpeg.exe y ffprobe.exe del zip, ignorando la estructura de carpetas
#[cfg(windows)]
fn extract_ffmpeg(zip_path: &PathBuf, dest_dir: &PathBuf) -> anyhow::Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut found = 0;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_ascii_lowercase();
        let wanted = name.ends_with("/bin/ffmpeg.exe") || name.ends_with("/bin/ffprobe.exe");
        if !wanted {
            continue;
        }
        let out_name = if name.ends_with("ffmpeg.exe") { "ffmpeg.exe" } else { "ffprobe.exe" };
        let out_path = dest_dir.join(out_name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        found += 1;
    }

    if found == 0 {
        anyhow::bail!("no se encontró ffmpeg.exe dentro del paquete");
    }
    Ok(())
}

#[cfg(not(windows))]
async fn install_ffmpeg(_client: reqwest::Client, tx: UnboundedSender<Ev>) {
    let _ = tx.send(Ev::FfmpegError(
        "instalación automática disponible solo en Windows: usa el gestor de paquetes del sistema".into(),
    ));
}

/// Verificación post-instalación con un argumento concreto
#[cfg_attr(not(windows), allow(dead_code))]
async fn verify_tool_arg(path: &PathBuf, arg: &str) -> bool {
    let mut cmd = tokio::process::Command::new(path);
    cmd.arg(arg);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    matches!(cmd.output().await, Ok(o) if o.status.success())
}

/// Instala gallery-dl (motor de imágenes) desde GitHub Releases.
/// Prueba cada URL candidata hasta que una funcione.
async fn install_gallerydl(client: reqwest::Client, tx: UnboundedSender<Ev>) {
    let dest = galdl_bundled_path();
    let mut result = Err(anyhow::anyhow!("sin URLs candidatas"));
    for url in galdl_release_urls() {
        let tx2 = tx.clone();
        result = download_binary(&client, url, &dest, move |p| {
            let _ = tx2.send(Ev::GalDlProgress(p));
        })
        .await;
        if result.is_ok() {
            break;
        }
    }
    match result
    {
        Ok(()) => {
            if verify_tool(&dest).await {
                let _ = tx.send(Ev::GalDl(Some(dest.to_string_lossy().into_owned())));
            } else {
                let _ = tokio::fs::remove_file(&dest).await;
                let _ = tx.send(Ev::GalDlError(
                    "el binario descargado no superó la verificación y fue eliminado".into(),
                ));
            }
        }
        Err(e) => {
            let _ = tx.send(Ev::GalDlError(e.to_string()));
        }
    }
}

/// Detecta yt-dlp: primero la copia integrada, luego el PATH del sistema
fn spawn_ytdlp_check(tx: UnboundedSender<Ev>) {
    std::thread::spawn(move || {
        let bundled = ytdlp_bundled_path();
        if bundled.exists() {
            let _ = tx.send(Ev::YtDlp(Some(bundled.to_string_lossy().into_owned())));
            return;
        }
        let mut cmd = std::process::Command::new("yt-dlp");
        cmd.arg("--version");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);
        let _ = tx.send(Ev::YtDlp(if ok { Some("yt-dlp".into()) } else { None }));
    });
}

/// Descarga el binario oficial de yt-dlp desde GitHub Releases (integración nativa)
async fn install_ytdlp(client: reqwest::Client, tx: UnboundedSender<Ev>) {
    let dest = ytdlp_bundled_path();
    let result: anyhow::Result<()> = async {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut resp = client.get(ytdlp_release_url()).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {}", resp.status().as_u16());
        }
        let total = resp.content_length().unwrap_or(0);
        let tmp = PathBuf::from(format!("{}.part", dest.display()));
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut done: u64 = 0;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
            done += chunk.len() as u64;
            if total > 0 {
                let _ = tx.send(Ev::YtDlpProgress(done as f32 / total as f32));
            }
        }
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&tmp, &dest).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&dest)?.permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&dest, perm)?;
        }
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            if verify_tool(&dest).await {
                let _ = tx.send(Ev::YtDlp(Some(dest.to_string_lossy().into_owned())));
            } else {
                let _ = tokio::fs::remove_file(&dest).await;
                let _ = tx.send(Ev::YtDlpError(
                    "el binario descargado no superó la verificación y fue eliminado".into(),
                ));
            }
        }
        Err(e) => {
            let _ = tx.send(Ev::YtDlpError(e.to_string()));
        }
    }
}

// ============================= Tema =============================

/// Carga una fuente del sistema con glifos CJK (chino/japonés/coreano).
/// Sin esto, los títulos de Douyin/TikTok en chino se ven como cuadrados «□».
fn load_cjk_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        // Windows — .ttf primero por ser el formato más simple de parsear
        "C:/Windows/Fonts/simhei.ttf",
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/simsun.ttc",
        "C:/Windows/Fonts/msgothic.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ];

    let Some(data) = CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) else {
        return; // sin fuente CJK: se mantiene la de por defecto
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), egui::FontData::from_owned(data));
    // Como respaldo (al final): solo se usa para los glifos que falten
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Tipografía
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(24.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.slider_width = 180.0;
    style.animation_time = 0.18; // transiciones suaves en hover

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0f32,CARD_HOVER);
    v.extreme_bg_color = CARD; // fondo de TextEdit / ProgressBar
    v.faint_bg_color = Color32::from_rgb(25, 28, 36); // striped
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0f32,ACCENT);
    v.slider_trailing_fill = true;
    v.hyperlink_color = CYAN;

    let r = Rounding::same(8.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0f32,TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32,Color32::from_rgb(40, 45, 58));

    v.widgets.inactive.rounding = r;
    v.widgets.inactive.weak_bg_fill = CARD; // relleno de botones
    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0f32,TEXT);
    v.widgets.inactive.bg_stroke = Stroke::NONE;

    v.widgets.hovered.rounding = r;
    v.widgets.hovered.weak_bg_fill = CARD_HOVER;
    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.fg_stroke = Stroke::new(1.2f32,Color32::WHITE);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0f32,ACCENT.gamma_multiply(0.6));
    v.widgets.hovered.expansion = 1.6; // el botón "crece" al pasar el ratón

    v.widgets.active.rounding = r;
    v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.5);
    v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.5);
    v.widgets.active.fg_stroke = Stroke::new(1.2f32,Color32::WHITE);

    v.widgets.open.rounding = r;
    v.widgets.open.weak_bg_fill = CARD_HOVER;
    v.widgets.open.bg_fill = CARD_HOVER;

    style.visuals = v;
    ctx.set_style(style);
}

// ============================= Componentes UI =============================

fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::symmetric(16.0, 12.0))
}

fn stat_card(ui: &mut egui::Ui, value: String, label: &str, color: Color32) {
    card_frame().show(ui, |ui| {
        ui.set_min_width(120.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(value).size(22.0).strong().color(color));
            ui.label(RichText::new(label).size(11.5).color(MUTED));
        });
    });
}

/// Efecto "gloss" animado: brillo superior + halo de acento al pasar el ratón
fn gloss_paint(ui: &egui::Ui, resp: &egui::Response) {
    let t = ui.ctx().animate_bool(resp.id.with("gloss"), resp.hovered());
    if t <= 0.0 {
        return;
    }
    let rect = resp.rect;
    let mut top = rect;
    top.set_height(rect.height() * 0.5);
    let top_round = Rounding {
        nw: 8.0,
        ne: 8.0,
        sw: 0.0,
        se: 0.0,
    };
    let painter = ui.painter();
    painter.rect_filled(top, top_round, Color32::from_white_alpha((26.0 * t) as u8));
    painter.rect_stroke(
        rect,
        Rounding::same(8.0),
        Stroke::new(1.0f32, ACCENT.gamma_multiply(0.55 * t)),
    );
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let resp = ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(ACCENT)
            .rounding(Rounding::same(8.0)),
    );
    gloss_paint(ui, &resp);
    resp
}

/// Botón secundario con el mismo efecto gloss en hover
fn soft_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let resp = ui.button(text);
    gloss_paint(ui, &resp);
    resp
}

fn status_pill(ui: &mut egui::Ui, status: &Status, lang: Lang, engine: Engine) {
    let color = status.color();
    egui::Frame::none()
        .fill(color.gamma_multiply(0.15))
        .rounding(Rounding::same(20.0))
        .inner_margin(Margin::symmetric(10.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(status.label(lang, engine)).size(12.0).color(color));
        });
}

fn nav_item(ui: &mut egui::Ui, selected: bool, icon: &str, label: &str, badge: usize) -> bool {
    let text = if badge > 0 {
        format!("{icon}   {label}   ·{badge}")
    } else {
        format!("{icon}   {label}")
    };
    let rt = if selected {
        RichText::new(text).color(Color32::WHITE).strong()
    } else {
        RichText::new(text).color(MUTED)
    };
    let btn = egui::Button::new(rt)
        .fill(if selected { ACCENT.gamma_multiply(0.25) } else { Color32::TRANSPARENT })
        .rounding(Rounding::same(8.0))
        .min_size(egui::vec2(176.0, 36.0));
    let resp = ui.add(btn);
    gloss_paint(ui, &resp);
    resp.clicked()
}

// ============================= UI principal =============================

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        // Drag & drop de TXT/JSON
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for p in dropped {
            self.import_path(&p);
        }

        let n_active = self.rows.iter().filter(|r| r.status.is_active()).count();
        let n_queue = self
            .rows
            .iter()
            .filter(|r| matches!(r.status, Status::Queued | Status::Paused))
            .count();
        let n_done = self.rows.iter().filter(|r| r.status == Status::Done).count();
        let n_failed = self
            .rows
            .iter()
            .filter(|r| matches!(r.status, Status::Error(_)))
            .count();
        let global_speed: f64 = self.rows.iter().map(|r| r.speed).sum();

        // ---------------- Sidebar ----------------
        egui::SidePanel::left("sidebar")
            .exact_width(208.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(16.0, 18.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::Frame::none()
                        .fill(ACCENT)
                        .rounding(Rounding::same(10.0))
                        .inner_margin(Margin::symmetric(9.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("⬇").size(18.0).color(Color32::WHITE));
                        });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("Todo").size(16.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new("Downloader").size(16.0).strong().color(CYAN));
                    });
                });
                ui.add_space(24.0);

                let lang = self.settings.lang;
                if nav_item(ui, self.view == View::Downloads, "📥", t(lang, "nav.downloads"), n_queue + n_active) {
                    self.view = View::Downloads;
                }
                if nav_item(ui, self.view == View::Profile, "🔍", t(lang, "nav.profile"), self.profile_entries.len()) {
                    self.view = View::Profile;
                }
                if nav_item(ui, self.view == View::Capture, "🧲", t(lang, "nav.capture"), 0) {
                    self.view = View::Capture;
                }
                if nav_item(ui, self.view == View::Done, "✅", t(lang, "nav.completed"), n_done) {
                    self.view = View::Done;
                }
                if nav_item(ui, self.view == View::Failed, "⚠", t(lang, "nav.errors"), n_failed) {
                    self.view = View::Failed;
                }
                ui.add_space(8.0);
                if nav_item(ui, self.view == View::Settings, "⚙", t(lang, "nav.settings"), 0) {
                    self.view = View::Settings;
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("By Eric V. Gramunt").size(11.5).color(MUTED));
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(11.0).color(MUTED));
                    ui.add_space(6.0);
                    match self.ytdlp_ok {
                        Some(true) => ui.label(RichText::new(t(lang, "side.ytdlp_active")).size(11.5).color(GREEN)),
                        Some(false) => ui
                            .label(RichText::new(t(lang, "side.ytdlp_missing")).size(11.5).color(AMBER))
                            .on_hover_text(t(lang, "side.ytdlp_tip")),
                        None => ui.label(RichText::new("● yt-dlp…").size(11.5).color(MUTED)),
                    };
                    if self.galdl_cmd.is_some() {
                        ui.label(RichText::new(t(lang, "side.galdl_active")).size(11.5).color(GREEN));
                    } else {
                        ui.label(RichText::new(t(lang, "side.galdl_missing")).size(11.5).color(AMBER))
                            .on_hover_text(t(lang, "side.galdl_tip"));
                    }
                    if self.ffmpeg_cmd.is_some() {
                        ui.label(RichText::new(t(lang, "side.ffmpeg_active")).size(11.5).color(GREEN));
                    } else {
                        ui.label(RichText::new(t(lang, "side.ffmpeg_missing")).size(11.5).color(AMBER))
                            .on_hover_text(t(lang, "side.ffmpeg_tip"));
                    }
                    if self.settings.clipboard_watch {
                        ui.label(RichText::new(t(lang, "side.grabber_active")).size(11.5).color(CYAN));
                    }
                });
            });

        // ---------------- Panel central ----------------
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(22.0, 18.0)),
            )
            .show(ctx, |ui| {
                match self.view {
                    // Vistas de formulario: con scroll vertical para que nada quede tapado
                    View::Settings => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.settings_ui(ui));
                    }
                    View::Profile => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.profile_ui(ui));
                    }
                    View::Capture => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.capture_ui(ui));
                    }
                    _ => self.queue_ui(ui, n_active, n_done, n_failed, global_speed),
                }

                // Toast
                if let Some(until) = self.toast_until {
                    if Instant::now() < until {
                        egui::Area::new(egui::Id::new("toast"))
                            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -18.0])
                            .show(ctx, |ui| {
                                egui::Frame::none()
                                    .fill(CARD_HOVER)
                                    .rounding(Rounding::same(10.0))
                                    .inner_margin(Margin::symmetric(16.0, 9.0))
                                    .stroke(Stroke::new(1.0f32,ACCENT.gamma_multiply(0.5)))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(&self.toast).color(TEXT));
                                    });
                            });
                    }
                }
            });

        // ---------------- Ventana "Añadir enlaces" ----------------
        if self.show_add {
            let mut open_flag = true;
            let mut do_add = false;
            let lang = self.settings.lang;
            egui::Window::new(t(lang, "add.title"))
                .open(&mut open_flag)
                .default_size([560.0, 300.0])
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new(t(lang, "add.hint")).color(MUTED));
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.add_text)
                                .desired_rows(10)
                                .desired_width(f32::INFINITY),
                        );
                    });
                    ui.horizontal(|ui| {
                        if primary_button(ui, t(lang, "add.confirm")).clicked() {
                            do_add = true;
                        }
                        if ui.button(t(lang, "btn.cancel")).clicked() {
                            self.add_text.clear();
                            self.show_add = false;
                        }
                    });
                });
            if do_add {
                let text = std::mem::take(&mut self.add_text);
                let n = self.add_plain_urls(&text);
                self.toast(i18n::added_links(lang, n));
                self.show_add = false;
            } else if !open_flag {
                self.show_add = false;
            }
        }

        if self.rows.iter().any(|r| r.status.is_active()) {
            ctx.request_repaint_after(Duration::from_millis(150));
        } else {
            ctx.request_repaint_after(Duration::from_millis(600));
        }
    }
}

impl App {
    // ---------------- Vista de cola ----------------

    fn queue_ui(&mut self, ui: &mut egui::Ui, n_active: usize, n_done: usize, n_failed: usize, global_speed: f64) {
        // Tarjetas de estadísticas
        let lg = self.settings.lang;
        ui.horizontal(|ui| {
            stat_card(ui, format!("{}", self.rows.len()), t(lg, "stat.total"), Color32::WHITE);
            stat_card(ui, format!("{n_active}"), t(lg, "stat.active"), CYAN);
            stat_card(ui, format!("{n_done}"), t(lg, "stat.completed"), GREEN);
            stat_card(ui, format!("{n_failed}"), t(lg, "stat.errors"), if n_failed > 0 { RED } else { MUTED });
            stat_card(
                ui,
                if global_speed > 0.0 { format!("{}/s", fmt_size(global_speed)) } else { "—".into() },
                t(lg, "stat.speed"),
                ACCENT,
            );
        });
        ui.add_space(14.0);

        // Barra de acciones
        let lang = self.settings.lang;
        ui.horizontal(|ui| {
            match self.view {
                View::Downloads => {
                    if primary_button(ui, t(lang, "btn.start_all")).clicked() {
                        self.start_all();
                    }
                    if soft_button(ui, t(lang, "btn.pause_all")).clicked() {
                        self.pause_all();
                    }
                    if soft_button(ui, t(lang, "btn.add_links")).clicked() {
                        self.show_add = true;
                    }
                    if soft_button(ui, t(lang, "btn.clear_all")).clicked() {
                        let n = self.rows.len();
                        self.rows.retain(|r| r.status.is_active());
                        let removed = n - self.rows.len();
                        if removed > 0 {
                            self.toast(i18n::cleared(lang, removed));
                        }
                    }
                    if soft_button(ui, t(lang, "btn.import")).clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Exportación TikTok", &["txt", "json"])
                            .pick_file()
                        {
                            self.import_path(&p);
                        }
                    }
                }
                View::Done => {
                    if soft_button(ui, t(lang, "btn.clean_completed")).clicked() {
                        self.clear_done();
                    }
                    if soft_button(ui, t(lang, "btn.open_dest")).clicked() {
                        let _ = open::that(&self.settings.dest);
                    }
                }
                View::Failed => {
                    if primary_button(ui, t(lang, "btn.retry_all")).clicked() {
                        self.retry_failed();
                    }
                }
                View::Settings | View::Profile | View::Capture => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [200.0, 30.0],
                    egui::TextEdit::singleline(&mut self.search).hint_text(t(lang, "btn.search_hint")),
                );
            });
        });
        ui.add_space(12.0);

        // Filtrado
        let search = self.search.to_lowercase();
        let view = self.view;
        let visible: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| match view {
                View::Downloads => r.status != Status::Done && !matches!(r.status, Status::Error(_)),
                View::Done => r.status == Status::Done,
                View::Failed => matches!(r.status, Status::Error(_)),
                View::Settings | View::Profile | View::Capture => false,
            })
            .filter(|(_, r)| {
                search.is_empty()
                    || r.filename.to_lowercase().contains(&search)
                    || r.author.to_lowercase().contains(&search)
            })
            .map(|(i, _)| i)
            .collect();

        if visible.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                let (icon, msg) = match view {
                    View::Done => ("✅", t(lang, "empty.done")),
                    View::Failed => ("🎉", t(lang, "empty.failed")),
                    _ => ("⬇", t(lang, "empty.queue")),
                };
                ui.label(RichText::new(icon).size(42.0));
                ui.add_space(6.0);
                ui.label(RichText::new(msg).size(15.0).color(MUTED));
            });
            return;
        }

        // Tabla
        let mut actions: Vec<(usize, RowAction)> = Vec::new();
        // Margen derecho reducido: deja hueco a la barra de scroll para que no
        // se coma la última columna (los botones de acción) al estrechar la ventana.
        egui::Frame::none()
            .fill(CARD)
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin {
                left: 16.0,
                right: 6.0,
                top: 12.0,
                bottom: 12.0,
            })
            .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                // Reservar siempre el ancho de la barra: evita saltos de layout
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    // clip(true): los nombres largos se recortan en vez de empujar
                    // al resto de columnas fuera de la vista
                    .column(Column::remainder().at_least(160.0).clip(true))
                    .column(Column::exact(80.0))
                    .column(Column::exact(150.0))
                    .column(Column::exact(90.0))
                    .column(Column::exact(130.0))
                    .column(Column::exact(78.0))
                    .header(26.0, |mut h| {
                        for key in ["col.file", "col.size", "col.progress", "col.speed", "col.status", ""] {
                            h.col(|ui| {
                                let txt = if key.is_empty() { "" } else { t(lang, key) };
                                ui.label(RichText::new(txt).size(11.0).color(MUTED).strong());
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(34.0, visible.len(), |mut row| {
                            let i = visible[row.index()];
                            let r = &self.rows[i];
                            row.col(|ui| {
                                ui.label(RichText::new(&r.filename).color(TEXT))
                                    .on_hover_text(&r.url);
                            });
                            row.col(|ui| {
                                // gallery-dl no da bytes totales: mostramos archivos descargados
                                let txt = if r.engine == Engine::GalleryDl && r.gal_files > 0 {
                                    format!("{} arch.", r.gal_files)
                                } else if r.size > 0 {
                                    fmt_size(r.size as f64)
                                } else {
                                    "—".into()
                                };
                                ui.label(RichText::new(txt).color(MUTED));
                            });
                            row.col(|ui| {
                                let frac = if r.size > 0 {
                                    r.downloaded as f32 / r.size as f32
                                } else if r.status == Status::Done {
                                    1.0
                                } else {
                                    0.0
                                };
                                ui.add(
                                    egui::ProgressBar::new(frac)
                                        .fill(if r.status == Status::Done { GREEN } else { ACCENT })
                                        .show_percentage(),
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(if r.speed > 0.0 {
                                        format!("{}/s", fmt_size(r.speed))
                                    } else {
                                        "—".into()
                                    })
                                    .color(CYAN),
                                );
                            });
                            row.col(|ui| {
                                let resp = ui.scope(|ui| status_pill(ui, &r.status, lang, r.engine)).response;
                                // Detalle completo del fallo al pasar el ratón
                                let detail = if !r.error_full.is_empty() {
                                    r.error_full.clone()
                                } else {
                                    r.status.brief()
                                };
                                if !detail.is_empty() {
                                    resp.on_hover_text(detail);
                                }
                            });
                            row.col(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                match r.status {
                                    Status::Downloading | Status::Waiting | Status::Resolving => {
                                        if ui.small_button("⏸").on_hover_text(t(lang, "tip.pause")).clicked() {
                                            actions.push((i, RowAction::Pause));
                                        }
                                    }
                                    Status::Paused | Status::Queued => {
                                        if ui.small_button("▶").on_hover_text(t(lang, "tip.start")).clicked() {
                                            actions.push((i, RowAction::Resume));
                                        }
                                    }
                                    Status::Done => {
                                        if ui.small_button("📁").on_hover_text(t(lang, "tip.open_folder")).clicked() {
                                            actions.push((i, RowAction::OpenDir));
                                        }
                                    }
                                    Status::Error(_) => {
                                        if ui.small_button("🔁").on_hover_text(t(lang, "tip.retry")).clicked() {
                                            actions.push((i, RowAction::Resume));
                                        }
                                    }
                                }
                                if !r.status.is_active() {
                                    if ui.small_button("🗑").on_hover_text(t(lang, "tip.remove")).clicked() {
                                        actions.push((i, RowAction::Remove));
                                    }
                                }
                            });
                        });
                    });
            });
        });

        // Aplicar acciones fuera del préstamo de la tabla
        let mut to_remove: Vec<usize> = Vec::new();
        for (i, a) in actions {
            match a {
                RowAction::Pause => self.rows[i].cancel.store(true, Ordering::Relaxed),
                RowAction::Resume => {
                    self.rows[i].status = Status::Queued;
                    self.start_row(i);
                }
                RowAction::OpenDir => {
                    let dir = self.dest_dir(&self.rows[i].author);
                    let _ = open::that(dir);
                }
                RowAction::Remove => to_remove.push(i),
            }
        }
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for i in to_remove {
            self.rows.remove(i);
        }
    }

    // ---------------- Vista de perfil ----------------

    fn profile_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "profile.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(4.0);
        ui.label(RichText::new(t(lang, "profile.subtitle")).color(MUTED));
        ui.add_space(14.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            ui.label(RichText::new(t(lang, "profile.url_label")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [420.0, 30.0],
                    egui::TextEdit::singleline(&mut self.profile_url)
                        .hint_text("https://www.tiktok.com/@usuario  ·  https://www.douyin.com/user/…"),
                );
                if soft_button(ui, t(lang, "btn.paste")).clicked() {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        if let Ok(t) = cb.get_text() {
                            self.profile_url = t.trim().to_string();
                        }
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t(lang, "profile.want")).color(MUTED));
                ui.checkbox(&mut self.profile_want_videos, t(lang, "profile.videos"));
                ui.checkbox(&mut self.profile_want_images, t(lang, "profile.images"));
            });
            ui.checkbox(
                &mut self.settings.use_browser_cookies,
                t(lang, "profile.cookies_inline"),
            );
            if is_douyin_profile(&self.profile_url) {
                ui.label(RichText::new(t(lang, "profile.douyin_note")).size(11.5).color(RED));
            } else if is_gallery_site(&self.profile_url) {
                ui.label(RichText::new(t(lang, "profile.gallery_note")).size(11.5).color(AMBER));
            }
            ui.add_space(8.0);
            if self.profile_analyzing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(t(lang, "profile.analyzing")).color(CYAN));
                });
            } else if primary_button(ui, t(lang, "btn.analyze")).clicked() {
                let url = self.profile_url.trim().to_string();
                if url.is_empty() || !url.starts_with("http") {
                    self.toast(t(lang, "profile.need_url"));
                } else if is_douyin_profile(&url) {
                    self.toast(t(lang, "profile.douyin_unsupported"));
                } else if is_gallery_site(&url) {
                    // Instagram, Weibo, Pinterest…: yt-dlp no puede enumerar el
                    // perfil. gallery-dl lo descarga completo de una pasada.
                    if self.galdl_cmd.is_some() {
                        // Quitar parámetros que rompen el extractor (?layerid=… en Weibo)
                        let url = normalize_profile_url(&url);
                        let author = Regex::new(r"(?:instagram\.com|pinterest\.[a-z]+|tumblr\.com)/([\w.\-]+)|weibo\.c[nom]+/u/(\d+)")
                            .unwrap()
                            .captures(&url)
                            .map(|c| {
                                c.get(1).or_else(|| c.get(2))
                                    .map(|m| m.as_str().to_string())
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();
                        let before = self.rows.len();
                        self.add_url(&url, &author, "", &url, "");
                        // Arrancar de inmediato: en estos sitios no hay lista previa
                        // que revisar, así que esperar a "Iniciar" solo confunde.
                        if self.rows.len() > before {
                            let i = self.rows.len() - 1;
                            self.start_row(i);
                        }
                        self.toast(t(lang, "profile.gallery_queued"));
                        self.view = View::Downloads;
                    } else {
                        self.toast(t(lang, "profile.need_galdl"));
                    }
                } else if let Some(prog) = self.ytdlp_cmd.clone() {
                    self.profile_analyzing = true;
                    self.profile_entries.clear();
                    let args = cookie_args(&self.settings);
                    let tx = self.tx.clone();
                    self.rt.spawn(analyze_profile(prog, url, args, tx));
                } else {
                    self.toast(t(lang, "profile.need_ytdlp"));
                }
            }
        });

        // Resultados del análisis
        if !self.profile_entries.is_empty() {
            ui.add_space(12.0);
            let visible: Vec<usize> = self
                .profile_entries
                .iter()
                .enumerate()
                .filter(|(_, e)| if e.is_image { self.profile_want_images } else { self.profile_want_videos })
                .map(|(i, _)| i)
                .collect();
            let selected = visible.iter().filter(|&&i| self.profile_entries[i].selected).count();
            let n_vid = self.profile_entries.iter().filter(|e| !e.is_image).count();
            let n_img = self.profile_entries.len() - n_vid;

            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width().min(720.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(i18n::posts_summary(lang, self.profile_entries.len(), n_vid, n_img))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if soft_button(ui, t(lang, "btn.none")).clicked() {
                            for e in &mut self.profile_entries {
                                e.selected = false;
                            }
                        }
                        if soft_button(ui, t(lang, "btn.all")).clicked() {
                            for e in &mut self.profile_entries {
                                e.selected = true;
                            }
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                    for &i in &visible {
                        let e = &mut self.profile_entries[i];
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut e.selected, "");
                            ui.label(if e.is_image { "🖼" } else { "🎬" });
                            let title: String = e.title.chars().take(64).collect();
                            ui.label(RichText::new(title).color(TEXT)).on_hover_text(&e.url);
                            ui.label(RichText::new(&e.id).size(11.0).color(MUTED));
                        });
                    }
                });
                ui.separator();
                if primary_button(ui, &i18n::add_selected(lang, selected)).clicked() && selected > 0 {
                    let author = {
                        let re = Regex::new(r"@([\w.\-]+)").unwrap();
                        re.captures(&self.profile_url)
                            .map(|c| c[1].to_string())
                            .unwrap_or_default()
                    };
                    let to_add: Vec<(String, String, String)> = visible
                        .iter()
                        .filter(|&&i| self.profile_entries[i].selected)
                        .map(|&i| {
                            let e = &self.profile_entries[i];
                            (e.url.clone(), e.title.clone(), e.id.clone())
                        })
                        .collect();
                    let n = to_add.len();
                    for (url, title, id) in to_add {
                        self.add_url(&url, &author, &title, &url, &id);
                    }
                    self.toast(i18n::added_to_queue(lang, n));
                    self.view = View::Downloads;
                }
            });
        }
    }

    // ---------------- Vista Capturar (Click'n'Load) ----------------

    fn capture_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "cap.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(4.0);
        ui.label(RichText::new(t(lang, "cap.subtitle")).color(MUTED));
        ui.add_space(14.0);

        // Estado del receptor
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.horizontal(|ui| {
                if self.settings.receiver_enabled {
                    ui.label(RichText::new(t(lang, "cap.listening")).color(GREEN));
                    ui.label(
                        RichText::new(format!("127.0.0.1:{}", self.settings.receiver_port))
                            .color(CYAN),
                    );
                } else {
                    ui.label(RichText::new(t(lang, "cap.off")).color(AMBER));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let chk = ui.checkbox(&mut self.settings.receiver_enabled, t(lang, "cap.enable"));
                    if chk.changed() {
                        self.recv_enabled
                            .store(self.settings.receiver_enabled, Ordering::Relaxed);
                    }
                });
            });
            ui.label(RichText::new(t(lang, "cap.note_restart")).size(11.5).color(MUTED));
        });
        ui.add_space(12.0);

        // Selector de sitio + pasos
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.label(RichText::new(t(lang, "cap.site")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (i, name) in ["TikTok", "Douyin"].iter().enumerate() {
                    let sel = self.capture_site == i;
                    let btn = egui::Button::new(
                        RichText::new(*name).color(if sel { Color32::WHITE } else { MUTED }),
                    )
                    .fill(if sel { ACCENT } else { CARD_HOVER })
                    .rounding(Rounding::same(8.0));
                    let r = ui.add(btn);
                    gloss_paint(ui, &r);
                    if r.clicked() {
                        self.capture_site = i;
                    }
                }
            });
            ui.add_space(8.0);
            ui.label(t(lang, "cap.step1"));
            ui.label(t(lang, "cap.step2"));
            ui.label(t(lang, "cap.step3"));
            ui.add_space(10.0);

            let script = if self.capture_site == 0 {
                scripts::tiktok(self.settings.receiver_port)
            } else {
                scripts::douyin(self.settings.receiver_port)
            };

            ui.horizontal(|ui| {
                if primary_button(ui, t(lang, "cap.copy")).clicked() {
                    ui.output_mut(|o| o.copied_text = script.clone());
                    self.toast(t(lang, "cap.copied"));
                }
                if soft_button(ui, t(lang, "cap.save")).clicked() {
                    let name = if self.capture_site == 0 {
                        "capturador_tiktok.js"
                    } else {
                        "capturador_douyin.js"
                    };
                    if let Some(p) = rfd::FileDialog::new().set_file_name(name).save_file() {
                        match std::fs::write(&p, &script) {
                            Ok(_) => self.toast(i18n::saved_to(lang, &p.display().to_string())),
                            Err(e) => self.toast(format!("{e}")),
                        }
                    }
                }
            });
        });
        ui.add_space(12.0);

        // Vista previa del script
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.label(RichText::new(t(lang, "cap.preview")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            let script = if self.capture_site == 0 {
                scripts::tiktok(self.settings.receiver_port)
            } else {
                scripts::douyin(self.settings.receiver_port)
            };
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .id_source("script_preview")
                .show(ui, |ui| {
                    let mut txt = script;
                    ui.add(
                        egui::TextEdit::multiline(&mut txt)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
        });
    }

    // ---------------- Vista de ajustes ----------------

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "set.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(16.0);

        // ---- Idioma ----
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.language")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t(lang, "set.language_label"));
                for l in Lang::ALL {
                    let selected = self.settings.lang == l;
                    let btn = egui::Button::new(
                        RichText::new(l.label()).color(if selected { Color32::WHITE } else { MUTED }),
                    )
                    .fill(if selected { ACCENT } else { CARD_HOVER })
                    .rounding(Rounding::same(8.0));
                    let resp = ui.add(btn);
                    gloss_paint(ui, &resp);
                    if resp.clicked() {
                        self.settings.lang = l;
                    }
                }
            });
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.folder")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let mut dest = self.settings.dest.clone();
                if ui
                    .add_sized([380.0, 30.0], egui::TextEdit::singleline(&mut dest))
                    .changed()
                {
                    self.settings.dest = dest;
                }
                if soft_button(ui, t(lang, "btn.browse")).clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.settings.dest = d.to_string_lossy().into_owned();
                    }
                }
                if soft_button(ui, t(lang, "btn.open")).clicked() {
                    let _ = open::that(&self.settings.dest);
                }
            });
            ui.checkbox(&mut self.settings.per_author, t(lang, "set.per_author"));
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.downloads")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t(lang, "set.concurrency"));
                ui.add(egui::Slider::new(&mut self.settings.concurrency, 1..=8));
            });
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.linkgrabber")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            let clip = ui.checkbox(
                &mut self.settings.clipboard_watch,
                t(lang, "set.clip_watch"),
            );
            if clip.changed() {
                self.clip_enabled
                    .store(self.settings.clipboard_watch, Ordering::Relaxed);
            }
            if self.settings.clipboard_watch {
                ui.checkbox(
                    &mut self.settings.auto_start_clipboard,
                    t(lang, "set.clip_autostart"),
                );
                let any = ui.checkbox(
                    &mut self.settings.grab_any_url,
                    t(lang, "set.clip_any"),
                );
                if any.changed() {
                    self.grab_any_flag
                        .store(self.settings.grab_any_url, Ordering::Relaxed);
                }
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.receiver")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            let chk = ui.checkbox(&mut self.settings.receiver_enabled, t(lang, "cap.enable"));
            if chk.changed() {
                self.recv_enabled
                    .store(self.settings.receiver_enabled, Ordering::Relaxed);
            }
            ui.horizontal(|ui| {
                ui.label(t(lang, "set.receiver_port"));
                ui.add(egui::DragValue::new(&mut self.settings.receiver_port).range(1024..=65535));
            });
            ui.label(RichText::new(t(lang, "cap.note_restart")).size(11.5).color(MUTED));
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.cookies")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            ui.checkbox(
                &mut self.settings.use_browser_cookies,
                t(lang, "set.cookies_use"),
            );
            if self.settings.use_browser_cookies {
                ui.horizontal(|ui| {
                    ui.label(t(lang, "set.cookies_browser"));
                    egui::ComboBox::from_id_source("cookies_browser")
                        .selected_text(&self.settings.cookies_browser)
                        .show_ui(ui, |ui| {
                            // Firefox primero: es el único no afectado por App-Bound Encryption
                            for b in ["firefox", "chrome", "edge", "brave", "opera", "vivaldi", "chromium"] {
                                let label = if b == "firefox" { "firefox  ✓" } else { b };
                                ui.selectable_value(&mut self.settings.cookies_browser, b.to_string(), label);
                            }
                        });
                });
                ui.label(RichText::new(t(lang, "set.cookies_note")).size(11.5).color(MUTED));
                ui.label(RichText::new(t(lang, "set.cookies_warn")).size(11.5).color(AMBER));
            }
            ui.add_space(6.0);
            ui.label(RichText::new(t(lang, "set.cookies_file")).size(11.5).color(MUTED));
            ui.horizontal(|ui| {
                let mut f = self.settings.cookies_file.clone();
                if ui
                    .add_sized([300.0, 26.0], egui::TextEdit::singleline(&mut f).hint_text("cookies.txt"))
                    .changed()
                {
                    self.settings.cookies_file = f;
                }
                if soft_button(ui, t(lang, "btn.pick_file")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("cookies.txt", &["txt"])
                        .pick_file()
                    {
                        self.settings.cookies_file = p.to_string_lossy().into_owned();
                    }
                }
                if !self.settings.cookies_file.is_empty() && soft_button(ui, t(lang, "btn.clear")).clicked() {
                    self.settings.cookies_file.clear();
                }
            });
            if !self.settings.cookies_file.is_empty() {
                ui.label(RichText::new(t(lang, "set.cookies_file_note")).size(11.5).color(MUTED));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "eng.ytdlp")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            if self.ytdlp_installing {
                ui.add(
                    egui::ProgressBar::new(self.ytdlp_progress)
                        .fill(CYAN)
                        .show_percentage(),
                );
                ui.label(RichText::new(t(lang, "eng.ytdlp_downloading")).color(MUTED));
            } else if self.ytdlp_ok == Some(true) {
                ui.label(RichText::new(t(lang, "eng.ytdlp_ok")).color(GREEN));
                if let Some(cmd) = &self.ytdlp_cmd {
                    ui.label(RichText::new(cmd.as_str()).size(11.5).color(MUTED));
                }
                if soft_button(ui, t(lang, "btn.update_latest")).clicked() {
                    self.ytdlp_installing = true;
                    self.ytdlp_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_ytdlp(client, tx));
                }
            } else {
                ui.label(RichText::new(t(lang, "eng.ytdlp_missing")).color(AMBER));
                if primary_button(ui, t(lang, "eng.install_ytdlp")).clicked() {
                    self.ytdlp_installing = true;
                    self.ytdlp_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_ytdlp(client, tx));
                }
                ui.label(RichText::new(t(lang, "eng.ytdlp_note")).size(11.5).color(MUTED));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "eng.galdl")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            if self.galdl_installing {
                ui.add(
                    egui::ProgressBar::new(self.galdl_progress)
                        .fill(CYAN)
                        .show_percentage(),
                );
                ui.label(RichText::new(t(lang, "eng.galdl_downloading")).color(MUTED));
            } else if let Some(cmd) = self.galdl_cmd.clone() {
                ui.label(RichText::new(t(lang, "eng.galdl_ok")).color(GREEN));
                ui.label(RichText::new(cmd).size(11.5).color(MUTED));
                if soft_button(ui, t(lang, "btn.update_latest")).clicked() {
                    self.galdl_installing = true;
                    self.galdl_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_gallerydl(client, tx));
                }
            } else {
                ui.label(RichText::new(t(lang, "eng.galdl_missing")).color(AMBER));
                if primary_button(ui, t(lang, "eng.install_galdl")).clicked() {
                    self.galdl_installing = true;
                    self.galdl_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_gallerydl(client, tx));
                }
                ui.label(RichText::new(t(lang, "eng.galdl_note")).size(11.5).color(MUTED));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "eng.ffmpeg")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            if self.ffmpeg_installing {
                ui.add(
                    egui::ProgressBar::new(self.ffmpeg_progress)
                        .fill(CYAN)
                        .show_percentage(),
                );
                ui.label(RichText::new(t(lang, "eng.ffmpeg_downloading")).color(MUTED));
            } else if let Some(cmd) = self.ffmpeg_cmd.clone() {
                ui.label(RichText::new(t(lang, "eng.ffmpeg_ok")).color(GREEN));
                ui.label(RichText::new(cmd).size(11.5).color(MUTED));
                ui.label(RichText::new(t(lang, "eng.ffmpeg_quality_on")).size(11.5).color(CYAN));
            } else {
                ui.label(RichText::new(t(lang, "eng.ffmpeg_missing")).color(AMBER));
                if primary_button(ui, t(lang, "eng.install_ffmpeg")).clicked() {
                    self.ffmpeg_installing = true;
                    self.ffmpeg_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_ffmpeg(client, tx));
                }
                ui.label(RichText::new(t(lang, "eng.ffmpeg_note")).size(11.5).color(MUTED));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "about")).size(11.0).color(MUTED).strong());
            ui.add_space(4.0);
            ui.label(format!(
                "Todo Downloader v{} — By Eric V. Gramunt",
                env!("CARGO_PKG_VERSION")
            ));
            ui.label(RichText::new(t(lang, "about.tech")).color(MUTED));
        });
    }
}
