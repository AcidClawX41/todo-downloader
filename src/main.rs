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

mod booru;
mod gallery;
mod hosters;
mod i18n;
mod mega;
mod receiver;
mod scripts;
mod torrents;
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

// ---------- Enlaces de apoyo al proyecto ----------
//
// EDITA SOLO ESTAS TRES LÍNEAS con tus usuarios reales. Un enlace que siga
// conteniendo «TU_USUARIO» no se muestra: es preferible ocultar el botón a
// enseñar uno roto.
//
// Ko-fi        -> https://ko-fi.com/manage  (tu nombre de página)
// PayPal.Me    -> https://paypal.me/  (crea el enlace; PayPal no permite
//                 cambiarlo después, así que elige bien el nombre)
// GitHub       -> el propio usuario, si tienes Sponsors activado
const KOFI_URL: &str = "https://ko-fi.com/ericdev";
const PAYPAL_URL: &str = "https://paypal.me/EricValls";
const SPONSORS_URL: &str = "https://github.com/sponsors/AcidClawX41";

/// Un enlace sin configurar no debe pintarse
fn link_ready(url: &str) -> bool {
    !url.contains("TU_USUARIO")
}


// ============================= Temas =============================
//
// Los colores no son constantes: se leen de una paleta intercambiable en
// caliente, para poder ofrecer varias «skins». Se accede por funciones cortas
// (bg(), card(), accent()…) que resuelven contra la paleta activa.

/// Skin seleccionable por el usuario
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Theme {
    /// Oscuro con acento rosa TikTok (el de siempre)
    #[default]
    Classic,
    /// Gris pizarra, sin rosa: discreto para entornos de trabajo
    Sober,
    /// Rosa intenso, con halos difuminados de fondo y gloss rosa al pasar
    HotPink,
}

impl Theme {
    const ALL: [Theme; 3] = [Theme::Classic, Theme::Sober, Theme::HotPink];

    /// Etiqueta traducida. Antes estaba escrita a fuego en español y no
    /// respetaba el idioma elegido.
    fn label(self, lang: Lang) -> &'static str {
        match self {
            Theme::Classic => t(lang, "theme.classic"),
            Theme::Sober => t(lang, "theme.sober"),
            Theme::HotPink => "Hot Pink", // nombre propio: igual en ambos idiomas
        }
    }

    /// ¿Pinta halos difuminados de fondo?
    fn has_glow(self) -> bool {
        matches!(self, Theme::HotPink)
    }

    fn palette(self) -> Palette {
        let rgb = Color32::from_rgb;
        match self {
            Theme::Classic => Palette {
                bg: rgb(15, 17, 21),
                panel: rgb(21, 24, 31),
                card: rgb(30, 34, 45),
                card_hover: rgb(38, 43, 57),
                accent: rgb(254, 44, 85),
                cyan: rgb(37, 244, 238),
                text: rgb(232, 234, 240),
                muted: rgb(138, 144, 160),
                green: rgb(61, 220, 132),
                amber: rgb(255, 180, 84),
                red: rgb(255, 84, 112),
            },
            // Azul pizarra, acento sobrio: nada llama la atención
            Theme::Sober => Palette {
                bg: rgb(18, 20, 24),
                panel: rgb(25, 28, 33),
                card: rgb(34, 38, 44),
                card_hover: rgb(45, 50, 58),
                accent: rgb(96, 132, 178),
                cyan: rgb(126, 176, 200),
                text: rgb(226, 229, 234),
                muted: rgb(139, 146, 158),
                green: rgb(104, 176, 130),
                amber: rgb(206, 168, 106),
                red: rgb(203, 106, 116),
            },
            // Rosa por todas partes, sobre un fondo cálido oscuro
            Theme::HotPink => Palette {
                bg: rgb(22, 10, 20),
                panel: rgb(32, 14, 29),
                card: rgb(46, 20, 41),
                card_hover: rgb(68, 28, 60),
                accent: rgb(255, 45, 130),
                cyan: rgb(255, 140, 200),
                text: rgb(255, 236, 247),
                muted: rgb(196, 148, 180),
                green: rgb(94, 226, 158),
                amber: rgb(255, 190, 120),
                red: rgb(255, 96, 130),
            },
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    panel: Color32,
    card: Color32,
    card_hover: Color32,
    accent: Color32,
    cyan: Color32,
    text: Color32,
    muted: Color32,
    green: Color32,
    amber: Color32,
    red: Color32,
}

/// Paleta activa. RwLock porque solo se escribe al cambiar de tema; las
/// lecturas por frame son miles pero baratísimas y sin contención.
fn palette() -> &'static std::sync::RwLock<Palette> {
    static P: std::sync::OnceLock<std::sync::RwLock<Palette>> = std::sync::OnceLock::new();
    P.get_or_init(|| std::sync::RwLock::new(Theme::Classic.palette()))
}

fn set_palette(theme: Theme) {
    if let Ok(mut p) = palette().write() {
        *p = theme.palette();
    }
}

macro_rules! pal {
    ($($fn_name:ident => $field:ident),* $(,)?) => {
        $(
            #[allow(non_snake_case)]
            fn $fn_name() -> Color32 {
                palette().read().map(|p| p.$field).unwrap_or(Color32::GRAY)
            }
        )*
    };
}

pal! {
    BG => bg, PANEL => panel, CARD => card, CARD_HOVER => card_hover,
    ACCENT => accent, CYAN => cyan, TEXT => text, MUTED => muted,
    GREEN => green, AMBER => amber, RED => red,
}

fn main() -> eframe::Result<()> {
    // Enlace recibido por línea de comandos: al pulsar un magnet en el
    // navegador, Windows lanza el programa con la URL como argumento.
    // También acepta una ruta a un .torrent (arrastrar sobre el ejecutable).
    let cli_link = std::env::args().nth(1).filter(|a| {
        let l = a.to_ascii_lowercase();
        l.starts_with("magnet:") || l.ends_with(".torrent")
    });

    // Si ya hay una instancia abierta, se le pasa el enlace por el receptor
    // local y salimos: así el clic en un magnet no abre una segunda ventana.
    if let Some(link) = &cli_link {
        if forward_to_running_instance(link) {
            return Ok(());
        }
    }

    // Icono de la ventana (barra de título, Alt+Tab, barra de tareas).
    // Es independiente del icono del .exe, que incrusta build.rs en Windows.
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1160.0, 700.0])
        // Mínimo calculado para que la tabla nunca recorte la columna de acciones
        .with_min_inner_size([1000.0, 520.0])
        .with_title("Todo Downloader");
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "todo-downloader-evg",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, cli_link)))),
    )
}

/// Decodifica el PNG del icono, incrustado en el binario en tiempo de compilación.
/// Devuelve `None` si algo falla: la app arranca igual, solo sin icono propio.
fn load_app_icon() -> Option<egui::IconData> {
    const PNG: &[u8] = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

/// Registra (o quita) Todo Downloader como manejador de los enlaces `magnet:`.
///
/// Se escribe en HKEY_CURRENT_USER, así que **no hace falta ser administrador**
/// y solo afecta a este usuario. Windows abre el programa registrado aquí
/// cuando pulsas un magnet en el navegador, pasándole la URL como argumento.
///
/// Se usa `reg.exe` en vez de una dependencia del registro para no añadir otro
/// crate por cuatro claves.
#[cfg(windows)]
fn set_magnet_handler(enable: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    const KEY: &str = r"HKCU\Software\Classes\magnet";

    let run = |args: Vec<String>| -> Result<(), String> {
        let out = std::process::Command::new("reg")
            .args(&args)
            .creation_flags(NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };

    if !enable {
        // Quitar el registro: Windows vuelve al manejador anterior (qBittorrent…)
        return run(vec!["delete".into(), KEY.into(), "/f".into()]);
    }

    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();

    // Estructura estándar de un protocolo: la clave, el marcador URL Protocol,
    // y el comando con "%1" (la URL que pasa Windows).
    run(vec!["add".into(), KEY.into(), "/ve".into(), "/d".into(), "URL:magnet".into(), "/f".into()])?;
    run(vec!["add".into(), KEY.into(), "/v".into(), "URL Protocol".into(), "/d".into(), String::new(), "/f".into()])?;
    run(vec![
        "add".into(),
        format!(r"{KEY}\shell\open\command"),
        "/ve".into(),
        "/d".into(),
        format!("\"{exe}\" \"%1\""),
        "/f".into(),
    ])?;

    // --- Registro como aplicación con capacidades ---
    //
    // Windows 10/11 protege el programa por defecto con `UserChoice`, una clave
    // firmada con un hash que NINGUNA aplicación puede escribir (es a propósito,
    // para impedir secuestros de asociaciones). Por eso lo anterior no basta si
    // ya hay otro cliente puesto por defecto.
    //
    // La vía legítima es publicar las «capacidades» de la app para que aparezca
    // en Configuración → Aplicaciones predeterminadas y sea el USUARIO quien la
    // elija. Eso es lo que se registra aquí.
    const CAP: &str = r"HKCU\Software\TodoDownloader\Capabilities";
    run(vec!["add".into(), CAP.into(), "/v".into(), "ApplicationName".into(), "/d".into(), "Todo Downloader".into(), "/f".into()])?;
    run(vec![
        "add".into(), CAP.into(), "/v".into(), "ApplicationDescription".into(),
        "/d".into(), "Lightweight download manager with BitTorrent".into(), "/f".into(),
    ])?;
    run(vec![
        "add".into(), format!(r"{CAP}\URLAssociations"), "/v".into(), "magnet".into(),
        "/d".into(), "TodoDownloader.Magnet".into(), "/f".into(),
    ])?;
    // ProgID al que apunta la asociación
    const PROGID: &str = r"HKCU\Software\Classes\TodoDownloader.Magnet";
    run(vec!["add".into(), PROGID.into(), "/ve".into(), "/d".into(), "Magnet Link".into(), "/f".into()])?;
    run(vec![
        "add".into(), format!(r"{PROGID}\shell\open\command"), "/ve".into(),
        "/d".into(), format!("\"{exe}\" \"%1\""), "/f".into(),
    ])?;
    // Alta en el listado de aplicaciones registradas del sistema
    run(vec![
        "add".into(), r"HKCU\Software\RegisteredApplications".into(),
        "/v".into(), "Todo Downloader".into(),
        "/d".into(), r"Software\TodoDownloader\Capabilities".into(), "/f".into(),
    ])?;
    Ok(())
}

/// ¿Somos ya el manejador de magnet? (compara la ruta registrada con la nuestra)
#[cfg(windows)]
fn is_magnet_handler() -> bool {
    use std::os::windows::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = exe.to_string_lossy().to_ascii_lowercase();
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Classes\magnet\shell\open\command", "/ve"])
        .creation_flags(0x0800_0000)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .to_ascii_lowercase()
            .contains(&exe),
        _ => false,
    }
}

#[cfg(not(windows))]
fn set_magnet_handler(_enable: bool) -> Result<(), String> {
    Err("solo disponible en Windows".into())
}

#[cfg(not(windows))]
fn is_magnet_handler() -> bool {
    false
}

/// Intenta entregar el enlace a una instancia ya en marcha usando el receptor
/// local (el mismo de Click'n'Load). Devuelve true si lo aceptó.
///
/// Se prueban los puertos habituales porque el receptor es configurable; con
/// que uno responda, la instancia viva se queda el enlace.
fn forward_to_running_instance(link: &str) -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    // El puerto por defecto primero; los siguientes cubren cambios manuales.
    for port in [9777u16, 9778, 9779] {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let body = serde_json::json!({ "items": [ { "url": link } ] }).to_string();
        let req = format!(
            "POST /add HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        if s.write_all(req.as_bytes()).is_err() {
            continue;
        }
        let mut resp = String::new();
        let _ = s.read_to_string(&mut resp);
        if resp.starts_with("HTTP/1.1 200") {
            return true;
        }
    }
    false
}

/// Elementos por página al explorar una galería.
///
/// Deliberadamente bajo. Instagram espacia sus peticiones 6-12 segundos, así
/// que pedir 200 de golpe deja la exploración parada varios minutos sin
/// mostrar nada. Mejor traer poco y enseñarlo enseguida.
const GALLERY_PER_PAGE: u32 = 30;

// ============================= Modelo =============================

#[derive(Clone, PartialEq)]
enum Status {
    Queued,
    Waiting,
    Downloading,
    Resolving, // yt-dlp
    /// Comprobando el MAC de MEGA antes de renombrar el .part.
    /// Es su propio estado a propósito: en un archivo grande la pasada de
    /// integridad tarda, y sin fase visible parecería que se ha colgado.
    Verifying,
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
                Engine::FileHost => t(lang, "status.resolving_host").into(),
                Engine::Cyberdrop => "cyberdrop-dl".into(),
                Engine::Mega => t(lang, "status.resolving_mega").into(),
                _ => t(lang, "status.resolving").into(),
            },
            Status::Verifying => t(lang, "status.verifying").into(),
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
            Status::Done => GREEN(),
            Status::Downloading | Status::Resolving | Status::Verifying => CYAN(),
            Status::Paused => AMBER(),
            Status::Error(_) => RED(),
            _ => MUTED(),
        }
    }
    fn is_active(&self) -> bool {
        matches!(
            self,
            Status::Waiting | Status::Downloading | Status::Resolving | Status::Verifying
        )
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
    /// Nombre del archivo que gallery-dl está escribiendo ahora mismo
    gal_current: String,
    error_full: String,
    cancel: Arc<AtomicBool>,
    /// URL de la portada del post; vacía si el origen no la proporcionó
    thumb_url: String,
    /// Cookie de descarga (GoFile la exige); vacía si no aplica
    dl_cookie: String,
}

/// Un archivo resuelto por un hoster, para expandir la fila origen en varias
struct HostItem {
    url: String,
    filename: String,
    cookie: String,
    /// Motor de la fila resultante. Los hosters nativos resuelven a HTTP
    /// directo; una carpeta de MEGA expande a filas Engine::Mega.
    engine: Engine,
}

// Algunas variantes solo se emiten en Windows (instalador de ffmpeg)
#[cfg_attr(not(windows), allow(dead_code))]
enum Ev {
    Status(u64, Status),
    Size(u64, u64),
    Progress(u64, u64, f64),
    Clipboard(Vec<String>),
    Received(Vec<receiver::Incoming>),
    /// (id, nº de archivos completados, nombre del que acaba de escribirse)
    GalFiles(u64, u64, String),
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
    /// Miniatura descargada y decodificada, lista para subir a la GPU
    Thumb(u64, egui::ColorImage),
    /// Un hoster resolvió su URL en estos archivos directos: se expande la fila
    FileHostResolved(u64, Vec<HostItem>),
    Cyberdrop(Option<String>),
    CyberdropProgress(f32),
    CyberdropError(String),
    /// La sesión BitTorrent quedó lista (creada de forma perezosa al 1er torrent)
    TorrentClientReady(Arc<torrents::Client>),
    /// Torrent añadido: id de la app, handle de librqbit, nombre provisional
    TorrentAdded(u64, Arc<librqbit::ManagedTorrent>, String),
    TorrentError(String),
    /// Resultados de explorar una galería (Instagram, Weibo): (elementos, página)
    GalleryResults(Vec<gallery::GalleryItem>, u32, u64),
    GalleryError(String, u64),
    /// Miniatura de un elemento de galería ya decodificada (índice, imagen)
    GalleryThumb(usize, egui::ColorImage),
    /// Miniatura de una entrada del análisis de perfil (TikTok, Bilibili…)
    ProfileThumb(usize, egui::ColorImage),
    /// La miniatura no se pudo obtener (CDN caducado, anti-hotlink, formato
    /// que no decodifica). Sin este aviso la celda se quedaba con los puntos
    /// suspensivos para siempre, dando a entender que seguía cargando.
    /// `true` = rejilla de perfil, `false` = rejilla de galería.
    ThumbFailed(usize, bool),
    /// Resultados de una búsqueda en un booru, con su número de generación
    BooruResults(Vec<booru::Post>, u64),
    BooruError(String, u64),
    /// Miniatura de un post de booru ya decodificada
    BooruThumb(u64, egui::ColorImage),
}

/// Motor de descarga por tarea
#[derive(Clone, Copy, PartialEq)]
enum Engine {
    Http,      // enlace directo a archivo (CDN)
    YtDlp,     // página de vídeo (TikTok, YouTube, Instagram, X…)
    GalleryDl, // post de imágenes (TikTok /photo/, Douyin /note/)
    FileHost,  // hoster con API abierta resuelto en Rust (Pixeldrain, GoFile, MediaFire)
    Cyberdrop, // motor opcional para hosters difíciles (Bunkr, Cyberdrop…): necesita Python
    Mega,      // enlaces públicos de MEGA: descifrado nativo en Rust (src/mega)
}

/// Entrada detectada al analizar un perfil
#[derive(Clone)]
struct ProfileEntry {
    selected: bool,
    id: String,
    title: String,
    url: String,
    is_image: bool,
    /// Portada que devuelve yt-dlp; alimenta la miniatura de la cola
    thumb: String,
}

/// Sitios que el LinkGrabber captura por defecto
const KNOWN_SITES: &[&str] = &[
    "tiktok.com", "douyin.com", "youtube.com", "youtu.be", "instagram.com",
    "twitter.com", "x.com", "reddit.com", "twitch.tv", "vimeo.com",
    "facebook.com", "bilibili.com", "b23.tv", "soundcloud.com", "dailymotion.com",
    // Hosters de archivos (grupo 2)
    "pixeldrain.com", "gofile.io", "mediafire.com", "bunkr.", "cyberdrop.",
    "saint.to", "saint2.su", "pixl.li",
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
    "flickr.com", "tumblr.com", "artstation.com",
    // Boorus: gallery-dl trae extractores para todos estos y se actualiza
    // cuando cambian, así que basta con enrutarlos hacia él.
    "danbooru", "gelbooru", "safebooru", "aibooru", "e621.net", "e926.net",
    "yande.re", "konachan", "rule34.xxx", "tbib.org", "hypnohub.net",
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

/// Fuerza UTF-8 en la entrada/salida de los ayudantes escritos en Python.
///
/// POR QUÉ: en Windows, yt-dlp y gallery-dl heredan la página de códigos del
/// sistema (cp1252 en un Windows en español). Cuando el título de un vídeo
/// trae un carácter que esa página no sabe representar —un emoji, un kanji,
/// unas comillas tipográficas— el `TextIOWrapper` de stdout revienta al
/// escribirlo y el intérprete aborta con `OSError: [Errno 22] Invalid
/// argument`. El vídeo no falla por el vídeo: falla por su título, y por eso
/// los de al lado del mismo perfil se descargan sin problema.
///
/// `PYTHONIOENCODING` fija la codificación de stdout/stderr; `PYTHONUTF8`
/// activa el modo UTF-8 completo, que además cubre los nombres de archivo.
/// En Linux y macOS ya es lo normal, así que no cambia nada allí.
fn utf8_env(cmd: &mut tokio::process::Command) {
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.env("PYTHONUTF8", "1");
}

/// Reescribe un perfil de Weibo hacia la pestaña «álbum».
///
/// ESTO NO ES UN APAÑO NI UNA PREFERENCIA ESTÉTICA: es lo que decide la
/// resolución de las imágenes. Los dos caminos de gallery-dl no son
/// equivalentes.
///
/// - `tabtype=feed` → `/ajax/statuses/mymblog`. Devuelve las publicaciones tal
///   como vienen en el muro, y el `pic_infos` de ese listado trae variantes ya
///   reducidas: por eso el explorador enseñaba 810×1080 mientras el mismo post
///   pegado a mano bajaba a resolución completa. Además responde 403 a quien no
///   lleve sesión (necesita las cookies `SUB` y `SUBP` de `.weibo.com`).
///
/// - `tabtype=album` → `/ajax/profile/getImageWall`, y por cada entrada vuelve
///   a pedir la publicación con `/ajax/statuses/show`, que es EXACTAMENTE la
///   misma llamada que hace el extractor de post suelto. Misma respuesta, mismo
///   `largest`, misma resolución que copiando la URL del post.
///
/// De regalo, el muro de fotos solo lista publicaciones con imagen o vídeo: las
/// de texto no aparecen, que es justo lo que se quería en la rejilla.
fn weibo_album_url(url: &str) -> Option<String> {
    let host = host_of(url)?;
    if !(host_matches(&host, "weibo.com") || host_matches(&host, "weibo.cn")) {
        return None;
    }
    let low = url.to_ascii_lowercase();
    // Ya se probó el álbum, o no es una URL de perfil
    if low.contains("tabtype=album") || !low.contains("/u/") {
        return None;
    }
    let base = url.split('?').next()?.trim_end_matches('/');
    Some(format!("{base}?tabtype=album"))
}

/// Vuelta atrás: del muro de fotos al feed.
///
/// El álbum es mejor cuando funciona, pero no todas las cuentas lo tienen
/// poblado. Si vuelve vacío, se prueba el feed antes de rendirse.
fn weibo_feed_url(url: &str) -> Option<String> {
    let host = host_of(url)?;
    if !(host_matches(&host, "weibo.com") || host_matches(&host, "weibo.cn")) {
        return None;
    }
    if !url.to_ascii_lowercase().contains("tabtype=album") {
        return None;
    }
    let base = url.split('?').next()?.trim_end_matches('/');
    Some(format!("{base}?tabtype=feed"))
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

/// Hosters "difíciles" (ofuscación cambiante, guerra de scrapers) que solo
/// cubre el motor opcional cyberdrop-dl. No se resuelven de forma nativa porque
/// cambian cada semana a propósito para romper a los descargadores.
const CYBERDROP_SITES: &[&str] = &[
    "bunkr.", "cyberdrop.me", "cyberdrop.to", "cyberfile.me", "pixl.li",
    "jpg.church", "jpg5.su", "saint.to", "saint2.su", "gofile.io/d/",
];

/// ¿La URL la maneja el motor opcional cyberdrop-dl? (excluye lo que ya
/// resolvemos de forma nativa, que tiene prioridad).
fn is_cyberdrop_site(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    if hosters::is_filehost(&u) {
        return false;
    }
    CYBERDROP_SITES.iter().any(|s| u.contains(s))
}

fn engine_for_url(url: &str) -> Engine {
    // MEGA va primero: sus enlaces no se parecen a nada de lo de abajo y el
    // descifrado es inseparable de la descarga, así que ningún otro motor
    // puede encargarse de ellos.
    if mega::is_mega_url(url) {
        return Engine::Mega;
    }
    // Hosters con API abierta (Pixeldrain, GoFile, MediaFire): resolución nativa
    if hosters::is_filehost(url) {
        return Engine::FileHost;
    }
    if is_cyberdrop_site(url) {
        return Engine::Cyberdrop;
    }
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
    // Bilibili: sus CDN (upos-…bilivideo.com, hdslb.com) rechazan cualquier
    // petición sin Referer del propio sitio (anti-hotlink estricto).
    } else if u.contains("bilibili") || u.contains("bilivideo") || u.contains("hdslb") {
        "https://www.bilibili.com/"
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

/// Decodifica %XX y '+' (espacios) de un valor de query, para el `dn=` del magnet
fn urldecode_plus(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
    Booru,
    Torrents,
    Support,
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
    /// Skin de la interfaz
    theme: Theme,
    /// Ruta a una imagen de fondo para el panel principal (vacío = ninguna)
    bg_image: String,
    /// Intensidad del fondo, 0.0–1.0. Bajo por defecto para no estorbar
    bg_opacity: f32,
    /// Sigma del desenfoque gaussiano del fondo (0 = nítido)
    bg_blur: f32,
    /// Credenciales de boorus: usuario/clave por clave de extractor
    booru_user: String,
    booru_key: String,
    receiver_enabled: bool,
    receiver_port: u16,
    /// Carpeta de destino de los torrents (vacío = <dest>/Torrents)
    torrent_dir: String,
    /// Límite de descarga de torrents en KiB/s (0 = sin límite)
    torrent_down_kbps: u32,
    /// Límite de subida de torrents en KiB/s (0 = sin límite)
    torrent_up_kbps: u32,
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
            theme: Theme::default(),
            bg_image: String::new(),
            bg_opacity: 0.22,
            bg_blur: 0.0,
            booru_user: String::new(),
            booru_key: String::new(),
            receiver_enabled: true,
            receiver_port: 9777,
            torrent_dir: String::new(),
            torrent_down_kbps: 0,
            torrent_up_kbps: 0,
        }
    }
}

impl Settings {
    /// Carpeta efectiva de torrents: la elegida, o <dest>/Torrents por defecto.
    fn torrent_folder(&self) -> PathBuf {
        if self.torrent_dir.trim().is_empty() {
            PathBuf::from(&self.dest).join("Torrents")
        } else {
            PathBuf::from(self.torrent_dir.trim())
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

/// Host de una URL, en minúsculas, sin userinfo ni puerto.
/// `None` si no es una URL absoluta (sin esquema) o no tiene host.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?; // descarta usuario:clave@
    let host = host.split(':').next()?; // descarta :puerto
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// ¿`host` es exactamente `suffix` o un subdominio suyo?
///
/// Comparación estructural, no por subcadena. Con `contains()`,
/// «weibo.com.atacante.net» pasaba por ser weibo.com, y «passport.weibo.com»
/// era indistinguible de «weibo.com» — que es justo lo que rompía el routing.
fn host_matches(host: &str, suffix: &str) -> bool {
    host == suffix || host.ends_with(&format!(".{suffix}"))
}

/// ¿Este sitio necesita cookies desde el PRIMER intento?
///
/// YouTube **no**, y es importante: en cuanto yt-dlp encuentra cookies de
/// cuenta de YouTube cambia al cliente `web_creator`, que exige un PO Token
/// ligado al ID del vídeo. Sin proveedor de PO Token se descartan TODOS los
/// formatos y la descarga muere con «Requested format is not available»
/// aunque el vídeo sea público (yt-dlp#16569). El contenido público se baja
/// sin problema sin cookies.
///
/// Instagram, Weibo y las redes sociales **sí**: sin sesión devuelven 401 o
/// una página de login antes de listar nada, así que empezar sin cookies solo
/// gasta una petición.
///
/// Para todo lo demás se empieza sin cookies y se escala solo si el error lo
/// pide (ver `needs_auth_error`): menos exposición de las cookies del usuario
/// a sitios que no las necesitan.
fn needs_cookies_upfront(url: &str) -> bool {
    const AUTH_FIRST: &[&str] = &[
        "instagram.com",
        "weibo.com",
        "weibo.cn",
        "facebook.com",
        "twitter.com",
        "x.com",
    ];
    let Some(host) = host_of(url) else { return false };
    AUTH_FIRST.iter().any(|s| host_matches(&host, s))
}

/// ¿El error indica que hace falta autenticarse DE VERDAD?
///
/// Distinto de `is_cookie_error`, que detecta cookies ilegibles. Este decide
/// si merece la pena reintentar *añadiendo* cookies: login, vídeo privado,
/// restricción de edad, contenido solo para miembros.
fn needs_auth_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("login required")
        || m.contains("sign in to confirm")
        || m.contains("this video is private")
        || m.contains("private video")
        || m.contains("members-only")
        || m.contains("join this channel")
        || m.contains("age-restricted")
        || m.contains("confirm your age")
        || m.contains("account associated with this")
        || m.contains("http error 401")
        || m.contains("http error 403")
}

/// Nombre del autor/perfil deducido de la propia URL.
///
/// Hasta ahora el autor solo lo aportaban la vista Perfil y el capturador del
/// navegador. Pegar un perfil de Instagram en Descargas dejaba el autor vacío
/// y, aunque «Crear subcarpeta por autor» estuviese activado, todo caía suelto
/// en la raíz de la carpeta de descargas mezclado con lo demás.
///
/// Devuelve cadena vacía cuando la URL no identifica a un autor. Es preferible
/// no crear carpeta a inventarse una con un trozo cualquiera de la ruta.
fn author_from_url(url: &str) -> String {
    let Some(host) = host_of(url) else { return String::new() };

    let path = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segs: Vec<&str> = path.split('/').skip(1).filter(|s| !s.is_empty()).collect();
    let first = segs.first().copied().unwrap_or("");

    /// Segmentos que son secciones del sitio, no perfiles.
    const NOT_PROFILES: &[&str] = &[
        "p", "tv", "reel", "reels", "stories", "explore", "accounts", "direct",
        "status", "i", "home", "search", "hashtag", "watch", "shorts", "channel",
        "video", "photo", "note", "u", "tag", "pin", "media", "about", "help",
    ];

    /// Valida que el segmento parezca un nombre de usuario y no basura.
    fn as_user(seg: &str) -> String {
        let s = seg.trim_start_matches('@');
        let plausible = !s.is_empty()
            && s.len() <= 40
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if plausible && !NOT_PROFILES.contains(&s.to_ascii_lowercase().as_str()) {
            s.to_string()
        } else {
            String::new()
        }
    }

    // Weibo identifica al autor con un número: weibo.com/u/1234567.
    // Se prefija para que la carpeta no sea un número suelto sin contexto.
    if host_matches(&host, "weibo.com") || host_matches(&host, "weibo.cn") {
        if first == "u" {
            if let Some(id) = segs.get(1) {
                let id = as_user(id);
                if !id.is_empty() {
                    return format!("weibo_{id}");
                }
            }
        }
        return String::new();
    }

    // TikTok, Douyin y YouTube marcan el perfil con @: exigirlo evita tomar
    // «watch» o «video» por un nombre de usuario.
    if host_matches(&host, "tiktok.com")
        || host_matches(&host, "douyin.com")
        || host_matches(&host, "youtube.com")
    {
        return if first.starts_with('@') { as_user(first) } else { String::new() };
    }

    // Sitios donde el primer segmento de la ruta es el perfil
    const FIRST_SEG_IS_USER: &[&str] = &[
        "instagram.com", "twitter.com", "x.com", "tumblr.com",
        "deviantart.com", "artstation.com",
    ];
    let pinterest = host == "pinterest.com"
        || host.starts_with("pinterest.")
        || host.contains(".pinterest.");
    if pinterest || FIRST_SEG_IS_USER.iter().any(|s| host_matches(&host, s)) {
        return as_user(first);
    }

    String::new()
}

/// Selector de formato de yt-dlp.
///
/// Con ffmpeg se piden los mejores flujos por separado y se fusionan, que es
/// la única forma de pasar de 720p en YouTube y de bajar cualquier cosa de
/// Bilibili (solo sirve DASH).
///
/// Sin ffmpeg, SOLO formatos ya fusionados. El respaldo anterior era
/// `b/bv*+ba`, que ante la ausencia de un premezclado intentaba fusionar sin
/// fusionador y abortaba con OSError [Errno 2] — exactamente lo que se quería
/// evitar.
fn format_selector(has_ffmpeg: bool) -> &'static str {
    if has_ffmpeg {
        "bv*+ba/b"
    } else {
        "b"
    }
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

/// Tiempo restante en formato compacto (12s, 4m, 1h 20m, 2d)
fn fmt_eta(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return "—".into();
    }
    let s = secs as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d {}h", s / 86_400, (s % 86_400) / 3600)
    }
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
    cyberdrop_cmd: Option<String>,
    cyberdrop_installing: bool,
    cyberdrop_progress: f32,
    profile_url: String,
    profile_want_videos: bool,
    /// Portadas del análisis de perfil, por índice en `profile_entries`
    profile_thumbs: std::collections::HashMap<usize, egui::TextureHandle>,
    profile_pending: std::collections::HashSet<usize>,
    profile_failed: std::collections::HashSet<usize>,
    profile_want_images: bool,
    profile_analyzing: bool,
    profile_entries: Vec<ProfileEntry>,
    view: View,
    show_add: bool,
    add_text: String,
    search: String,
    toast: String,
    toast_until: Option<Instant>,
    /// Texturas de miniatura ya subidas a la GPU, por id de fila
    thumbs: std::collections::HashMap<u64, egui::TextureHandle>,
    /// Ids con descarga de miniatura en curso (o fallida: no se reintenta)
    thumbs_pending: std::collections::HashSet<u64>,
    // ---- BitTorrent ----
    torrent_client: Option<Arc<torrents::Client>>,
    torrents: Vec<torrents::Handle>,
    next_torrent_id: u64,
    torrent_input: String,
    torrent_adding: bool,
    /// Cálculo de velocidad por torrent: (bytes previos, instante, velocidad)
    torrent_speed: std::collections::HashMap<u64, (u64, Instant, f64)>,
    /// Ancla del desplazamiento automático con el botón central del ratón
    autoscroll: Option<egui::Pos2>,
    /// Textura del fondo personalizado y ruta con la que se cargó
    bg_texture: Option<egui::TextureHandle>,
    bg_loaded_from: String,
    /// Imagen original en memoria: permite re-difuminar sin releer del disco
    bg_source: Option<image::DynamicImage>,
    /// Marca que hay que regenerar la textura (cambió la ruta o el desenfoque)
    bg_dirty: bool,
    // ---- Animación de la pestaña de apoyo ----
    /// Fotogramas del GIF elegido: (textura, duración en segundos)
    tip_frames: Vec<(egui::TextureHandle, f32)>,
    /// Instante en que empezó la animación, para saber qué fotograma toca
    tip_started: Option<Instant>,
    /// Se pone al entrar en la pestaña: fuerza elegir otro GIF al azar
    tip_reload: bool,
    // ---- Booru Browser ----
    booru_site: usize,
    booru_tags: String,
    booru_page: u32,
    /// Generación de la búsqueda en curso. Una respuesta con un número
    /// distinto llega de una búsqueda que el usuario ya descartó, y pisarla
    /// mostraría resultados de otro personaje o de otro sitio.
    booru_epoch: u64,
    booru_posts: Vec<booru::Post>,
    /// Explorador de galerías: elementos listados sin descargar
    gallery_items: Vec<gallery::GalleryItem>,
    gallery_page: u32,
    /// Igual que en Booru: descarta listados de un perfil ya abandonado
    gallery_epoch: u64,
    /// Filtro PROPIO de la rejilla de galerías. Separado del de TikTok a
    /// propósito: son dos flujos distintos y compartir el estado hacía que
    /// tocar uno cambiara el otro sin que se viera la relación.
    gallery_want_images: bool,
    gallery_want_videos: bool,
    gallery_loading: bool,
    /// URL del perfil que se está explorando
    gallery_url: String,
    /// Último motivo de fallo, íntegro y legible en la propia vista
    gallery_error: String,
    /// Texturas de previsualización, por índice en `gallery_items`
    gallery_thumbs: std::collections::HashMap<usize, egui::TextureHandle>,
    /// Índices con petición en vuelo, para no pedir la misma dos veces
    gallery_pending: std::collections::HashSet<usize>,
    gallery_failed: std::collections::HashSet<usize>,
    booru_searching: bool,
    booru_min_w: u32,
    /// Filtro de clasificación: "" = todo, si no la letra del booru (g/s/q/e)
    booru_rating: String,
    booru_thumbs: std::collections::HashMap<u64, egui::TextureHandle>,
    booru_pending: std::collections::HashSet<u64>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, cli_link: Option<String>) -> Self {
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
        // La paleta debe estar puesta ANTES de construir el estilo
        set_palette(settings.theme);
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
        // Limpieza defensiva: si la app se cerró de golpe durante una búsqueda,
        // el archivo temporal de credenciales pudo quedar huérfano.
        let _ = std::fs::remove_file(ytdlp_dir().join("booru-auth.json"));

        spawn_ytdlp_check(tx.clone());
        spawn_galdl_check(tx.clone());
        spawn_ffmpeg_check(tx.clone());
        spawn_cyberdrop_check(tx.clone());

        let mut app = Self {
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
            cyberdrop_cmd: None,
            cyberdrop_installing: false,
            cyberdrop_progress: 0.0,
            profile_url: String::new(),
            profile_want_videos: true,
            profile_thumbs: std::collections::HashMap::new(),
            profile_pending: std::collections::HashSet::new(),
            profile_failed: std::collections::HashSet::new(),
            profile_want_images: true,
            profile_analyzing: false,
            profile_entries: Vec::new(),
            view: View::Downloads,
            show_add: false,
            add_text: String::new(),
            search: String::new(),
            toast: String::new(),
            toast_until: None,
            thumbs: std::collections::HashMap::new(),
            thumbs_pending: std::collections::HashSet::new(),
            torrent_client: None,
            torrents: Vec::new(),
            next_torrent_id: 0,
            torrent_input: String::new(),
            torrent_adding: false,
            torrent_speed: std::collections::HashMap::new(),
            autoscroll: None,
            bg_texture: None,
            bg_loaded_from: String::new(),
            bg_source: None,
            bg_dirty: false,
            tip_frames: Vec::new(),
            tip_started: None,
            tip_reload: true,
            booru_site: 0,
            booru_tags: String::new(),
            booru_page: 1,
            booru_epoch: 0,
            booru_posts: Vec::new(),
            gallery_items: Vec::new(),
            gallery_page: 1,
            gallery_epoch: 0,
            gallery_want_images: true,
            gallery_want_videos: true,
            gallery_loading: false,
            gallery_url: String::new(),
            gallery_error: String::new(),
            gallery_thumbs: std::collections::HashMap::new(),
            gallery_pending: std::collections::HashSet::new(),
            gallery_failed: std::collections::HashSet::new(),
            booru_searching: false,
            booru_min_w: 0,
            booru_rating: String::new(),
            booru_thumbs: std::collections::HashMap::new(),
            booru_pending: std::collections::HashSet::new(),
        };

        // Enlace recibido por línea de comandos (clic en un magnet del navegador)
        if let Some(link) = cli_link {
            app.view = View::Torrents;
            app.add_torrent(link);
        }
        app
    }

    /// Añade un magnet / URL .torrent / ruta local a la sesión BitTorrent,
    /// creando la sesión de forma perezosa la primera vez.
    fn add_torrent(&mut self, source: String) {
        let src = source.trim().to_string();
        if src.is_empty() {
            return;
        }
        self.torrent_adding = true;
        let id = self.next_torrent_id;
        self.next_torrent_id += 1;
        // Nombre provisional: el `dn=` del magnet, si lo trae
        let provisional = Regex::new(r"[?&]dn=([^&]+)")
            .ok()
            .and_then(|re| re.captures(&src).map(|c| urldecode_plus(&c[1])))
            .unwrap_or_else(|| "torrent".into());

        let existing = self.torrent_client.clone();
        let folder = self.settings.torrent_folder();
        let limits = torrents::Limits {
            download_kbps: self.settings.torrent_down_kbps,
            upload_kbps: self.settings.torrent_up_kbps,
        };
        // La carpeta base la fija la sesión (creada una vez). Como destino de
        // ESTE torrent se pasa también la carpeta actual, así respeta cambios.
        let out = folder.to_string_lossy().into_owned();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let client = match existing {
                Some(c) => c,
                None => match torrents::Client::new(folder, limits).await {
                    Ok(c) => {
                        let a = Arc::new(c);
                        let _ = tx.send(Ev::TorrentClientReady(a.clone()));
                        a
                    }
                    Err(e) => {
                        let _ = tx.send(Ev::TorrentError(e.to_string()));
                        return;
                    }
                },
            };
            match client.add(&src, Some(out)).await {
                Ok(h) => {
                    let _ = tx.send(Ev::TorrentAdded(id, h, provisional));
                }
                Err(e) => {
                    let _ = tx.send(Ev::TorrentError(e.to_string()));
                }
            }
        });
    }

    /// Desplazamiento automático estilo navegador: clic central para anclar,
    /// luego el desplazamiento va en la dirección y a la velocidad que marque
    /// la distancia del ratón al ancla. Otro clic, o Esc, lo cancela.
    ///
    /// Se implementa inyectando delta de scroll en la entrada del frame, antes
    /// de pintar nada: así lo consume el área de scroll que esté bajo el ratón,
    /// sea la tabla, los ajustes o la vista de perfil.
    fn handle_autoscroll(&mut self, ctx: &egui::Context) {
        let (middle, other, esc, pos) = ctx.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.button_pressed(egui::PointerButton::Primary)
                    || i.pointer.button_pressed(egui::PointerButton::Secondary),
                i.key_pressed(egui::Key::Escape),
                i.pointer.hover_pos(),
            )
        });

        if middle {
            self.autoscroll = if self.autoscroll.is_some() { None } else { pos };
        } else if (other || esc) && self.autoscroll.is_some() {
            self.autoscroll = None;
        }

        let (Some(anchor), Some(p)) = (self.autoscroll, pos) else { return };

        // Zona muerta: sin ella el más mínimo temblor de mano ya desplaza
        const DEAD: f32 = 14.0;
        let dy = p.y - anchor.y;
        if dy.abs() > DEAD {
            // Ratón por debajo del ancla → bajar (delta de scroll negativo)
            let step = ((dy.abs() - DEAD) / 4.0).min(90.0) * -dy.signum();
            ctx.input_mut(|i| {
                i.smooth_scroll_delta.y += step;
                i.raw_scroll_delta.y += step;
            });
        }

        // Indicador del ancla, por encima de todo
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("__autoscroll")));
        painter.circle_filled(anchor, 16.0, Color32::from_black_alpha(170));
        painter.circle_stroke(anchor, 16.0, Stroke::new(1.5f32, CYAN()));
        for dir in [-1.0f32, 1.0] {
            let base = anchor.y + dir * 4.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(anchor.x, base + dir * 6.0),
                    egui::pos2(anchor.x - 4.5, base),
                    egui::pos2(anchor.x + 4.5, base),
                ],
                CYAN(),
                Stroke::NONE,
            ));
        }
        ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
        ctx.request_repaint(); // movimiento continuo mientras esté activo
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = msg.into();
        self.toast_until = Some(Instant::now() + Duration::from_secs(4));
    }

    fn idx(&self, id: u64) -> Option<usize> {
        self.rows.iter().position(|r| r.id == id)
    }

    // -------------------- Alta de tareas --------------------

    #[allow(clippy::too_many_arguments)]
    fn add_url(&mut self, url: &str, author: &str, title: &str, page_url: &str, vid_id: &str, thumb: &str) {
        let trimmed = url.trim();
        // Canal seguro: forzar HTTPS en cualquier enlace http:// (TLS obligatorio)
        let upgraded;
        let url: &str = if let Some(rest) = trimmed.strip_prefix("http://") {
            upgraded = format!("https://{rest}");
            &upgraded
        } else {
            trimmed
        };
        // Canonicalización ANTES de deduplicar y de enrutar. Sin esto, el
        // formato moderno y el antiguo del mismo archivo de MEGA se encolarían
        // como dos filas distintas y se descargaría dos veces.
        let canonical;
        let url: &str = match mega::canonicalize(url) {
            Some(c) => {
                canonical = c;
                &canonical
            }
            None => url,
        };

        if url.is_empty() || self.rows.iter().any(|r| r.url == url) {
            return;
        }
        let engine = engine_for_url(url);

        // Autor para agrupar en subcarpeta. Si quien llama no lo aporta
        // (LinkGrabber, pegar enlaces, importar TXT/JSON, reintento) se deduce
        // de la URL. A propósito NO entra en el nombre del archivo: así los
        // enlaces pegados conservan exactamente el nombre de siempre y el
        // cambio se limita a dónde se guardan.
        let derived = if author.is_empty() { author_from_url(url) } else { String::new() };
        let folder_author: &str = if author.is_empty() { &derived } else { author };

        // Identificador del post: el que envía el capturador, el de la URL,
        // o un hash corto y estable de la propia URL (nunca un contador, que
        // producía nombres opacos tipo «100001.mp4»).
        let vid = if !vid_id.is_empty() {
            vid_id.to_string()
        } else if let Some(m) = Regex::new(r"/(?:video|note|photo)/(\d+)").unwrap().captures(url) {
            m[1].to_string()
        // Bilibili usa IDs alfanuméricos (BV1xx…): sin esto caía al hash opaco
        } else if let Some(m) = Regex::new(r"/(BV[0-9A-Za-z]{8,12})").unwrap().captures(url) {
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
            // Se resuelve al iniciar; el nombre real llega con cada archivo.
            Engine::FileHost => format!("{} ({})", stem, hosters::host_name(url)),
            Engine::Cyberdrop => format!("{stem} (cyberdrop-dl)"),
            // El nombre real viene cifrado en los atributos y solo se conoce
            // tras resolver el enlace; esto es solo la etiqueta provisional.
            Engine::Mega => format!("{stem} (MEGA)"),
        };

        self.push_row(url, page_url, folder_author, engine, filename, thumb, "");
    }

    /// Inserta una fila en la cola. `cookie` solo lo usan los enlaces directos
    /// resueltos por un hoster (GoFile). Centralizado para no repetir el struct.
    #[allow(clippy::too_many_arguments)]
    fn push_row(
        &mut self,
        url: &str,
        page_url: &str,
        author: &str,
        engine: Engine,
        filename: String,
        thumb: &str,
        cookie: &str,
    ) {
        self.next_id += 1;
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
            gal_current: String::new(),
            error_full: String::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            thumb_url: if thumb.starts_with("http") { thumb.to_string() } else { String::new() },
            dl_cookie: cookie.to_string(),
        });
    }

    fn add_plain_urls(&mut self, text: &str) -> usize {
        let mut n = 0;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("http") {
                self.add_url(line, "", "", "", "", "");
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
                            self.add_url(&url, &g("author"), &g("title"), &g("pageUrl"), &g("id"), &g("thumb"));
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

    /// Ajusta el límite de descargas simultáneas.
    ///
    /// NUNCA reemplaza el semáforo mientras haya tareas esperando turno. Antes
    /// se creaba uno nuevo y se soltaba el anterior: las tareas que aguardaban
    /// en el viejo recibían un error de adquisición al destruirse y morían en
    /// silencio, dejando sus filas en «Esperando» para siempre.
    ///
    /// Subir el límite se hace añadiendo permisos al semáforo existente, que es
    /// una operación segura y no rompe a nadie. Bajarlo requiere sustituirlo, y
    /// eso solo se permite con la cola parada.
    fn refresh_semaphore(&mut self) {
        let want = self.settings.concurrency;
        if self.sem_permits == want {
            return;
        }
        if want > self.sem_permits {
            self.sem.add_permits(want - self.sem_permits);
            self.sem_permits = want;
            return;
        }
        // Reducir: solo si no hay nada en marcha que pueda quedarse colgado
        if !self.rows.iter().any(|r| r.status.is_active()) {
            self.sem = Arc::new(Semaphore::new(want));
            self.sem_permits = want;
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
        // OJO: aquí había un std::fs::create_dir_all, es decir E/S SÍNCRONA en
        // el hilo de la interfaz. Con una fila suelta no se nota; al expandir
        // una carpeta de MEGA se llama 100+ veces seguidas y, con un antivirus
        // inspeccionando cada acceso, la ventana se queda «No responde».
        // La carpeta se crea ahora dentro de la tarea asíncrona.

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
            lang: self.settings.lang,
            cookie: row.dl_cookie.clone(),
        };
        let client = self.client.clone();
        let sem = self.sem.clone();
        let tx = self.tx.clone();
        let ytdlp = self.ytdlp_cmd.clone();
        let galdl = self.galdl_cmd.clone();
        let cyberdrop = self.cyberdrop_cmd.clone();
        self.rt.spawn(async move {
            let _ = tokio::fs::create_dir_all(&dir).await;
            download_task(client, spec, sem, tx, ytdlp, galdl, cyberdrop).await;
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

    fn drain_events(&mut self, ctx: &egui::Context) {
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
                    // Los magnet van al motor BitTorrent, no a la cola HTTP.
                    // Es la vía por la que llega un clic en un magnet cuando la
                    // app ya estaba abierta (la lanza forward_to_running_instance).
                    let (magnets, links): (Vec<_>, Vec<_>) = items
                        .into_iter()
                        .partition(|i| i.url.to_ascii_lowercase().starts_with("magnet:"));
                    for m in magnets {
                        self.view = View::Torrents;
                        self.add_torrent(m.url);
                    }
                    let items = links;

                    let before = self.rows.len();
                    for it in &items {
                        self.add_url(&it.url, &it.author, &it.title, &it.page_url, &it.id, &it.thumb);
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
                Ev::GalFiles(id, n, name) => {
                    if let Some(i) = self.idx(id) {
                        self.rows[i].gal_files = n;
                        self.rows[i].gal_current = name;
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
                Ev::Thumb(id, img) => {
                    self.thumbs_pending.remove(&id);
                    let tex = ctx.load_texture(
                        format!("thumb_{id}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.thumbs.insert(id, tex);
                }
                Ev::FileHostResolved(id, items) => {
                    // La fila del hoster se sustituye por una fila HTTP por
                    // archivo resuelto; se heredan autor y carpeta de la origen.
                    let (author, page_url) = match self.idx(id) {
                        Some(i) => (self.rows[i].author.clone(), self.rows[i].page_url.clone()),
                        None => continue,
                    };
                    self.rows.retain(|r| r.id != id);
                    let n = items.len();
                    let mut new_ids = Vec::new();
                    for it in items {
                        // Evitar duplicados si ya estaba en cola
                        if self.rows.iter().any(|r| r.url == it.url) {
                            continue;
                        }
                        let name = sanitize(&it.filename, 150);
                        self.push_row(&it.url, &page_url, &author, it.engine, name, "", &it.cookie);
                        new_ids.push(self.next_id);
                    }
                    // Arrancar de inmediato lo resuelto (el usuario ya dio a iniciar)
                    for id in new_ids {
                        if let Some(i) = self.idx(id) {
                            self.start_row(i);
                        }
                    }
                    self.toast(i18n::host_resolved(self.settings.lang, n));
                }
                Ev::Cyberdrop(cmd) => {
                    self.cyberdrop_cmd = cmd;
                    self.cyberdrop_installing = false;
                }
                Ev::CyberdropProgress(p) => {
                    self.cyberdrop_installing = true;
                    self.cyberdrop_progress = p;
                }
                Ev::CyberdropError(e) => {
                    self.cyberdrop_installing = false;
                    let msg = i18n::install_error(self.settings.lang, "cyberdrop-dl", &e);
                    self.toast(msg);
                }
                Ev::TorrentClientReady(c) => {
                    self.torrent_client = Some(c);
                }
                Ev::TorrentAdded(id, handle, name) => {
                    self.torrent_adding = false;
                    self.torrents.push(torrents::Handle { id, inner: handle, name });
                    self.toast(t(self.settings.lang, "torrent.added"));
                }
                Ev::TorrentError(e) => {
                    self.torrent_adding = false;
                    self.toast(i18n::torrent_error(self.settings.lang, &e));
                }
                Ev::GalleryResults(_, _, epoch) if epoch != self.gallery_epoch => {}
                Ev::GalleryError(_, epoch) if epoch != self.gallery_epoch => {}
                Ev::GalleryResults(items, page, _) => {
                    self.gallery_loading = false;
                    self.gallery_page = page;
                    if items.is_empty() && page > 1 {
                        self.toast(t(self.settings.lang, "gal.no_more"));
                    } else if page > 1 {
                        // «Cargar más» AÑADE. Reemplazar perdía lo ya marcado y
                        // además invalidaba las miniaturas ya descargadas.
                        self.gallery_items.extend(items);
                    } else {
                        // Una lista vacía en la primera página NO puede quedarse
                        // en silencio: el usuario ve la pantalla igual que antes
                        // y no sabe si falló, si está cargando o si no hay nada.
                        // En Instagram la causa casi siempre es la sesión.
                        if items.is_empty() {
                            self.toast(t(self.settings.lang, "gal.empty"));
                        }
                        self.gallery_items = items;
                        self.gallery_thumbs.clear();
                        self.gallery_pending.clear();
                        self.gallery_failed.clear();
                    }
                }
                Ev::ThumbFailed(idx, es_perfil) => {
                    if es_perfil {
                        self.profile_pending.remove(&idx);
                        self.profile_failed.insert(idx);
                    } else {
                        self.gallery_pending.remove(&idx);
                        self.gallery_failed.insert(idx);
                    }
                }
                Ev::ProfileThumb(idx, img) => {
                    self.profile_pending.remove(&idx);
                    let tex =
                        ctx.load_texture(format!("prof_{idx}"), img, egui::TextureOptions::LINEAR);
                    self.profile_thumbs.insert(idx, tex);
                }
                Ev::GalleryThumb(idx, img) => {
                    self.gallery_pending.remove(&idx);
                    let tex = ctx.load_texture(
                        format!("gal_{idx}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.gallery_thumbs.insert(idx, tex);
                }
                Ev::GalleryError(msg, _) => {
                    self.gallery_loading = false;
                    // El texto íntegro se queda en la vista; el toast solo avisa
                    self.gallery_error = msg.clone();
                    self.toast(msg.lines().next().unwrap_or("").to_string());
                }
                Ev::BooruResults(posts, epoch) if epoch != self.booru_epoch => {
                    // Respuesta de una búsqueda ya descartada: se ignora sin
                    // tocar el indicador de carga, que pertenece a la actual.
                    let _ = posts;
                }
                Ev::BooruError(_, epoch) if epoch != self.booru_epoch => {}
                Ev::BooruResults(posts, _) => {
                    self.booru_searching = false;
                    let n = posts.len();
                    self.booru_posts = posts;
                    // Las miniaturas de la búsqueda anterior ya no sirven
                    self.booru_thumbs.clear();
                    self.booru_pending.clear();
                    self.toast(i18n::booru_found(self.settings.lang, n));
                }
                Ev::BooruError(e, _) => {
                    self.booru_searching = false;
                    self.toast(i18n::booru_error(self.settings.lang, &e));
                }
                Ev::BooruThumb(id, img) => {
                    self.booru_pending.remove(&id);
                    let tex = ctx.load_texture(
                        format!("booru_{id}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.booru_thumbs.insert(id, tex);
                }
            }
        }
        if !clip_batch.is_empty() {
            let before = self.rows.len();
            for l in &clip_batch {
                self.add_url(l, "", "", "", "", "");
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
    lang: Lang,
    /// Cookie de descarga (GoFile); vacía si no aplica
    cookie: String,
}

#[allow(clippy::too_many_arguments)]
async fn download_task(
    client: reqwest::Client,
    spec: DlSpec,
    sem: Arc<Semaphore>,
    tx: UnboundedSender<Ev>,
    ytdlp: Option<String>,
    galdl: Option<String>,
    cyberdrop: Option<String>,
) {
    // Aquí había un `else { return }` mudo. Si el semáforo se cierra o se
    // reemplaza mientras hay tareas esperando turno, `acquire_owned` devuelve
    // Err y la tarea se moría sin decir nada: la fila se quedaba en «Esperando»
    // eternamente, sin error, sin progreso y sin forma de saber por qué.
    // Un fallo invisible es peor que uno ruidoso.
    let _permit = match sem.acquire_owned().await {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Ev::ErrorDetail(
                spec.id,
                format!("no se pudo reservar hueco de descarga: {e}"),
            ));
            let _ = tx.send(Ev::Status(
                spec.id,
                Status::Error("cola interrumpida; pulsa Reintentar".into()),
            ));
            return;
        }
    };
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
        Engine::FileHost => {
            run_filehost(&client, &spec, &tx).await;
            return;
        }
        Engine::Cyberdrop => {
            run_cyberdrop(&spec, &spec.url.clone(), &tx, cyberdrop.as_deref()).await;
            return;
        }
        Engine::Mega => {
            run_mega(&client, &spec, &tx).await;
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

/// Carpeta de donde salen los GIF de la pestaña de apoyo: `tips/` junto al
/// ejecutable. Deliberadamente NO se incrustan en el binario ni se publican en
/// el repositorio: así cada quien pone los suyos sin redistribuir obra ajena.
fn tips_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("tips")))
        .unwrap_or_else(|| PathBuf::from("tips"))
}

/// Elige un GIF al azar de `tips/` y decodifica sus fotogramas.
///
/// Sin dependencia de números aleatorios: basta con los nanosegundos del reloj
/// como semilla, que para elegir una imagen sobra.
fn load_random_tip_gif(ctx: &egui::Context) -> Vec<(egui::TextureHandle, f32)> {
    use image::AnimationDecoder;

    let dir = tips_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|e| e.eq_ignore_ascii_case("gif"))
                .unwrap_or(false)
        })
        .collect();
    if files.is_empty() {
        return Vec::new();
    }
    files.sort(); // orden estable antes de sortear

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    let path = &files[seed % files.len()];

    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let Ok(decoder) = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)) else {
        return Vec::new();
    };
    let Ok(frames) = decoder.into_frames().collect_frames() else { return Vec::new() };

    frames
        .into_iter()
        .enumerate()
        // Tope de fotogramas: un GIF largo no debe llenar la memoria de vídeo
        .take(120)
        .filter_map(|(i, f)| {
            let (num, den) = f.delay().numer_denom_ms();
            // Muchos GIF declaran 0 ms; los navegadores usan 100 ms en ese caso
            let ms = if den == 0 { 100.0 } else { num as f32 / den as f32 };
            let secs = if ms < 10.0 { 0.1 } else { ms / 1000.0 };
            let buf = f.into_buffer();
            let (w, h) = buf.dimensions();
            if w == 0 || h == 0 {
                return None;
            }
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                buf.as_raw(),
            );
            Some((
                ctx.load_texture(format!("tip_{i}"), img, egui::TextureOptions::NEAREST),
                secs,
            ))
        })
        .collect()
}

/// Limita cuántas miniaturas se piden a la vez, en toda la aplicación.
///
/// Cuatro simultáneas es el punto en que ningún CDN de booru protesta. Con las
/// 40 de golpe que se lanzaban antes, cada sitio reaccionaba distinto: Danbooru
/// devolvía una página de bloqueo en todas, yande.re dejaba pasar dos o tres,
/// y AIBooru o Safebooru lo aguantaban. Ese «depende del booru» era, en el
/// fondo, un problema de ritmo, no de compatibilidad.
fn thumb_gate() -> &'static Arc<Semaphore> {
    static G: std::sync::OnceLock<Arc<Semaphore>> = std::sync::OnceLock::new();
    G.get_or_init(|| Arc::new(Semaphore::new(4)))
}

/// Referer del sitio al que pertenece un CDN de booru. Varios lo comprueban
/// para evitar el hotlinking.
fn booru_referer(url: &str) -> &'static str {
    let u = url.to_ascii_lowercase();
    if u.contains("donmai.us") {
        "https://danbooru.donmai.us/"
    } else if u.contains("aibooru") {
        "https://aibooru.online/"
    } else if u.contains("yande.re") {
        "https://yande.re/"
    } else if u.contains("konachan") {
        "https://konachan.com/"
    } else if u.contains("e621") || u.contains("e926") {
        "https://e621.net/"
    } else if u.contains("safebooru") {
        "https://safebooru.org/"
    } else if u.contains("gelbooru") {
        "https://gelbooru.com/"
    } else {
        ""
    }
}

/// Escribe el archivo temporal de credenciales para gallery-dl.
///
/// En Unix se crea con permisos **0600** (solo el propietario lee y escribe)
/// ANTES de volcar el contenido, para que no exista ni un instante con permisos
/// abiertos. En Windows va en la carpeta de datos de la app, dentro del perfil
/// del usuario, que ya hereda una ACL restringida a ese usuario.
async fn write_booru_auth(json: &str) -> Option<PathBuf> {
    let dir = ytdlp_dir();
    tokio::fs::create_dir_all(&dir).await.ok()?;
    let path = dir.join("booru-auth.json");

    #[cfg(unix)]
    {
        // `mode()` lo aporta el propio tokio::fs::OpenOptions en Unix; el trait
        // de std sobraba y solo generaba un aviso en Linux y macOS.
        use tokio::io::AsyncWriteExt;
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .await
            .ok()?;
        f.write_all(json.as_bytes()).await.ok()?;
        f.flush().await.ok()?;
    }
    #[cfg(not(unix))]
    {
        tokio::fs::write(&path, json).await.ok()?;
    }
    Some(path)
}

/// Lanza `gallery-dl -j` para listar posts SIN descargarlos.
///
/// `--range` pagina: 40 resultados por página. Es el mismo binario que ya usa
/// la app para descargar galerías, así que no añade ninguna dependencia nueva.
/// Ejecuta un motor capturando su salida, CON LÍMITE DE TIEMPO.
///
/// `Command::output()` no tiene tope: si gallery-dl se queda esperando a un
/// booru que no responde, o reintentando internamente tras un bloqueo por
/// ritmo, la tarea no termina nunca. Como el evento de resultado nunca llega,
/// la interfaz se queda con el indicador girando para siempre y no hay forma
/// de recuperarse salvo reiniciar. Es el mismo patrón de fallo mudo que ya
/// apareció en MEGA: algo que no vuelve y nadie lo cuenta.
///
/// Al agotarse el plazo se mata el ÁRBOL de procesos, no solo el hijo:
/// gallery-dl es un empaquetado PyInstaller y deja un nieto de Python vivo si
/// se termina únicamente el lanzador.
async fn run_capture_timeout(
    mut cmd: tokio::process::Command,
    limite: Duration,
) -> std::io::Result<(std::process::ExitStatus, String, String)> {
    use tokio::io::AsyncReadExt;

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();

    // Las tuberías se drenan en paralelo: si se llenan, el hijo se bloquea
    // escribiendo y el tope de tiempo saltaría por un motivo equivocado.
    let t_out = tokio::spawn(async move {
        let mut s = String::new();
        if let Some(h) = so.as_mut() {
            let _ = h.read_to_string(&mut s).await;
        }
        s
    });
    let t_err = tokio::spawn(async move {
        let mut s = String::new();
        if let Some(h) = se.as_mut() {
            let _ = h.read_to_string(&mut s).await;
        }
        s
    });

    let fin = Instant::now() + limite;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if Instant::now() >= fin {
            kill_tree(&mut child).await;
            let _ = child.wait().await;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "el motor no respondió a tiempo",
            ));
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    };

    Ok((
        status,
        t_out.await.unwrap_or_default(),
        t_err.await.unwrap_or_default(),
    ))
}

/// Tope para una búsqueda en un booru.
///
/// Corto a propósito. Un booru sano contesta en 2-5 segundos; pasados 25 no
/// está pensando, está bloqueando o reintentando por dentro. Esperar 90 s
/// «por si acaso» solo consigue que el usuario crea que la aplicación se ha
/// colgado, que es exactamente el problema que este tope venía a resolver.
const BOORU_TIMEOUT: Duration = Duration::from_secs(25);
/// Tope para listar una galería. Mucho mayor porque Instagram se espacia
/// 6-12 s entre peticiones y una página de 30 puede tardar varios minutos.
const GALLERY_TIMEOUT: Duration = Duration::from_secs(300);

#[allow(clippy::too_many_arguments)]
async fn booru_search(
    program: String,
    url: String,
    page: u32,
    per_page: u32,
    auth_cfg: Option<String>,
    tx: UnboundedSender<Ev>,
    epoch: u64,
) {
    let first = (page.saturating_sub(1)) * per_page + 1;
    let last = first + per_page - 1;

    // Credenciales en archivo temporal, NO en la línea de comandos (ver
    // booru::auth_config). Se borra pase lo que pase, también si falla.
    let cfg_path = match &auth_cfg {
        Some(json) => write_booru_auth(json).await,
        None => None,
    };

    let mut cmd = tokio::process::Command::new(&program);
    utf8_env(&mut cmd);
    cmd.args(["-j", "--range", &format!("{first}-{last}"), "--no-download"]);
    if let Some(p) = &cfg_path {
        cmd.arg("-c").arg(p);
    }
    cmd.arg("--").arg(&url);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }

    let result = run_capture_timeout(cmd, BOORU_TIMEOUT).await;

    // Borrado inmediato: el archivo solo existe mientras dura la búsqueda
    if let Some(p) = &cfg_path {
        let _ = tokio::fs::remove_file(p).await;
    }

    match result {
        Ok((_st, stdout, stderr)) => {
            let stdout = stdout.as_str();
            if stdout.trim().is_empty() {
                let err = stderr.as_str();
                let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("sin resultados");
                let _ = tx.send(Ev::BooruError(last.chars().take(160).collect(), epoch));
                return;
            }
            match booru::parse(stdout) {
                Ok(posts) => {
                    let _ = tx.send(Ev::BooruResults(posts, epoch));
                }
                Err(e) => {
                    let _ = tx.send(Ev::BooruError(e, epoch));
                }
            }
        }
        Err(e) => {
            let _ = tx.send(Ev::BooruError(e.to_string(), epoch));
        }
    }
}

/// Descarga la miniatura de un post de booru para la rejilla.
async fn fetch_booru_thumb(client: reqwest::Client, id: u64, url: String, tx: UnboundedSender<Ev>) {
    const MAX: usize = 4 * 1024 * 1024;

    // Cola global de miniaturas. Sin esto se lanzaban 40 peticiones de golpe
    // al mismo CDN y varios boorus respondían con una página de desafío en vez
    // de la imagen: Danbooru bloqueaba todas, yande.re dejaba pasar unas pocas
    // y AIBooru lo toleraba. De ahí que «dependiera del booru».
    let _permit = thumb_gate().acquire().await.ok();

    // Referer del propio sitio: varios CDN de boorus lo comprueban
    let referer = booru_referer(&url);

    // Un reintento: los bloqueos por ritmo son transitorios y se van solos
    let mut bytes = None;
    for attempt in 0..2u32 {
        let mut req = client.get(&url);
        if !referer.is_empty() {
            req = req.header(reqwest::header::REFERER, referer);
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(b) = resp.bytes().await {
                    // Un CDN que responde HTML es una página de bloqueo, no
                    // una imagen: se descarta y se reintenta.
                    if !b.is_empty() && b.len() <= MAX && !b.starts_with(b"<") {
                        bytes = Some(b);
                        break;
                    }
                }
            }
            _ => {}
        }
        if attempt == 0 {
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    }
    let Some(bytes) = bytes else { return };

    let img = tokio::task::spawn_blocking(move || {
        let im = image::load_from_memory(&bytes).ok()?;
        let im = im.thumbnail(180, 180);
        let rgba = im.to_rgba8();
        let (w, h) = rgba.dimensions();
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            rgba.as_raw(),
        ))
    })
    .await
    .ok()
    .flatten();
    if let Some(img) = img {
        let _ = tx.send(Ev::BooruThumb(id, img));
    }
}

/// Descarga y decodifica una miniatura. Silenciosa ante cualquier fallo: una
/// portada caída jamás debe generar ruido; la fila simplemente queda sin imagen.
async fn fetch_thumb(client: reqwest::Client, id: u64, url: String, tx: UnboundedSender<Ev>) {
    const MAX_THUMB: usize = 6 * 1024 * 1024;

    // Mismo límite global que las miniaturas de booru: una cola de 300 posts
    // no debe lanzar 300 peticiones simultáneas al CDN.
    let _permit = thumb_gate().acquire().await.ok();

    let mut req = client.get(&url);
    // Referer del sitio de origen; si no es un dominio conocido, se prueba
    // con el del booru correspondiente.
    let referer = {
        let r = referer_for(&url);
        if r.is_empty() { booru_referer(&url) } else { r }
    };
    if !referer.is_empty() {
        req = req.header(reqwest::header::REFERER, referer);
    }
    let Ok(resp) = req.send().await else { return };
    if !resp.status().is_success() {
        return;
    }
    let Ok(bytes) = resp.bytes().await else { return };
    // Una respuesta HTML es una página de bloqueo, no una imagen
    if bytes.is_empty() || bytes.len() > MAX_THUMB || bytes.starts_with(b"<") {
        return;
    }

    // Decodificar y reescalar fuera del pool async (trabajo de CPU)
    let img = tokio::task::spawn_blocking(move || {
        let im = image::load_from_memory(&bytes).ok()?;
        // 96 px de lado mayor: nítido en la tabla y ligero en la GPU (~37 KB)
        let im = im.thumbnail(96, 96);
        let rgba = im.to_rgba8();
        let (w, h) = rgba.dimensions();
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            rgba.as_raw(),
        ))
    })
    .await
    .ok()
    .flatten();

    if let Some(img) = img {
        let _ = tx.send(Ev::Thumb(id, img));
    }
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
    // Cookie de descarga que exige el hoster (GoFile: accountToken=…)
    if !spec.cookie.is_empty() {
        req = req.header(reqwest::header::COOKIE, &spec.cookie);
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

/// Resultado de ejecutar un motor externo
struct ExecOutcome {
    ok: bool,
    stderr: String,
    /// El usuario pulsó Pausa y se mató el subproceso
    killed: bool,
}

/// Mata el proceso **y toda su descendencia**.
///
/// Imprescindible: yt-dlp y gallery-dl se distribuyen como binarios PyInstaller
/// «onefile». Lo que se lanza es un bootloader que se descomprime en `_MEIxxxx`
/// y arranca el intérprete de Python real **en un proceso hijo aparte**.
/// `TerminateProcess` sobre el bootloader deja vivo a ese nieto, que sigue
/// descargando huérfano — exactamente el síntoma de «pulso Pausa y no para».
///
/// En Windows se delega en `taskkill /T`, que recorre el árbol. En Unix basta
/// con la señal al hijo, que se propaga por el grupo de procesos.
async fn kill_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let mut k = tokio::process::Command::new("taskkill");
            k.args(["/F", "/T", "/PID", &pid.to_string()]);
            k.creation_flags(0x0800_0000); // sin ventana de consola
            let _ = k.status().await;
        }
    }
    // Unix: como el hijo lidera su propio grupo (process_group(0) al lanzar),
    // un PID negativo envía SIGKILL a TODO el grupo, alcanzando también al
    // intérprete de Python que PyInstaller ejecuta como proceso nieto. Sin
    // esto, matar solo al bootloader dejaba al nieto descargando huérfano.
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            // -9 (SIGKILL) a «-pid» = a todo el grupo. Más portable que -KILL.
            let _ = tokio::process::Command::new("kill")
                .args(["-9", &format!("-{pid}")])
                .status()
                .await;
        }
    }
    // Respaldo directo al proceso
    let _ = child.start_kill();
}

/// Espera a que termine el hijo, **vigilando la cancelación**.
///
/// Antes se hacía `child.wait().await` a secas: pulsar Pausa marcaba el flag
/// pero nadie se lo decía a los motores, que seguían descargando el perfil
/// entero. Ahora se sondea cada 150 ms y se mata el árbol de procesos.
///
/// Se sondea con `try_wait()` en vez de `tokio::select!` a propósito: select
/// necesitaría prestar `child` mutablemente en dos ramas a la vez.
async fn wait_or_kill(
    child: &mut tokio::process::Child,
    cancel: &Arc<AtomicBool>,
) -> std::io::Result<(std::process::ExitStatus, bool)> {
    let mut killed = false;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((status, killed));
        }
        if !killed && cancel.load(Ordering::Relaxed) {
            kill_tree(child).await;
            killed = true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Lanza yt-dlp leyendo su salida en streaming para reportar progreso real.
/// Se detiene de inmediato si el usuario pulsa Pausa.
async fn ytdlp_exec(
    program: &str,
    args: &[String],
    id: u64,
    tx: &UnboundedSender<Ev>,
    cancel: &Arc<AtomicBool>,
) -> std::io::Result<ExecOutcome> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut cmd = tokio::process::Command::new(program);
    utf8_env(&mut cmd);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    // Unix: el hijo lidera su propio grupo de procesos, para poder matar el
    // árbol entero (incluido el nieto de Python de PyInstaller) al pausar.
    #[cfg(unix)]
    {
        cmd.process_group(0);
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

    // stderr en su propia tarea: hay que drenar la tubería mientras esperamos,
    // o el hijo se bloquearía al llenarla.
    let err_task = tokio::spawn(async move {
        let mut buf = String::new();
        if let Some(e) = stderr {
            let mut lines = BufReader::new(e).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                buf.push_str(&l);
                buf.push('\n');
            }
        }
        buf
    });

    let (status, killed) = wait_or_kill(&mut child, cancel).await?;
    let _ = progress_task.await;
    let stderr = err_task.await.unwrap_or_default();
    Ok(ExecOutcome { ok: status.success() && !killed, stderr, killed })
}

/// ¿La URL pertenece a Bilibili? (incluye el acortador b23.tv)
fn is_bilibili(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("bilibili.com") || u.contains("b23.tv")
}

async fn run_ytdlp(spec: &DlSpec, url: &str, tx: &UnboundedSender<Ev>, program: Option<&str>) {
    let Some(program) = program else {
        let _ = tx.send(Ev::Status(
            spec.id,
            Status::Error("yt-dlp no instalado (ver Ajustes)".into()),
        ));
        return;
    };

    // Bilibili solo sirve DASH (vídeo y audio SIEMPRE separados): sin ffmpeg
    // no hay forma de obtener un archivo completo. Mejor un error claro ahora
    // que el críptico «Requested merging of multiple formats» de yt-dlp.
    if is_bilibili(url) && spec.ffmpeg.is_none() {
        let msg = "Bilibili necesita ffmpeg (Ajustes → instalar ffmpeg)";
        let _ = tx.send(Ev::ErrorDetail(spec.id, msg.into()));
        let _ = tx.send(Ev::Status(spec.id, Status::Error(msg.into())));
        return;
    }

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
    let fmt = format_selector(spec.ffmpeg.is_some());

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
        // Sin --no-warnings a propósito: los avisos de yt-dlp son justo lo que
        // explica por qué falla una descarga (runtime JS ausente, challenge no
        // resuelto, PO token, extracción de firma). Silenciarlos dejaba a la
        // app y al usuario a ciegas. El panel de error muestra solo la última
        // línea relevante; la salida íntegra queda en Ev::ErrorDetail.
    ];

    // Bilibili publica cada resolución en dos códecs (AVC y HEVC) y el AVC
    // suele llevar bastante más bitrate (p. ej. 4174k vs 2503k a 1080p).
    // Este orden de preferencia elige resolución > fps > bitrate, ignorando
    // la preferencia de códec por defecto: máxima calidad real.
    if is_bilibili(url) {
        base.push("-S".into());
        base.push("res,fps,hdr,tbr".into());
    }

    // Indicar a yt-dlp dónde está nuestro ffmpeg integrado
    if let Some(ff) = &spec.ffmpeg {
        if let Some(dir) = std::path::Path::new(ff).parent() {
            base.push("--ffmpeg-location".into());
            base.push(dir.to_string_lossy().into_owned());
        }
    }

    // Throttle global: evita que varios procesos golpeen la API a la vez
    throttle().await;

    // Política de cookies (ver needs_cookies_upfront). Antes se pasaban a TODA
    // descarga, y eso es lo que rompía YouTube: con cookies de cuenta yt-dlp
    // exige un PO Token que no tenemos y descarta todos los formatos.
    let has_cookies = !spec.extra_args.is_empty();
    let cookies_first = has_cookies && needs_cookies_upfront(url);

    // fin de opciones (`--`): la URL nunca se interpreta como flag
    let build = |with_cookies: bool| -> Vec<String> {
        let mut a = base.clone();
        if with_cookies {
            a.extend(spec.extra_args.iter().cloned());
        }
        a.push("--".into());
        a.push(url.to_string());
        a
    };

    let first = match ytdlp_exec(program, &build(cookies_first), spec.id, tx, &spec.cancel).await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("yt-dlp: {e}"))));
            return;
        }
    };

    // Pausa del usuario: no es un fallo, es una parada solicitada
    if first.killed {
        let _ = tx.send(Ev::Status(spec.id, Status::Paused));
        return;
    }
    if first.ok {
        let _ = tx.send(Ev::Status(spec.id, Status::Done));
        return;
    }

    let err = first.stderr;

    // Escalada: se fue sin cookies y el sitio pide autenticación de verdad
    // (login, privado, edad, solo miembros). Ahora sí tiene sentido añadirlas.
    let retry_with_cookies = !cookies_first && has_cookies && needs_auth_error(&err);

    // Camino inverso: se fueron con cookies y resultaron ilegibles
    // (App-Bound Encryption de Chrome 127+, DB bloqueada…): reintentar sin ellas.
    let retry_without_cookies = cookies_first && is_cookie_error(&err);

    if retry_with_cookies || retry_without_cookies {
        if retry_without_cookies {
            let _ = tx.send(Ev::CookieFallback);
            let _ = tx.send(Ev::DisableCookies);
        }
        match ytdlp_exec(program, &build(retry_with_cookies), spec.id, tx, &spec.cancel).await {
            Ok(r) if r.killed => {
                let _ = tx.send(Ev::Status(spec.id, Status::Paused));
            }
            Ok(r) if r.ok => {
                let _ = tx.send(Ev::Status(spec.id, Status::Done));
            }
            Ok(r) => report_ytdlp_error(spec.id, &err, r.stderr, tx),
            Err(e) => {
                let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("yt-dlp: {e}"))));
            }
        }
        return;
    }

    report_ytdlp_error(spec.id, "", err, tx);
}

/// Reporta un fallo de yt-dlp conservando la salida completa.
///
/// La etiqueta corta busca primero una línea `ERROR:`; antes se cogía la
/// última línea no vacía, que con los avisos ya visibles suele ser un
/// `WARNING:` irrelevante en lugar del error real.
///
/// `first` es la salida del primer intento cuando hubo dos: sin ella se
/// perdería la razón por la que se decidió reintentar.
fn report_ytdlp_error(id: u64, first: &str, last_out: String, tx: &UnboundedSender<Ev>) {
    let brief = last_out
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("ERROR:"))
        .or_else(|| last_out.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or("error yt-dlp");
    let short: String = brief.trim().chars().take(60).collect();

    let full = if first.is_empty() {
        last_out
    } else {
        format!("--- intento 1 ---\n{first}\n--- intento 2 ---\n{last_out}")
    };
    let _ = tx.send(Ev::ErrorDetail(id, full));
    let _ = tx.send(Ev::Status(id, Status::Error(short)));
}

/// Descarga un enlace público de MEGA con el motor nativo de `src/mega`.
///
/// Un enlace de ARCHIVO se descarga aquí. Un enlace de CARPETA se enumera y la
/// fila se expande en una por archivo, reutilizando el mismo mecanismo que ya
/// usan los hosters nativos: así cada archivo tiene su propio progreso, su
/// pausa y su error, en vez de una barra única y opaca.
async fn run_mega(client: &reqwest::Client, spec: &DlSpec, tx: &UnboundedSender<Ev>) {
    // Pausar mientras 100 filas están resolviendo debe notarse YA. Sin esta
    // comprobación, cada tarea encolada seguía haciendo su petición aunque el
    // usuario ya hubiera pulsado Pausa.
    if spec.cancel.load(Ordering::Relaxed) {
        let _ = tx.send(Ev::Status(spec.id, Status::Paused));
        return;
    }
    let _ = tx.send(Ev::Status(spec.id, Status::Resolving));

    let link = match mega::parse(&spec.url) {
        Ok(l) => l,
        Err(e) => {
            // Aquí no hay enlace parseado, así que no hay nada que redactar:
            // se informa sin incluir la URL, que podría llevar la clave.
            let _ = tx.send(Ev::ErrorDetail(spec.id, e.to_string()));
            let _ = tx.send(Ev::Status(spec.id, Status::Error(e.to_string())));
            return;
        }
    };

    /// Diagnóstico con el enlace YA REDACTADO.
    ///
    /// Saber qué enlace falló es justo lo que hace falta para dar soporte, y
    /// `redacted()` deja el identificador visible pero sustituye la clave por
    /// [REDACTED]. Volcar `spec.url` tal cual filtraría la clave al log.
    fn detail(link: &mega::MegaLink, e: &mega::MegaError) -> String {
        format!("{e}\n\nenlace: {}\nid: {}", link.redacted(), link.handle())
    }

    // ---------------- Carpeta pública: expandir en una fila por archivo -------
    //
    // OJO CON LA CONDICIÓN: solo se expande una carpeta SIN nodo. Un enlace
    // `/folder/H#K/file/NODE` también parsea como Folder, pero designa UN
    // archivo concreto, y es justo la forma en que se encola cada archivo al
    // expandir. Sin el `d.node.is_none()`, cada fila de archivo volvía a listar
    // la carpeta y se re-expandía a sí misma: la fila se borraba, se recreaba
    // con otro id, arrancaba y vuelta a empezar. Un bucle infinito que exploraba
    // sin descargar nunca nada.
    // Un único match sobre el enlace ya parseado. Antes se parseaba dos veces
    // y las ramas no cubrían el caso «carpeta con nodo», que es precisamente
    // como se encola cada archivo al expandir una carpeta.
    let (query_link, folder) = match &link {
        // ---- Carpeta SIN nodo: expandir en una fila por archivo y salir ----
        //
        // Solo aquí se expande. Un `/folder/H#K/file/NODE` también es Folder,
        // pero designa UN archivo; si entrase por esta rama volvería a listar
        // la carpeta y se re-expandiría a sí mismo en bucle infinito, sin
        // descargar nunca nada.
        mega::MegaLink::Folder(d) if d.node.is_none() => {
            match mega::folder::list_cached(client, d).await {
                Ok(entries) => {
                    let items: Vec<HostItem> = entries
                        .iter()
                        .filter(|e| !e.is_folder && e.size > 0)
                        .map(|e| HostItem {
                            url: format!(
                                "https://mega.nz/folder/{}#{}/file/{}",
                                d.handle, d.key_b64, e.handle
                            ),
                            filename: e
                                .relative_path
                                .to_string_lossy()
                                .replace(['/', '\\'], "_"),
                            cookie: String::new(),
                            engine: Engine::Mega,
                        })
                        .collect();
                    if items.is_empty() {
                        let msg = "MEGA: la carpeta no contiene archivos descargables";
                        let _ = tx.send(Ev::Status(spec.id, Status::Error(msg.into())));
                        return;
                    }
                    let _ = tx.send(Ev::FileHostResolved(spec.id, items));
                    return;
                }
                Err(e) => {
                    let _ = tx.send(Ev::ErrorDetail(spec.id, detail(&link, &e)));
                    let _ = tx.send(Ev::Status(spec.id, Status::Error(e.to_string())));
                    return;
                }
            }
        }

        // ---- Carpeta CON nodo: es UN archivo dentro de la carpeta ----
        //
        // Necesita dos cosas: el handle de la carpeta para la API, y su PROPIA
        // clave de 32 bytes. La de la carpeta son 16 y no sirve para descifrar
        // un archivo, así que se saca del listado (que está cacheado).
        mega::MegaLink::Folder(d) => {
            let node = d.node.clone().unwrap_or_default();
            match mega::folder::list_cached(client, d).await {
                Ok(entries) => match entries.iter().find(|e| e.handle == node) {
                    Some(e) => (
                        mega::MegaFileLink { handle: node, key_b64: e.key_b64() },
                        Some(d.handle.clone()),
                    ),
                    None => {
                        let err = mega::MegaError::NotFound;
                        let _ = tx.send(Ev::ErrorDetail(spec.id, detail(&link, &err)));
                        let _ = tx.send(Ev::Status(spec.id, Status::Error(err.to_string())));
                        return;
                    }
                },
                Err(e) => {
                    let _ = tx.send(Ev::ErrorDetail(spec.id, detail(&link, &e)));
                    let _ = tx.send(Ev::Status(spec.id, Status::Error(e.to_string())));
                    return;
                }
            }
        }

        // ---- Enlace de archivo suelto ----
        mega::MegaLink::File(f) => (f.clone(), None),
    };

    // A propósito NO se usa el throttle() global: está calibrado para yt-dlp y
    // gallery-dl raspando perfiles, y mantiene su candado durante 1,8 s. Con 107
    // archivos de una carpeta eso son más de tres minutos de espera serializada
    // en los que la aplicación parece colgada. El comando `g` de MEGA es barato,
    // el listado ya está cacheado, y el límite real lo imponen el semáforo de
    // concurrencia y el backoff ante los errores -3/-4.
    mega::gate().await;

    let info = match mega::resolve_file(client, &query_link, folder.as_deref()).await {
        Ok(i) => i,
        Err(e) => {
            let _ = tx.send(Ev::ErrorDetail(spec.id, detail(&link, &e)));
            let _ = tx.send(Ev::Status(spec.id, Status::Error(e.to_string())));
            return;
        }
    };

    if spec.cancel.load(Ordering::Relaxed) {
        let _ = tx.send(Ev::Status(spec.id, Status::Paused));
        return;
    }

    // El tamaño real llega con los metadatos: nunca se muestra un 0 % inventado
    let _ = tx.send(Ev::Size(spec.id, info.size));

    // El nombre viene cifrado en los atributos: se sanea antes de tocar el disco
    let dir = spec.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let final_path = dir.join(sanitize(&info.name, 150));
    let part_path = PathBuf::from(format!("{}.part", final_path.display()));

    let id = spec.id;
    let tx_p = tx.clone();
    let tx_f = tx.clone();
    let cb = mega::Callbacks {
        progress: &move |done, speed| {
            let _ = tx_p.send(Ev::Progress(id, done, speed));
        },
        phase: &move |ph| {
            let st = match ph {
                mega::MegaPhase::FetchingMetadata => Status::Resolving,
                mega::MegaPhase::Downloading => Status::Downloading,
                mega::MegaPhase::VerifyingIntegrity => Status::Verifying,
                mega::MegaPhase::Completed => Status::Done,
            };
            let _ = tx_f.send(Ev::Status(id, st));
        },
    };

    match mega::download_file(
        client,
        &query_link,
        folder.as_deref(),
        &info,
        &part_path,
        &final_path,
        &spec.cancel,
        &cb,
    )
    .await
    {
        Ok(_) => {
            let _ = tx.send(Ev::Status(spec.id, Status::Done));
        }
        Err(mega::MegaError::Cancelled) => {
            let _ = tx.send(Ev::Status(spec.id, Status::Paused));
        }
        Err(e) => {
            let _ = tx.send(Ev::ErrorDetail(spec.id, detail(&link, &e)));
            let _ = tx.send(Ev::Status(spec.id, Status::Error(e.to_string())));
        }
    }
}

/// Descarga y decodifica la previsualización de un elemento de galería.
///
/// Reutiliza la misma compuerta global que las miniaturas de booru: los CDN de
/// Instagram y Weibo responden con una página de bloqueo si les llegan treinta
/// peticiones a la vez. Y el Referer es obligatorio: son CDN anti-hotlink, y
/// `referer_for` ya sabe cuál toca para cada uno.
async fn fetch_gallery_thumb(
    client: reqwest::Client,
    idx: usize,
    url: String,
    tx: UnboundedSender<Ev>,
) {
    fetch_preview_thumb(client, idx, url, tx, false).await
}

/// Descarga la portada de una entrada del análisis de perfil.
async fn fetch_profile_thumb(
    client: reqwest::Client,
    idx: usize,
    url: String,
    tx: UnboundedSender<Ev>,
) {
    fetch_preview_thumb(client, idx, url, tx, true).await
}

/// Cuerpo común de ambas: sólo cambia el evento con el que se responde.
async fn fetch_preview_thumb(
    client: reqwest::Client,
    idx: usize,
    url: String,
    tx: UnboundedSender<Ev>,
    es_perfil: bool,
) {
    const MAX: usize = 8 * 1024 * 1024;
    let _permit = thumb_gate().acquire().await.ok();

    // Todo camino que no acaba en imagen tiene que avisar. Si no, la celda se
    // queda cargando eternamente y encima nunca se reintenta.
    let fallo = || {
        let _ = tx.send(Ev::ThumbFailed(idx, es_perfil));
    };

    let referer = referer_for(&url);
    let mut req = client.get(&url);
    if !referer.is_empty() {
        req = req.header(reqwest::header::REFERER, referer);
    }
    let Ok(resp) = req.send().await else {
        fallo();
        return;
    };
    if !resp.status().is_success() {
        fallo();
        return;
    }
    let Ok(bytes) = resp.bytes().await else {
        fallo();
        return;
    };
    // Una respuesta HTML es una página de bloqueo, no una imagen
    if bytes.is_empty() || bytes.len() > MAX || bytes.starts_with(b"<") {
        fallo();
        return;
    }

    // Decodificar y reescalar fuera del pool async: es trabajo de CPU y
    // bloquearía a las tareas de descarga.
    let img = tokio::task::spawn_blocking(move || {
        let im = image::load_from_memory(&bytes).ok()?;
        let im = im.thumbnail(320, 320);
        let rgba = im.to_rgba8();
        let (w, h) = rgba.dimensions();
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            rgba.as_raw(),
        ))
    })
    .await
    .ok()
    .flatten();

    match img {
        Some(img) => {
            let _ = tx.send(if es_perfil {
                Ev::ProfileThumb(idx, img)
            } else {
                Ev::GalleryThumb(idx, img)
            });
        }
        None => fallo(),
    }
}

/// Lista una galería con gallery-dl SIN descargar nada.
///
/// El ritmo entre peticiones lo marca `galdl_pacing`: Instagram corta sesiones
/// con facilidad, así que explorar debe ser tan pausado como descargar.
async fn browse_gallery(
    program: String,
    url: String,
    page: u32,
    per_page: u32,
    cookies: Vec<String>,
    tx: UnboundedSender<Ev>,
    epoch: u64,
) {
    // Weibo se explora por el muro de fotos, no por el feed: ver weibo_album_url
    let url = weibo_album_url(&url).unwrap_or(url);
    browse_gallery_hop(program, url, page, per_page, cookies, tx, 0, epoch).await
}

/// Tope de redirecciones de extractor. Instagram necesita una
/// (`/usuario/` → `/usuario/posts/`); más de tres es una cadena rota.
const MAX_GALLERY_HOPS: u32 = 3;

#[allow(clippy::too_many_arguments)]
async fn browse_gallery_hop(
    program: String,
    url: String,
    page: u32,
    per_page: u32,
    cookies: Vec<String>,
    tx: UnboundedSender<Ev>,
    hops: u32,
    epoch: u64,
) {
    let first = (page - 1) * per_page + 1;
    let last = page * per_page;

    let mut args = gallery::list_args(&url, first, last);
    // Se clonan porque el salto de extractor vuelve a necesitarlas
    let cookies_next = cookies.clone();
    // Las cookies van ANTES del `--`, que cierra la lista de opciones
    let sep = args.iter().position(|a| a == "--").unwrap_or(args.len());
    for (i, c) in cookies.into_iter().enumerate() {
        args.insert(sep + i, c);
    }
    let sep = args.iter().position(|a| a == "--").unwrap_or(0);
    args.insert(sep, galdl_pacing(&url).to_string());
    args.insert(sep, "--sleep-request".into());

    let mut cmd = tokio::process::Command::new(&program);
    utf8_env(&mut cmd);
    cmd.args(&args);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }

    let (estado, stdout, stderr) = match run_capture_timeout(cmd, GALLERY_TIMEOUT).await {
        Ok(o) => o,
        Err(e) => {
            let _ = tx.send(Ev::GalleryError(format!("gallery-dl: {e}"), epoch));
            return;
        }
    };
    let stdout = stdout.as_str();
    let stderr = stderr.as_str();

    // Comando ejecutado, con la ruta del cookies.txt REDACTADA. Sin esto no hay
    // forma de saber si el problema es lo que se pide o lo que se responde.
    let cmdline: String = {
        let mut v: Vec<String> = Vec::new();
        let mut saltar = false;
        for a in &args {
            if saltar {
                v.push("[REDACTADO]".into());
                saltar = false;
                continue;
            }
            if a == "--cookies" {
                saltar = true;
            }
            v.push(a.clone());
        }
        format!("gallery-dl {}", v.join(" "))
    };

    // El stderr se conserva SIEMPRE, no solo cuando no hay JSON. gallery-dl
    // puede devolver `[]` perfectamente válido y explicar el porqué por stderr
    // («login required», «no results», un aviso del extractor). Descartarlo
    // dejaba al usuario con un «no devolvió nada» sin causa, que es justo el
    // fallo mudo que ya costó caro en el motor de MEGA.
    let motivo = |extra: &str| -> String {
        let err: String = stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let err = if err.is_empty() { "(stderr vacío)".to_string() } else { err };
        // Los primeros caracteres del JSON tal cual: si es `[]`, el extractor
        // corrió bien y sencillamente no obtuvo nada de Instagram, que es un
        // diagnóstico completamente distinto a un fallo de argumentos.
        let crudo: String = stdout.trim().chars().take(300).collect();
        // Pista específica del sitio. El texto genérico habla de Instagram, que
        // en un 403 de Weibo despista más de lo que ayuda.
        let es_weibo = host_of(&url)
            .map(|h| host_matches(&h, "weibo.com") || host_matches(&h, "weibo.cn"))
            .unwrap_or(false);
        let extra = if es_weibo && (err.contains("403") || stdout.contains("403")) {
            format!(
                "{extra}\n\nWeibo responde 403 cuando la petición no lleva sesión: \
gallery-dl necesita las cookies SUB y SUBP del dominio .weibo.com. \
Comprueba que tienes la sesión abierta en el MISMO navegador que has elegido \
en Ajustes, o exporta un cookies.txt de Weibo."
            )
        } else {
            extra.to_string()
        };
        format!(
            "{extra}\n\n$ {cmdline}\n\ncódigo de salida: {}\n\nstderr:\n{err}\n\nstdout ({} bytes):\n{crudo}",
            estado.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
            stdout.len(),
        )
        .chars()
        .take(1500)
        .collect()
    };

    // Se decide primero y se actúa después: el reintento por el muro de fotos
    // de Weibo tiene que poder salir tanto de un listado vacío como de un
    // error, y con la estructura anterior habría que duplicarlo en cada rama.
    enum Accion {
        Listado(Vec<gallery::GalleryItem>),
        Salto(String),
        Fallo(String),
    }

    let accion = if stdout.trim().is_empty() {
        Accion::Fallo(motivo("gallery-dl no devolvió JSON."))
    } else {
        match gallery::parse_listing(stdout) {
            Ok(l) if !l.items.is_empty() => Accion::Listado(l.items),
            // gallery-dl delega en otro extractor: se sigue la pista.
            Ok(l) if !l.queued.is_empty() => Accion::Salto(l.queued[0].clone()),
            Ok(_) => Accion::Fallo(motivo("gallery-dl no encontró archivos en este perfil.")),
            Err(e) => Accion::Fallo(motivo(&e)),
        }
    };

    // Si el muro de fotos no da nada (cuenta sin álbum poblado), se prueba el
    // feed antes de rendirse, aun sabiendo que su resolución es menor.
    let accion = match accion {
        Accion::Fallo(m) => match weibo_feed_url(&url) {
            Some(alt) => Accion::Salto(alt),
            None => Accion::Fallo(m),
        },
        otro => otro,
    };

    match accion {
        Accion::Listado(items) => {
            let _ = tx.send(Ev::GalleryResults(items, page, epoch));
        }
        Accion::Salto(siguiente) => {
            // Tope de saltos para que una cadena circular no cuelgue la app
            if hops < MAX_GALLERY_HOPS {
                Box::pin(browse_gallery_hop(
                    program, siguiente, page, per_page, cookies_next, tx, hops + 1, epoch,
                ))
                .await;
            } else {
                let _ = tx.send(Ev::GalleryError(motivo(
                    "gallery-dl encadena demasiadas redirecciones de extractor.",
                ), epoch));
            }
        }
        Accion::Fallo(m) => {
            let _ = tx.send(Ev::GalleryError(m, epoch));
        }
    }
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
/// Se detiene de inmediato si el usuario pulsa Pausa.
async fn galdl_exec(
    program: &str,
    args: &[String],
    id: u64,
    tx: &UnboundedSender<Ev>,
    cancel: &Arc<AtomicBool>,
) -> std::io::Result<ExecOutcome> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut cmd = tokio::process::Command::new(program);
    utf8_env(&mut cmd);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    // Unix: grupo de procesos propio (ver ytdlp_exec) para matar el árbol.
    #[cfg(unix)]
    {
        cmd.process_group(0);
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
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            n += 1;
            // gallery-dl imprime la ruta completa de cada archivo escrito;
            // nos quedamos con el nombre para mostrar en qué va.
            let name = l
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(l)
                .chars()
                .take(48)
                .collect::<String>();
            let _ = tx_files.send(Ev::GalFiles(id, n, name));
        }
    });

    // stderr en su propia tarea: hay que drenar la tubería mientras esperamos
    let err_task = tokio::spawn(async move {
        let mut buf = String::new();
        if let Some(e) = stderr {
            let mut lines = BufReader::new(e).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                buf.push_str(&l);
                buf.push('\n');
            }
        }
        buf
    });

    let (status, killed) = wait_or_kill(&mut child, cancel).await?;
    let _ = counter.await;
    let stderr = err_task.await.unwrap_or_default();
    Ok(ExecOutcome { ok: status.success() && !killed, stderr, killed })
}

/// Historial de archivos ya descargados por gallery-dl.
///
/// Sin esto, «Reintentar» un perfil que se cortó en el archivo 54 vuelve a
/// empezar por el 1 y se estrella siempre en el mismo punto: nunca terminaría.
/// Con el historial, cada reintento avanza.
fn galdl_archive_path() -> PathBuf {
    ytdlp_dir().join("descargados.sqlite3")
}

/// Espaciado entre peticiones según el sitio. Instagram es el más agresivo
/// cortando sesiones, así que se le da mucho más aire que al resto.
fn galdl_pacing(url: &str) -> &'static str {
    if url.to_ascii_lowercase().contains("instagram.com") {
        "6.0-12.0" // gallery-dl acepta rangos: elige un valor aleatorio
    } else {
        "1.5"
    }
}

/// Argumentos comunes de gallery-dl para una descarga
fn galdl_base_args(dir: &std::path::Path, url: &str) -> Vec<String> {
    vec![
        "-D".into(),
        dir.to_string_lossy().into_owned(),
        "--sleep-request".into(),
        galdl_pacing(url).into(),
        "--download-archive".into(),
        galdl_archive_path().to_string_lossy().into_owned(),
    ]
}

/// Traduce errores crípticos de gallery-dl a algo accionable.
/// Devuelve `None` si no hay nada mejor que decir que el mensaje original.
fn galdl_hint(lang: Lang, url: &str, err: &str) -> Option<&'static str> {
    let u = url.to_ascii_lowercase();
    let e = err.to_ascii_lowercase();
    if u.contains("instagram.com")
        && (e.contains("401") || e.contains("unauthorized") || e.contains("login"))
    {
        return Some(i18n::t(lang, "err.instagram_login"));
    }
    None
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

    /// Publica el fallo con una explicación útil cuando la hay
    fn report(spec: &DlSpec, url: &str, err: String, tx: &UnboundedSender<Ev>) {
        let last = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("error gallery-dl");
        match galdl_hint(spec.lang, url, &err) {
            Some(hint) => {
                // El consejo va primero en el tooltip; el error crudo, debajo
                let _ = tx.send(Ev::ErrorDetail(spec.id, format!("{hint}\n\n{err}")));
                let _ = tx.send(Ev::Status(spec.id, Status::Error(hint.chars().take(60).collect())));
            }
            None => {
                let _ = tx.send(Ev::ErrorDetail(spec.id, err.clone()));
                let _ = tx.send(Ev::Status(spec.id, Status::Error(last.chars().take(60).collect())));
            }
        }
    }

    // gallery-dl imprime una ruta por archivo descargado: contamos líneas
    // para dar progreso en vivo.
    let mut args = galdl_base_args(&dir, url);
    args.extend(spec.extra_args.iter().cloned());
    args.push("--".into());
    args.push(url.to_string());

    match galdl_exec(program, &args, spec.id, tx, &spec.cancel).await {
        // Pausa del usuario: parada solicitada, no un fallo. Lo ya descargado
        // queda anotado en el historial, así que Reanudar continúa desde ahí.
        Ok(r) if r.killed => {
            let _ = tx.send(Ev::Status(spec.id, Status::Paused));
        }
        Ok(r) if r.ok => {
            let _ = tx.send(Ev::Status(spec.id, Status::Done));
        }
        Ok(r) => {
            // Cookies ilegibles (App-Bound Encryption, BD bloqueada…): se
            // reintenta sin ellas, que basta para los perfiles públicos.
            if is_cookie_error(&r.stderr) && !spec.extra_args.is_empty() {
                let _ = tx.send(Ev::CookieFallback);
                let _ = tx.send(Ev::DisableCookies);
                let mut retry = galdl_base_args(&dir, url);
                retry.push("--".into());
                retry.push(url.to_string());
                match galdl_exec(program, &retry, spec.id, tx, &spec.cancel).await {
                    Ok(r2) if r2.killed => {
                        let _ = tx.send(Ev::Status(spec.id, Status::Paused));
                    }
                    Ok(r2) if r2.ok => {
                        let _ = tx.send(Ev::Status(spec.id, Status::Done));
                    }
                    Ok(r2) => report(spec, url, r2.stderr, tx),
                    Err(e) => {
                        let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("gallery-dl: {e}"))));
                    }
                }
            } else {
                report(spec, url, r.stderr, tx);
            }
        }
        Err(e) => {
            let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("gallery-dl: {e}"))));
        }
    }
}

/// Motor de hosters nativos (Pixeldrain, GoFile, MediaFire): resuelve la URL
/// de página a enlaces directos y los emite para que se conviertan en filas
/// HTTP normales, que ya saben reanudar por Range.
async fn run_filehost(client: &reqwest::Client, spec: &DlSpec, tx: &UnboundedSender<Ev>) {
    let _ = tx.send(Ev::Status(spec.id, Status::Resolving));
    match hosters::resolve(client, &spec.url).await {
        Ok(items) => {
            let mapped: Vec<HostItem> = items
                .into_iter()
                .map(|r| HostItem {
                    engine: Engine::Http,
                    url: r.url,
                    filename: r.filename,
                    cookie: r.cookie.unwrap_or_default(),
                })
                .collect();
            // La expansión (crear filas hijas y arrancarlas) la hace el hilo de
            // UI, que es quien puede tocar la cola.
            let _ = tx.send(Ev::FileHostResolved(spec.id, mapped));
        }
        Err(e) => {
            let msg = format!("{}: {e}", hosters::host_name(&spec.url));
            let _ = tx.send(Ev::ErrorDetail(spec.id, msg.clone()));
            let _ = tx.send(Ev::Status(spec.id, Status::Error(msg.chars().take(60).collect())));
        }
    }
}

/// Motor opcional cyberdrop-dl para hosters difíciles (Bunkr, Cyberdrop…).
/// Requiere que el usuario lo haya instalado desde Ajustes (necesita Python).
async fn run_cyberdrop(spec: &DlSpec, url: &str, tx: &UnboundedSender<Ev>, program: Option<&str>) {
    let Some(program) = program else {
        let msg = i18n::t(spec.lang, "err.need_cyberdrop");
        let _ = tx.send(Ev::ErrorDetail(spec.id, msg.into()));
        let _ = tx.send(Ev::Status(spec.id, Status::Error(msg.chars().take(60).collect())));
        return;
    };
    let _ = tx.send(Ev::Status(spec.id, Status::Resolving));
    throttle().await;

    let dir = spec.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    // cyberdrop-dl baja a su propia estructura; se le fija la carpeta de destino.
    let args: Vec<String> = vec![
        "--download-folder".into(),
        dir.to_string_lossy().into_owned(),
        "--disable-progress-bar".into(),
        "--no-ui".into(),
        url.to_string(),
    ];

    // Reutiliza el ejecutor de gallery-dl: misma mecánica (cuenta líneas de
    // stdout como archivos, mata el árbol al pausar).
    match galdl_exec(program, &args, spec.id, tx, &spec.cancel).await {
        Ok(r) if r.killed => {
            let _ = tx.send(Ev::Status(spec.id, Status::Paused));
        }
        Ok(r) if r.ok => {
            let _ = tx.send(Ev::Status(spec.id, Status::Done));
        }
        Ok(r) => {
            let last = r.stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error cyberdrop-dl");
            let _ = tx.send(Ev::ErrorDetail(spec.id, r.stderr.clone()));
            let _ = tx.send(Ev::Status(spec.id, Status::Error(last.chars().take(60).collect())));
        }
        Err(e) => {
            let _ = tx.send(Ev::Status(spec.id, Status::Error(format!("cyberdrop-dl: {e}"))));
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
    utf8_env(&mut cmd);
    // --sleep-requests: Bilibili responde 412 (bloqueo temporal) si las
    // peticiones de paginación van demasiado seguidas; espaciarlas lo evita.
    cmd.args([
        "--flat-playlist", "-J", "--no-warnings", "--playlist-end", "2000",
        "--sleep-requests", "1", "--retries", "3", "--retry-sleep", "exp=2:30",
    ]);
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
                        // Portada: yt-dlp la da como `thumbnail` suelto o como
                        // lista `thumbnails[]`. Sin esto la cola salía sin
                        // miniaturas aunque el análisis fuera perfecto.
                        let thumb = {
                            let direct = g("thumbnail");
                            if !direct.is_empty() {
                                direct
                            } else {
                                e.get("thumbnails")
                                    .and_then(|t| t.as_array())
                                    .and_then(|arr| arr.last())
                                    .and_then(|t| t.get("url"))
                                    .and_then(|u| u.as_str())
                                    .unwrap_or("")
                                    .to_string()
                            }
                        };
                        entries.push(ProfileEntry {
                            selected: true,
                            id: g("id"),
                            title,
                            url: u,
                            is_image,
                            thumb,
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

/// Detecta cyberdrop-dl en el PATH. A diferencia de yt-dlp/gallery-dl, no se
/// distribuye como binario suelto: se instala con `uv tool install` y queda
/// accesible por PATH (o en ~/.local/bin).
fn spawn_cyberdrop_check(tx: UnboundedSender<Ev>) {
    std::thread::spawn(move || {
        for cand in cyberdrop_candidates() {
            let mut cmd = std::process::Command::new(&cand);
            cmd.arg("--version");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
                let _ = tx.send(Ev::Cyberdrop(Some(cand)));
                return;
            }
        }
        let _ = tx.send(Ev::Cyberdrop(None));
    });
}

/// Rutas candidatas del ejecutable de cyberdrop-dl
fn cyberdrop_candidates() -> Vec<String> {
    let mut v = vec!["cyberdrop-dl".to_string()];
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let bin = PathBuf::from(home).join(".local").join("bin");
        let exe = bin.join(if cfg!(windows) { "cyberdrop-dl.exe" } else { "cyberdrop-dl" });
        v.push(exe.to_string_lossy().into_owned());
    }
    v
}

/// Instala cyberdrop-dl mediante `uv` (que a su vez trae un Python gestionado).
/// Es la vía oficial del proyecto; no hay binario autónomo. Todo el proceso se
/// hace en segundo plano y se informa del resultado.
async fn install_cyberdrop(tx: UnboundedSender<Ev>) {
    let _ = tx.send(Ev::CyberdropProgress(0.1));

    // 1) Asegurar uv. Si no está, instalarlo con el script oficial.
    let uv = match ensure_uv(&tx).await {
        Ok(u) => u,
        Err(e) => {
            let _ = tx.send(Ev::CyberdropError(e));
            return;
        }
    };
    let _ = tx.send(Ev::CyberdropProgress(0.5));

    // 2) uv tool install cyberdrop-dl-patched (el paquete mantenido en PyPI)
    let mut cmd = tokio::process::Command::new(&uv);
    cmd.args([
        "tool", "install", "--managed-python", "-p", "<3.14",
        "--force", "cyberdrop-dl-patched>=10.0,<11.0",
    ]);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.output().await {
        Ok(o) if o.status.success() => {
            let _ = tx.send(Ev::CyberdropProgress(0.95));
            // Verificar que quedó accesible
            for cand in cyberdrop_candidates() {
                if verify_tool(&PathBuf::from(&cand)).await {
                    let _ = tx.send(Ev::Cyberdrop(Some(cand)));
                    return;
                }
            }
            let _ = tx.send(Ev::CyberdropError(
                "instalado, pero no se encontró en el PATH; reinicia la app".into(),
            ));
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("uv falló");
            let _ = tx.send(Ev::CyberdropError(last.chars().take(200).collect()));
        }
        Err(e) => {
            let _ = tx.send(Ev::CyberdropError(e.to_string()));
        }
    }
}

/// Devuelve la ruta a `uv`, instalándolo con el script oficial si falta.
async fn ensure_uv(tx: &UnboundedSender<Ev>) -> Result<String, String> {
    // ¿Ya está?
    for cand in uv_candidates() {
        let mut c = tokio::process::Command::new(&cand);
        c.arg("--version");
        #[cfg(windows)]
        {
            c.creation_flags(0x0800_0000);
        }
        if matches!(c.output().await, Ok(o) if o.status.success()) {
            return Ok(cand);
        }
    }

    let _ = tx.send(Ev::CyberdropProgress(0.25));

    // Instalar uv con el script oficial de astral.sh
    #[cfg(windows)]
    let install = {
        let mut c = tokio::process::Command::new("powershell");
        c.args([
            "-NoProfile", "-ExecutionPolicy", "ByPass", "-c",
            "irm https://astral.sh/uv/install.ps1 | iex",
        ]);
        c.creation_flags(0x0800_0000);
        c.status().await
    };
    #[cfg(not(windows))]
    let install = {
        tokio::process::Command::new("sh")
            .args(["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"])
            .status()
            .await
    };

    match install {
        Ok(s) if s.success() => {
            for cand in uv_candidates() {
                let mut c = tokio::process::Command::new(&cand);
                c.arg("--version");
                #[cfg(windows)]
                {
                    c.creation_flags(0x0800_0000);
                }
                if matches!(c.output().await, Ok(o) if o.status.success()) {
                    return Ok(cand);
                }
            }
            Err("uv se instaló pero no se encuentra; reinicia la app".into())
        }
        Ok(_) => Err("no se pudo instalar uv (gestor de Python)".into()),
        Err(e) => Err(format!("no se pudo instalar uv: {e}")),
    }
}

fn uv_candidates() -> Vec<String> {
    let mut v = vec!["uv".to_string()];
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let bin = PathBuf::from(home).join(".local").join("bin");
        v.push(bin.join(if cfg!(windows) { "uv.exe" } else { "uv" }).to_string_lossy().into_owned());
    }
    v
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

/// Carga una imagen de disco y la sube como textura de fondo.
///
/// Se reescala a 1920 px de ancho como máximo: un wallpaper 4K ocuparía ~33 MB
/// de memoria de vídeo sin ninguna ganancia visual al ir difuminado y a baja
/// opacidad detrás de la interfaz.
fn load_bg_source(path: &str) -> Option<image::DynamicImage> {
    let img = image::open(path).ok()?;
    Some(if img.width() > 1920 {
        img.resize(1920, u32::MAX, image::imageops::FilterType::Triangle)
    } else {
        img
    })
}

/// Genera la textura de fondo aplicando desenfoque gaussiano si procede.
///
/// Truco de rendimiento: un desenfoque fuerte **destruye el detalle fino**, así
/// que no tiene sentido calcularlo a 1920 px (sería lentísimo, O(n) por píxel).
/// Se reduce antes a 720 px y la GPU lo reescala con filtrado lineal: mismo
/// resultado visual, una fracción del coste.
fn make_bg_texture(
    ctx: &egui::Context,
    src: &image::DynamicImage,
    blur: f32,
) -> Option<egui::TextureHandle> {
    let rgba = if blur > 0.1 {
        let small = src
            .resize(720, u32::MAX, image::imageops::FilterType::Triangle)
            .to_rgba8();
        image::imageops::blur(&small, blur)
    } else {
        src.to_rgba8()
    };
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
    Some(ctx.load_texture("bg_image", color, egui::TextureOptions::LINEAR))
}

/// Pinta la imagen de fondo cubriendo el rectángulo dado, **sin deformarla**.
///
/// Emula el `background-size: cover` de CSS: se recorta por el eje que sobra y
/// se centra, así una foto apaisada en una ventana alta no sale estirada.
fn paint_bg_image(ui: &egui::Ui, tex: &egui::TextureHandle, rect: egui::Rect, opacity: f32) {
    if opacity <= 0.001 || rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let ts = tex.size_vec2();
    let img_ar = ts.x / ts.y.max(1.0);
    let rect_ar = rect.width() / rect.height().max(1.0);

    // UV recortado y centrado sobre el eje sobrante
    let uv = if img_ar > rect_ar {
        let frac = rect_ar / img_ar; // porción del ancho que se usa
        egui::Rect::from_min_max(
            egui::pos2((1.0 - frac) * 0.5, 0.0),
            egui::pos2((1.0 + frac) * 0.5, 1.0),
        )
    } else {
        let frac = img_ar / rect_ar; // porción del alto que se usa
        egui::Rect::from_min_max(
            egui::pos2(0.0, (1.0 - frac) * 0.5),
            egui::pos2(1.0, (1.0 + frac) * 0.5),
        )
    };

    // El tint blanco con alfa actúa como opacidad global de la imagen
    let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0) as u8);
    ui.painter().image(tex.id(), rect, uv, tint);
}

/// Halos difuminados de fondo, al estilo de un desenfoque gaussiano.
///
/// egui no tiene desenfoque, así que se imita apilando círculos concéntricos
/// con muy poca opacidad cada uno: al acumularse dan una caída suave del
/// centro al borde, indistinguible de un degradado radial difuminado.
fn paint_bg_glow(ctx: &egui::Context, theme: Theme) {
    if !theme.has_glow() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::background());
    let r = ctx.screen_rect();
    let accent = ACCENT();
    let cyan = CYAN();

    // (posición relativa, radio relativo al ancho, color)
    let blobs = [
        (egui::pos2(r.left() + r.width() * 0.18, r.top() + r.height() * 0.12), 0.42f32, accent),
        (egui::pos2(r.right() - r.width() * 0.08, r.bottom() - r.height() * 0.10), 0.38, cyan),
        (egui::pos2(r.center().x, r.bottom() + r.height() * 0.06), 0.30, accent),
    ];

    const STEPS: usize = 26;
    for (pos, rel, color) in blobs {
        let radius = r.width() * rel;
        for i in 0..STEPS {
            let t = i as f32 / STEPS as f32;
            // Capas de fuera hacia dentro; la opacidad se acumula en el centro
            painter.circle_filled(pos, radius * (1.0 - t), color.gamma_multiply(0.014));
        }
    }
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
    v.override_text_color = Some(TEXT());
    v.panel_fill = BG();
    v.window_fill = PANEL();
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0f32,CARD_HOVER());
    v.extreme_bg_color = CARD(); // fondo de TextEdit / ProgressBar
    v.faint_bg_color = Color32::from_rgb(25, 28, 36); // striped
    v.selection.bg_fill = ACCENT().gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0f32,ACCENT());
    v.slider_trailing_fill = true;
    v.hyperlink_color = CYAN();

    let r = Rounding::same(8.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.noninteractive.bg_fill = CARD();
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0f32,TEXT());
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32,Color32::from_rgb(40, 45, 58));

    v.widgets.inactive.rounding = r;
    v.widgets.inactive.weak_bg_fill = CARD(); // relleno de botones
    v.widgets.inactive.bg_fill = CARD();
    v.widgets.inactive.fg_stroke = Stroke::new(1.0f32,TEXT());
    v.widgets.inactive.bg_stroke = Stroke::NONE;

    v.widgets.hovered.rounding = r;
    v.widgets.hovered.weak_bg_fill = CARD_HOVER();
    v.widgets.hovered.bg_fill = CARD_HOVER();
    v.widgets.hovered.fg_stroke = Stroke::new(1.2f32,Color32::WHITE);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0f32,ACCENT().gamma_multiply(0.6));
    v.widgets.hovered.expansion = 1.6; // el botón "crece" al pasar el ratón

    v.widgets.active.rounding = r;
    v.widgets.active.weak_bg_fill = ACCENT().gamma_multiply(0.5);
    v.widgets.active.bg_fill = ACCENT().gamma_multiply(0.5);
    v.widgets.active.fg_stroke = Stroke::new(1.2f32,Color32::WHITE);

    v.widgets.open.rounding = r;
    v.widgets.open.weak_bg_fill = CARD_HOVER();
    v.widgets.open.bg_fill = CARD_HOVER();

    style.visuals = v;
    ctx.set_style(style);
}

// ============================= Componentes UI =============================

fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD())
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::symmetric(16.0, 12.0))
}

fn stat_card(ui: &mut egui::Ui, value: String, label: &str, color: Color32) {
    card_frame().show(ui, |ui| {
        ui.set_min_width(120.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(value).size(22.0).strong().color(color));
            ui.label(RichText::new(label).size(11.5).color(MUTED()));
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
        Stroke::new(1.0f32, ACCENT().gamma_multiply(0.55 * t)),
    );
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let resp = ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(ACCENT())
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
        RichText::new(text).color(MUTED())
    };
    let btn = egui::Button::new(rt)
        .fill(if selected { ACCENT().gamma_multiply(0.25) } else { Color32::TRANSPARENT })
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
        self.drain_events(ctx);
        // Halos difuminados de fondo (solo en los temas que los usan)
        paint_bg_glow(ctx, self.settings.theme);

        // Cargar la imagen de fondo cuando cambie la ruta elegida
        if self.bg_loaded_from != self.settings.bg_image {
            self.bg_loaded_from = self.settings.bg_image.clone();
            self.bg_source = if self.settings.bg_image.trim().is_empty() {
                None
            } else {
                load_bg_source(&self.settings.bg_image)
            };
            self.bg_dirty = true;
        }
        // Regenerar la textura solo cuando hace falta (cambio de imagen o de
        // desenfoque al soltar el deslizador), nunca en cada frame.
        if self.bg_dirty {
            self.bg_dirty = false;
            self.bg_texture = self
                .bg_source
                .as_ref()
                .and_then(|s| make_bg_texture(ctx, s, self.settings.bg_blur));
        }
        // Antes de pintar: el delta inyectado debe llegar al área de scroll
        self.handle_autoscroll(ctx);

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
                    .fill(PANEL())
                    .inner_margin(Margin::symmetric(16.0, 18.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::Frame::none()
                        .fill(ACCENT())
                        .rounding(Rounding::same(10.0))
                        .inner_margin(Margin::symmetric(9.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("⬇").size(18.0).color(Color32::WHITE));
                        });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("Todo").size(16.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new("Downloader").size(16.0).strong().color(CYAN()));
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
                if nav_item(ui, self.view == View::Booru, "🖼", t(lang, "nav.booru"), self.booru_posts.len()) {
                    self.view = View::Booru;
                }
                if nav_item(ui, self.view == View::Torrents, "🌀", t(lang, "nav.torrents"), self.torrents.len()) {
                    self.view = View::Torrents;
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
                // Justo debajo de Ajustes, con el corazón para que destaque
                if nav_item(ui, self.view == View::Support, "❤", t(lang, "nav.tip"), 0) {
                    if self.view != View::Support {
                        self.tip_reload = true; // otro GIF en cada visita
                    }
                    self.view = View::Support;
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("By Eric V. Gramunt").size(11.5).color(MUTED()));
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(11.0).color(MUTED()));
                    ui.add_space(6.0);
                    match self.ytdlp_ok {
                        Some(true) => ui.label(RichText::new(t(lang, "side.ytdlp_active")).size(11.5).color(GREEN())),
                        Some(false) => ui
                            .label(RichText::new(t(lang, "side.ytdlp_missing")).size(11.5).color(AMBER()))
                            .on_hover_text(t(lang, "side.ytdlp_tip")),
                        None => ui.label(RichText::new("● yt-dlp…").size(11.5).color(MUTED())),
                    };
                    if self.galdl_cmd.is_some() {
                        ui.label(RichText::new(t(lang, "side.galdl_active")).size(11.5).color(GREEN()));
                    } else {
                        ui.label(RichText::new(t(lang, "side.galdl_missing")).size(11.5).color(AMBER()))
                            .on_hover_text(t(lang, "side.galdl_tip"));
                    }
                    if self.ffmpeg_cmd.is_some() {
                        ui.label(RichText::new(t(lang, "side.ffmpeg_active")).size(11.5).color(GREEN()));
                    } else {
                        ui.label(RichText::new(t(lang, "side.ffmpeg_missing")).size(11.5).color(AMBER()))
                            .on_hover_text(t(lang, "side.ffmpeg_tip"));
                    }
                    // MEGA no necesita binario auxiliar: va compilado dentro.
                    // Siempre activo, y se deja claro que es sin cuenta.
                    ui.label(RichText::new(t(lang, "side.mega_active")).size(11.5).color(GREEN()))
                        .on_hover_text(t(lang, "set.mega"));
                    // cyberdrop-dl es opcional: solo se anuncia si está presente
                    if self.cyberdrop_cmd.is_some() {
                        ui.label(RichText::new(t(lang, "side.cyberdrop_active")).size(11.5).color(GREEN()));
                    }
                    if self.settings.clipboard_watch {
                        ui.label(RichText::new(t(lang, "side.grabber_active")).size(11.5).color(CYAN()));
                    }
                    // Estado real de las cookies: la app las desactiva sola si
                    // resultan ilegibles, y sin este aviso no había forma de
                    // saber que se estaba descargando sin sesión.
                    if !cookie_args(&self.settings).is_empty() {
                        ui.label(RichText::new(t(lang, "side.cookies_on")).size(11.5).color(GREEN()));
                    } else {
                        ui.label(RichText::new(t(lang, "side.cookies_off")).size(11.5).color(MUTED()));
                    }
                });
            });

        // ---------------- Panel central ----------------
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG())
                    .inner_margin(Margin::symmetric(22.0, 18.0)),
            )
            .show(ctx, |ui| {
                // Fondo personalizado: se pinta lo primero, así queda DETRÁS de
                // todo el contenido. Solo aquí; la barra lateral sigue sólida
                // para que el menú se lea siempre.
                if let Some(tex) = &self.bg_texture {
                    let full = ui.max_rect().expand2(egui::vec2(22.0, 18.0)); // compensa el margen
                    paint_bg_image(ui, tex, full, self.settings.bg_opacity);
                }

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
                    View::Booru => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .id_source("booru_scroll")
                            .show(ui, |ui| self.booru_ui(ui));
                    }
                    View::Support => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .id_source("support_scroll")
                            .show(ui, |ui| self.support_ui(ui));
                    }
                    View::Torrents => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.torrents_ui(ui));
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
                                    .fill(CARD_HOVER())
                                    .rounding(Rounding::same(10.0))
                                    .inner_margin(Margin::symmetric(16.0, 9.0))
                                    .stroke(Stroke::new(1.0f32,ACCENT().gamma_multiply(0.5)))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(&self.toast).color(TEXT()));
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
                    ui.label(RichText::new(t(lang, "add.hint")).color(MUTED()));
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
            stat_card(ui, format!("{n_active}"), t(lg, "stat.active"), CYAN());
            stat_card(ui, format!("{n_done}"), t(lg, "stat.completed"), GREEN());
            stat_card(ui, format!("{n_failed}"), t(lg, "stat.errors"), if n_failed > 0 { RED() } else { MUTED() });
            stat_card(
                ui,
                if global_speed > 0.0 { format!("{}/s", fmt_size(global_speed)) } else { "—".into() },
                t(lg, "stat.speed"),
                ACCENT(),
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
                View::Settings | View::Profile | View::Capture | View::Torrents | View::Booru | View::Support => {}
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
                View::Settings | View::Profile | View::Capture | View::Torrents | View::Booru | View::Support => false,
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
                ui.label(RichText::new(msg).size(15.0).color(MUTED()));
            });
            return;
        }

        // Tabla
        let mut actions: Vec<(usize, RowAction)> = Vec::new();

        // Miniaturas: se sacan los campos como locales para poder mutar el set
        // de pendientes dentro del closure de la tabla sin pelear con el borrow
        // checker (self queda prestado inmutablemente por rows/thumbs).
        let mut thumbs_pending = std::mem::take(&mut self.thumbs_pending);
        let thumb_client = self.client.clone();
        let thumb_tx = self.tx.clone();
        let rt_handle = self.rt.handle().clone();
        // Margen derecho reducido: deja hueco a la barra de scroll para que no
        // se coma la última columna (los botones de acción) al estrechar la ventana.
        egui::Frame::none()
            .fill(CARD())
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin {
                left: 16.0,
                right: 6.0,
                top: 12.0,
                bottom: 12.0,
            })
            .show(ui, |ui| {
                TableBuilder::new(ui)
                    .striped(true)
                    // La tabla gestiona su propio scroll. Antes iba envuelta en
                    // un ScrollArea externo: dos áreas de scroll anidadas hacían
                    // que la fila superior se cortara por la mitad.
                    .auto_shrink([false, false])
                    // egui_extras limita la tabla a 800 px de alto por defecto;
                    // en pantalla completa eso dejaba medio panel vacío.
                    .max_scroll_height(f32::INFINITY)
                    .min_scrolled_height(0.0)
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
                                ui.label(RichText::new(txt).size(11.0).color(MUTED()).strong());
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(46.0, visible.len(), |mut row| {
                            let i = visible[row.index()];
                            let r = &self.rows[i];
                            row.col(|ui| {
                                // Miniatura de la portada, si el origen la proporcionó.
                                // La descarga se lanza de forma perezosa: solo para las
                                // filas que llegan a pintarse (la tabla es virtual).
                                if let Some(tex) = self.thumbs.get(&r.id) {
                                    let ts = tex.size_vec2();
                                    let h = 38.0f32;
                                    let w = (ts.x / ts.y.max(1.0) * h).clamp(21.0, 68.0);
                                    ui.add(
                                        egui::Image::new(egui::load::SizedTexture::new(
                                            tex.id(),
                                            egui::vec2(w, h),
                                        ))
                                        .rounding(Rounding::same(5.0)),
                                    );
                                } else if !r.thumb_url.is_empty()
                                    && !thumbs_pending.contains(&r.id)
                                    && self.thumbs.len() + thumbs_pending.len() < 512
                                {
                                    thumbs_pending.insert(r.id);
                                    rt_handle.spawn(fetch_thumb(
                                        thumb_client.clone(),
                                        r.id,
                                        r.thumb_url.clone(),
                                        thumb_tx.clone(),
                                    ));
                                }
                                ui.label(RichText::new(&r.filename).color(TEXT()))
                                    .on_hover_text(&r.url);
                            });
                            row.col(|ui| {
                                // gallery-dl no da bytes totales: mostramos archivos descargados
                                let txt = if r.engine == Engine::GalleryDl && r.gal_files > 0 {
                                    i18n::files_done(lang, r.gal_files)
                                } else if r.size > 0 {
                                    fmt_size(r.size as f64)
                                } else {
                                    "—".into()
                                };
                                ui.label(RichText::new(txt).color(MUTED()));
                            });
                            row.col(|ui| {
                                // Las galerías no conocen el total de archivos hasta
                                // terminar (saberlo exigiría una pasada previa que
                                // duplicaría las peticiones y dispararía el rate-limit).
                                // Así que en vez de un 0% falso se usa una barra
                                // animada indeterminada con el recuento en vivo.
                                let gallery_running = r.engine == Engine::GalleryDl
                                    && r.status != Status::Done
                                    && !matches!(r.status, Status::Error(_));
                                if gallery_running {
                                    let label = if r.gal_files > 0 {
                                        i18n::files_done(lang, r.gal_files)
                                    } else {
                                        t(lang, "gal.analyzing").to_string()
                                    };
                                    ui.add(
                                        egui::ProgressBar::new(0.999)
                                            .fill(ACCENT().gamma_multiply(0.55))
                                            .animate(r.status.is_active())
                                            .text(RichText::new(label).size(11.0)),
                                    )
                                    .on_hover_text(if r.gal_current.is_empty() {
                                        t(lang, "gal.analyzing").to_string()
                                    } else {
                                        format!("{}\n{}", t(lang, "gal.current"), r.gal_current)
                                    });
                                    return;
                                }
                                let frac = if r.size > 0 {
                                    r.downloaded as f32 / r.size as f32
                                } else if r.status == Status::Done {
                                    1.0
                                } else {
                                    0.0
                                };
                                ui.add(
                                    egui::ProgressBar::new(frac)
                                        .fill(if r.status == Status::Done { GREEN() } else { ACCENT() })
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
                                    .color(CYAN()),
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
                                    Status::Downloading
                                    | Status::Waiting
                                    | Status::Resolving
                                    | Status::Verifying => {
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
                                if !r.status.is_active()
                                    && ui
                                        .small_button("🗑")
                                        .on_hover_text(t(lang, "tip.remove"))
                                        .clicked()
                                {
                                    actions.push((i, RowAction::Remove));
                                }
                            });
                        });
                    });
        });

        // Devolver el set de pendientes (se sacó como local para el closure)
        self.thumbs_pending = thumbs_pending;

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
            let id = self.rows[i].id;
            self.thumbs.remove(&id); // liberar la textura de la GPU
            self.rows.remove(i);
        }
    }

    // ---------------- Vista de perfil ----------------

    fn profile_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "profile.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(4.0);
        ui.label(RichText::new(t(lang, "profile.subtitle")).color(MUTED()));
        ui.add_space(14.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            ui.label(RichText::new(t(lang, "profile.url_label")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [420.0, 30.0],
                    egui::TextEdit::singleline(&mut self.profile_url)
                        .hint_text("https://www.tiktok.com/@usuario  ·  space.bilibili.com/UID  ·  weibo.com/u/…"),
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
                ui.label(RichText::new(t(lang, "profile.want")).color(MUTED()));
                ui.checkbox(&mut self.profile_want_videos, t(lang, "profile.videos"));
                ui.checkbox(&mut self.profile_want_images, t(lang, "profile.images"));
            });
            ui.checkbox(
                &mut self.settings.use_browser_cookies,
                t(lang, "profile.cookies_inline"),
            );
            if is_douyin_profile(&self.profile_url) {
                ui.label(RichText::new(t(lang, "profile.douyin_note")).size(11.5).color(RED()));
            } else if is_gallery_site(&self.profile_url) {
                ui.label(RichText::new(t(lang, "profile.gallery_note")).size(11.5).color(AMBER()));
            }
            ui.add_space(8.0);
            if self.profile_analyzing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(t(lang, "profile.analyzing")).color(CYAN()));
                });
            } else if primary_button(ui, t(lang, "btn.analyze")).clicked() {
                let url = self.profile_url.trim().to_string();
                if url.is_empty() || !url.starts_with("http") {
                    self.toast(t(lang, "profile.need_url"));
                } else if is_douyin_profile(&url) {
                    self.toast(t(lang, "profile.douyin_unsupported"));
                } else if gallery::is_browsable(&host_of(&url).unwrap_or_default()) {
                    // Instagram y Weibo: en vez de tragarse el perfil entero,
                    // se LISTA primero y el usuario elige. gallery-dl con
                    // `-j --no-download` da los metadatos sin bajar un byte.
                    if let Some(prog) = self.galdl_cmd.clone() {
                        let url = normalize_profile_url(&url);
                        // Simétrico: explorar una galería limpia el análisis
                        self.profile_entries.clear();
                        self.profile_thumbs.clear();
                        self.profile_pending.clear();
                        self.profile_failed.clear();

                        self.gallery_url = url.clone();
                        self.gallery_items.clear();
                        self.gallery_error.clear();
                        self.gallery_loading = true;
                        self.gallery_page = 1;
                        let tx = self.tx.clone();
                        let cookies = cookie_args(&self.settings);
                        self.gallery_epoch += 1;
                        let ep = self.gallery_epoch;
                        self.rt.spawn(browse_gallery(prog, url, 1, GALLERY_PER_PAGE, cookies, tx, ep));
                    } else {
                        self.toast(t(lang, "profile.need_galdl"));
                    }
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
                        self.add_url(&url, &author, "", &url, "", "");
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
                    // Son dos flujos distintos en la misma vista: dejar la
                    // rejilla de una búsqueda anterior de Instagram mientras se
                    // analiza un perfil de TikTok confunde y además mezcla dos
                    // conjuntos de casillas en pantalla.
                    self.gallery_items.clear();
                    self.gallery_error.clear();
                    self.gallery_url.clear();
                    self.gallery_thumbs.clear();
                    self.gallery_pending.clear();
                        self.gallery_failed.clear();

                    self.profile_analyzing = true;
                    self.profile_entries.clear();
                    self.profile_thumbs.clear();
                    self.profile_pending.clear();
                        self.profile_failed.clear();
                    let args = cookie_args(&self.settings);
                    let tx = self.tx.clone();
                    self.rt.spawn(analyze_profile(prog, url, args, tx));
                } else {
                    self.toast(t(lang, "profile.need_ytdlp"));
                }
            }
        });

        // ---------------- Explorador de galerías (Instagram, Weibo) ----------
        if self.gallery_loading {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(t(lang, "gal.listing")).color(CYAN()));
            });
        }

        if !self.gallery_loading && self.gallery_items.is_empty() && !self.gallery_url.is_empty() {
            ui.add_space(12.0);
            card_frame().show(ui, |ui| {
                ui.label(RichText::new(t(lang, "gal.empty")).size(12.0).color(AMBER()));
                if !self.gallery_error.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(t(lang, "gal.reason")).size(11.0).color(MUTED()));
                    // Seleccionable a propósito: el texto de gallery-dl es lo
                    // que hace falta copiar para diagnosticar.
                    ui.add(
                        egui::TextEdit::multiline(&mut self.gallery_error.as_str())
                            .desired_width(f32::INFINITY)
                            .desired_rows(12)
                            .font(egui::TextStyle::Monospace),
                    );
                }
            });
        }

        if !self.gallery_items.is_empty() {
            ui.add_space(12.0);

            let visibles: Vec<usize> = self
                .gallery_items
                .iter()
                .enumerate()
                .filter(|(_, it)| {
                    if it.is_video { self.gallery_want_videos } else { self.gallery_want_images }
                })
                .map(|(i, _)| i)
                .collect();
            let marcados = visibles.iter().filter(|&&i| self.gallery_items[i].selected).count();

            card_frame().show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.gallery_want_images, t(lang, "gal.images"));
                    ui.checkbox(&mut self.gallery_want_videos, t(lang, "gal.videos"));
                    ui.separator();
                    if soft_button(ui, t(lang, "gal.select_all")).clicked() {
                        for &i in &visibles {
                            self.gallery_items[i].selected = true;
                        }
                    }
                    if soft_button(ui, t(lang, "gal.select_none")).clicked() {
                        for it in self.gallery_items.iter_mut() {
                            it.selected = false;
                        }
                    }
                    // Lo filtrado no puede quedarse marcado a escondidas: si no
                    // se ve, no se encola.
                    for (i, it) in self.gallery_items.iter_mut().enumerate() {
                        if it.selected && !visibles.contains(&i) {
                            it.selected = false;
                        }
                    }
                    ui.separator();
                    let ocultos = self.gallery_items.len() - visibles.len();
                    ui.label(
                        RichText::new(format!("{} / {}", marcados, visibles.len()))
                            .size(12.0)
                            .color(MUTED()),
                    );
                    if ocultos > 0 {
                        // Sin esto, filtrar parece «se ha perdido la mitad»
                        ui.label(
                            RichText::new(i18n::hidden_by_filter(lang, ocultos))
                                .size(11.0)
                                .color(AMBER()),
                        );
                    }
                });

                ui.add_space(6.0);

                // Rejilla con previsualización.
                //
                // OJO CON EL ANCHO: `horizontal_wrapped` decide dónde envolver
                // usando `available_width()`. Dentro de un ScrollArea ese ancho
                // no está acotado, así que sin fijarlo la rejilla colocaba las
                // 60 celdas en UNA fila infinita que se salía de la ventana y
                // no dejaba nada que desplazar hacia abajo.
                const CELDA: f32 = 150.0;
                const ALTO_CELDA: f32 = 250.0;
                let ancho_util = ui.available_width();

                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                    ui.set_max_width(ancho_util);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        for &i in &visibles {
                            let (thumb_url, is_video, marcado, resumen, pos, carrusel) = {
                                let it = &self.gallery_items[i];
                                (
                                    it.thumb_url.clone(),
                                    it.is_video,
                                    it.selected,
                                    it.summary(),
                                    it.position(),
                                    it.is_carousel(),
                                )
                            };

                            // Pedir la miniatura una sola vez por elemento
                            if !thumb_url.is_empty()
                                && !self.gallery_thumbs.contains_key(&i)
                                && !self.gallery_pending.contains(&i)
                                && !self.gallery_failed.contains(&i)
                            {
                                self.gallery_pending.insert(i);
                                self.rt.spawn(fetch_gallery_thumb(
                                    self.client.clone(),
                                    i,
                                    thumb_url.clone(),
                                    self.tx.clone(),
                                ));
                            }

                            // Cada celda va en su propio ámbito de id. Sin esto
                            // las respuestas de egui colisionan entre celdas y
                            // los clics se pierden.
                            let mut sel = marcado;
                            let descripcion = self.gallery_items[i].description.clone();

                            ui.allocate_ui(egui::vec2(CELDA, ALTO_CELDA), |ui| {
                            ui.set_max_width(CELDA);
                            ui.push_id(i, |ui| {
                                let marco = egui::Frame::none()
                                    .fill(if marcado { ACCENT().linear_multiply(0.30) } else { CARD() })
                                    .rounding(6.0)
                                    .inner_margin(4.0);
                                marco.show(ui, |ui| {
                                    ui.set_min_width(CELDA - 10.0);
                                    ui.set_max_width(CELDA - 10.0);
                                    ui.vertical_centered(|ui| {
                                        let alto = CELDA - 10.0;

                                        // ImageButton y checkbox son widgets REALES.
                                        // Antes se pintaba la celda y se intentaba
                                        // capturar el clic con `interact()` sobre la
                                        // respuesta de `allocate_ui`, que no registra
                                        // la interacción de forma fiable: se podía
                                        // marcar todo o nada, pero no una sola.
                                        let clic = if let Some(tex) = self.gallery_thumbs.get(&i) {
                                            let t = tex.size_vec2();
                                            let k = (alto / t.x.max(1.0)).min(alto / t.y.max(1.0));
                                            ui.add(
                                                egui::ImageButton::new((tex.id(), t * k))
                                                    .frame(false),
                                            )
                                            .clicked()
                                        } else {
                                            let etiqueta = if thumb_url.is_empty()
                                                || self.gallery_failed.contains(&i)
                                            {
                                                if is_video { "▶" } else { "—" }
                                            } else {
                                                "…"
                                            };
                                            ui.add_sized(
                                                [alto, alto],
                                                egui::Button::new(
                                                    RichText::new(etiqueta).size(22.0).color(MUTED()),
                                                )
                                                .frame(false),
                                            )
                                            .clicked()
                                        };
                                        if clic {
                                            sel = !sel;
                                        }

                                        ui.horizontal(|ui| {
                                            ui.checkbox(&mut sel, "");
                                            if is_video {
                                                ui.label(
                                                    RichText::new("VÍDEO").size(9.5).color(AMBER()),
                                                );
                                            }
                                            if carrusel && !pos.is_empty() {
                                                ui.label(
                                                    RichText::new(format!("❏ {pos}"))
                                                        .size(9.5)
                                                        .color(CYAN()),
                                                );
                                            }
                                        });
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(resumen).size(9.0).color(MUTED()),
                                            )
                                            .wrap(),
                                        );
                                    });
                                })
                                .response
                            })
                            .response
                            .on_hover_text(if descripcion.is_empty() {
                                String::from("sin descripción")
                            } else {
                                descripcion
                            });
                            });

                            if sel != marcado {
                                self.gallery_items[i].selected = sel;
                            }
                        }
                    });
                });

                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if primary_button(ui, t(lang, "gal.queue_selected")).clicked() {
                        let autor = self
                            .gallery_items
                            .iter()
                            .map(|i| i.author.clone())
                            .find(|a| !a.is_empty())
                            .unwrap_or_else(|| {
                                author_from_url(&self.gallery_url)
                            });
                        let elegidos: Vec<gallery::GalleryItem> = self
                            .gallery_items
                            .iter()
                            .filter(|i| i.selected)
                            .cloned()
                            .collect();
                        let n = elegidos.len();
                        for it in elegidos {
                            // El enlace de la publicación va como page_url: si la
                            // URL de CDN caduca, el motor puede reintentar por ahí.
                            self.add_url(&it.url, &autor, &it.filename, &it.post_url, &it.post_id, "");
                        }
                        self.toast(i18n::added_links(lang, n));
                        if n > 0 {
                            self.view = View::Downloads;
                        }
                    }
                    if !self.gallery_loading && soft_button(ui, t(lang, "gal.more")).clicked() {
                        if let Some(prog) = self.galdl_cmd.clone() {
                            let next = self.gallery_page + 1;
                            self.gallery_loading = true;
                            let tx = self.tx.clone();
                            let cookies = cookie_args(&self.settings);
                            let url = self.gallery_url.clone();
                            self.gallery_epoch += 1;
                            let ep = self.gallery_epoch;
                            self.rt.spawn(browse_gallery(
                                prog, url, next, GALLERY_PER_PAGE, cookies, tx, ep,
                            ));
                        }
                    }
                    ui.label(
                        RichText::new(t(lang, "gal.expiry_note"))
                            .size(10.5)
                            .color(AMBER()),
                    );
                });
            });
        }

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

            // Acciones de borrado, diferidas: modificar la lista mientras se
            // pinta rompería los índices de `visible`.
            let mut clear_all = false;
            let mut remove_selected = false;
            let mut drop_one: Option<usize> = None;

            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width().min(720.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(i18n::posts_summary(lang, self.profile_entries.len(), n_vid, n_img))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Vaciar la lista entera (distinto de «Ninguno», que solo desmarca)
                        if soft_button(ui, t(lang, "btn.clear_list")).clicked() {
                            clear_all = true;
                        }
                        // Quitar de la lista lo que esté marcado
                        if selected > 0 && soft_button(ui, t(lang, "btn.remove_selected")).clicked() {
                            remove_selected = true;
                        }
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
                // Rejilla con portada. yt-dlp ya devuelve una miniatura por
                // entrada (`ProfileEntry::thumb`), así que la vista de TikTok
                // puede enseñar lo mismo que la de Instagram en vez de una
                // lista de títulos con la que se elige a ciegas.
                const CELDA_P: f32 = 132.0;
                const ALTO_P: f32 = 210.0;
                let ancho_p = ui.available_width();

                egui::ScrollArea::vertical()
                    .max_height(430.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                    ui.set_max_width(ancho_p);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        for &i in &visible {
                            let (thumb, es_img, marcada, titulo, id_post, url) = {
                                let e = &self.profile_entries[i];
                                (
                                    e.thumb.clone(),
                                    e.is_image,
                                    e.selected,
                                    e.title.chars().take(48).collect::<String>(),
                                    e.id.clone(),
                                    e.url.clone(),
                                )
                            };

                            if thumb.starts_with("http")
                                && !self.profile_thumbs.contains_key(&i)
                                && !self.profile_pending.contains(&i)
                                && !self.profile_failed.contains(&i)
                            {
                                self.profile_pending.insert(i);
                                self.rt.spawn(fetch_profile_thumb(
                                    self.client.clone(),
                                    i,
                                    thumb.clone(),
                                    self.tx.clone(),
                                ));
                            }

                            let mut sel = marcada;
                            let mut borrar = false;

                            ui.allocate_ui(egui::vec2(CELDA_P, ALTO_P), |ui| {
                            ui.set_max_width(CELDA_P);
                            ui.push_id(("prof", i), |ui| {
                                let marco = egui::Frame::none()
                                    .fill(if marcada { ACCENT().linear_multiply(0.30) } else { CARD() })
                                    .rounding(6.0)
                                    .inner_margin(4.0);
                                marco.show(ui, |ui| {
                                    ui.set_min_width(CELDA_P - 10.0);
                                    ui.set_max_width(CELDA_P - 10.0);
                                    ui.vertical_centered(|ui| {
                                        let alto = CELDA_P - 12.0;
                                        let clic = if let Some(tex) = self.profile_thumbs.get(&i) {
                                            let t = tex.size_vec2();
                                            let k = (alto / t.x.max(1.0)).min(alto / t.y.max(1.0));
                                            ui.add(
                                                egui::ImageButton::new((tex.id(), t * k)).frame(false),
                                            )
                                            .clicked()
                                        } else {
                                            let etq = if thumb.starts_with("http")
                                                && !self.profile_failed.contains(&i)
                                            {
                                                "…"
                                            } else if es_img {
                                                "🖼"
                                            } else {
                                                "🎬"
                                            };
                                            ui.add_sized(
                                                [alto, alto],
                                                egui::Button::new(
                                                    RichText::new(etq).size(20.0).color(MUTED()),
                                                )
                                                .frame(false),
                                            )
                                            .clicked()
                                        };
                                        if clic {
                                            sel = !sel;
                                        }

                                        ui.horizontal(|ui| {
                                            ui.checkbox(&mut sel, "");
                                            ui.label(
                                                RichText::new(if es_img { "🖼" } else { "🎬" })
                                                    .size(10.0),
                                            );
                                            if ui
                                                .small_button("🗑")
                                                .on_hover_text(t(lang, "tip.remove"))
                                                .clicked()
                                            {
                                                borrar = true;
                                            }
                                        });
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(titulo).size(9.0).color(MUTED()),
                                            )
                                            .wrap(),
                                        );
                                    });
                                })
                                .response
                            })
                            .response
                            .on_hover_text(format!("{id_post}\n{url}"));
                            });

                            if sel != marcada {
                                self.profile_entries[i].selected = sel;
                            }
                            if borrar {
                                drop_one = Some(i);
                            }
                        }
                    });
                });
                ui.separator();
                if primary_button(ui, &i18n::add_selected(lang, selected)).clicked() && selected > 0 {
                    let author = {
                        let re = Regex::new(r"@([\w.\-]+)").unwrap();
                        re.captures(&self.profile_url)
                            .map(|c| c[1].to_string())
                            .unwrap_or_default()
                    };
                    let to_add: Vec<(String, String, String, String)> = visible
                        .iter()
                        .filter(|&&i| self.profile_entries[i].selected)
                        .map(|&i| {
                            let e = &self.profile_entries[i];
                            (e.url.clone(), e.title.clone(), e.id.clone(), e.thumb.clone())
                        })
                        .collect();
                    let n = to_add.len();
                    for (url, title, id, thumb) in to_add {
                        self.add_url(&url, &author, &title, &url, &id, &thumb);
                    }
                    self.toast(i18n::added_to_queue(lang, n));
                    self.view = View::Downloads;
                }
            });

            // Aplicar los borrados fuera del pintado de la lista
            if clear_all {
                let n = self.profile_entries.len();
                self.profile_entries.clear();
                self.toast(i18n::list_cleared(lang, n));
            } else if remove_selected {
                let before = self.profile_entries.len();
                self.profile_entries.retain(|e| !e.selected);
                let n = before - self.profile_entries.len();
                self.toast(i18n::list_cleared(lang, n));
            } else if let Some(i) = drop_one {
                if i < self.profile_entries.len() {
                    self.profile_entries.remove(i);
                }
            }
        }
    }

    // ---------------- Vista Capturar (Click'n'Load) ----------------

    fn capture_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "cap.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(4.0);
        ui.label(RichText::new(t(lang, "cap.subtitle")).color(MUTED()));
        ui.add_space(14.0);

        // Estado del receptor
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.horizontal(|ui| {
                if self.settings.receiver_enabled {
                    ui.label(RichText::new(t(lang, "cap.listening")).color(GREEN()));
                    ui.label(
                        RichText::new(format!("127.0.0.1:{}", self.settings.receiver_port))
                            .color(CYAN()),
                    );
                } else {
                    ui.label(RichText::new(t(lang, "cap.off")).color(AMBER()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let chk = ui.checkbox(&mut self.settings.receiver_enabled, t(lang, "cap.enable"));
                    if chk.changed() {
                        self.recv_enabled
                            .store(self.settings.receiver_enabled, Ordering::Relaxed);
                    }
                });
            });
            ui.label(RichText::new(t(lang, "cap.note_restart")).size(11.5).color(MUTED()));
        });
        ui.add_space(12.0);

        // Selector de sitio + pasos
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.label(RichText::new(t(lang, "cap.site")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (i, name) in ["TikTok", "Douyin"].iter().enumerate() {
                    let sel = self.capture_site == i;
                    let btn = egui::Button::new(
                        RichText::new(*name).color(if sel { Color32::WHITE } else { MUTED() }),
                    )
                    .fill(if sel { ACCENT() } else { CARD_HOVER() })
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
            ui.label(RichText::new(t(lang, "cap.preview")).size(11.0).color(MUTED()).strong());
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

    // ---------------- Vista Apoyar ----------------

    fn support_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;

        // Al entrar en la pestaña se sortea otro GIF
        if self.tip_reload {
            self.tip_reload = false;
            self.tip_frames = load_random_tip_gif(ui.ctx());
            self.tip_started = Some(Instant::now());
        }

        ui.label(RichText::new(t(lang, "tip.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(4.0);
        ui.label(RichText::new(t(lang, "tip.subtitle")).color(MUTED()));
        ui.add_space(14.0);

        // ---- Mensaje ----
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            ui.label(RichText::new(t(lang, "tip.msg1")).size(13.5).color(TEXT()));
            ui.add_space(8.0);
            ui.label(RichText::new(t(lang, "tip.msg2")).size(13.0).color(MUTED()));
            ui.add_space(8.0);
            ui.label(RichText::new(t(lang, "tip.msg3")).size(13.0).color(TEXT()));
            ui.add_space(8.0);
            ui.label(RichText::new(t(lang, "tip.msg4")).size(12.5).color(MUTED()));
            ui.add_space(6.0);
            ui.label(RichText::new(t(lang, "tip.thanks")).size(14.0).strong().color(ACCENT()));
        });
        ui.add_space(14.0);

        // ---- Botones ----
        let links: Vec<(&str, &str, Color32)> = [
            ("support.kofi", KOFI_URL, ACCENT()),
            ("support.paypal", PAYPAL_URL, CYAN()),
            ("support.sponsors", SPONSORS_URL, GREEN()),
        ]
        .into_iter()
        .filter(|(_, url, _)| link_ready(url))
        .collect();

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            ui.label(RichText::new(t(lang, "support.title")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.label(RichText::new(t(lang, "tip.help")).size(13.0).color(TEXT()));
            ui.add_space(10.0);
            if links.is_empty() {
                // Ningún enlace configurado todavía: se explica en vez de
                // dejar una tarjeta vacía sin sentido.
                ui.label(RichText::new(t(lang, "tip.no_links")).size(12.0).color(AMBER()));
            } else {
                ui.horizontal_wrapped(|ui| {
                    for (key, url, color) in &links {
                        let btn = egui::Button::new(
                            RichText::new(t(lang, key)).size(14.0).color(Color32::WHITE).strong(),
                        )
                        .fill(*color)
                        .rounding(Rounding::same(10.0))
                        .min_size(egui::vec2(150.0, 40.0));
                        let resp = ui.add(btn);
                        gloss_paint(ui, &resp);
                        if resp.clicked() {
                            if let Err(e) = open::that(*url) {
                                self.toast(format!("{e}"));
                            }
                        }
                    }
                });
            }
            ui.add_space(6.0);
            ui.label(RichText::new(t(lang, "support.optional")).size(11.5).color(MUTED()));
        });

        // ---- Animación, enmarcada y pegada bajo el panel de apoyo ----
        //
        // Va aquí abajo a propósito: primero el mensaje y los botones, y la
        // animación como remate visual. Si no hay ningún GIF en la carpeta,
        // simplemente no se dibuja nada — nunca se enseña la ruta interna al
        // usuario, que además quedaría fea en una captura.
        if !self.tip_frames.is_empty() {
            // Fotograma que toca según el tiempo transcurrido, en bucle
            let total: f32 = self.tip_frames.iter().map(|(_, d)| *d).sum::<f32>().max(0.05);
            let elapsed = self
                .tip_started
                .map(|s| s.elapsed().as_secs_f32())
                .unwrap_or(0.0)
                % total;
            let mut acc = 0.0;
            let mut idx = 0;
            for (i, (_, d)) in self.tip_frames.iter().enumerate() {
                acc += *d;
                if elapsed < acc {
                    idx = i;
                    break;
                }
            }
            let (tex, _) = &self.tip_frames[idx];
            let ts = tex.size_vec2();

            // Se ajusta al ancho del panel de arriba para que queden alineados,
            // sin deformar la imagen y con tope de altura.
            let panel_w = ui.available_width().min(720.0);
            let mut w = panel_w;
            let mut h = w / ts.x.max(1.0) * ts.y;
            if h > 240.0 {
                h = 240.0;
                w = h / ts.y.max(1.0) * ts.x;
            }

            ui.add_space(10.0);
            // Marco oscuro estilizado: borde negro grueso + halo del acento
            egui::Frame::none()
                .fill(Color32::BLACK)
                .rounding(Rounding::same(14.0))
                .inner_margin(Margin::same(6.0))
                .stroke(Stroke::new(2.0f32, ACCENT().gamma_multiply(0.55)))
                .show(ui, |ui| {
                    ui.set_width(panel_w);
                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(
                                tex.id(),
                                egui::vec2(w, h),
                            ))
                            .rounding(Rounding::same(9.0)),
                        );
                    });
                });
            // Repintado continuo para que la animación corra
            ui.ctx().request_repaint_after(Duration::from_millis(60));
        }
    }


    // ---------------- Vista Booru ----------------

    fn booru_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "booru.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(4.0);
        ui.label(RichText::new(t(lang, "booru.subtitle")).color(MUTED()));
        ui.add_space(14.0);

        let site = &booru::SITES[self.booru_site.min(booru::SITES.len() - 1)];
        let mut do_search = false;

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(860.0));
            // Selector de sitio
            ui.horizontal_wrapped(|ui| {
                for (i, s) in booru::SITES.iter().enumerate() {
                    let sel = self.booru_site == i;
                    let btn = egui::Button::new(
                        RichText::new(s.name).color(if sel { Color32::WHITE } else { MUTED() }),
                    )
                    .fill(if sel { ACCENT() } else { CARD_HOVER() })
                    .rounding(Rounding::same(8.0));
                    let r = ui.add(btn);
                    gloss_paint(ui, &r);
                    if r.clicked() {
                        self.booru_site = i;
                        self.booru_page = 1;
                    }
                }
            });
            ui.add_space(8.0);

            // Etiquetas
            ui.label(RichText::new(t(lang, "booru.tags")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let te = ui.add_sized(
                    [420.0, 30.0],
                    egui::TextEdit::singleline(&mut self.booru_tags)
                        .hint_text("landscape scenery 1girl  ·  rating:general"),
                );
                // Enter también busca
                if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    do_search = true;
                }
                if self.booru_searching {
                    ui.spinner();
                } else if primary_button(ui, t(lang, "booru.search")).clicked() {
                    do_search = true;
                }

                // Ejemplos: además de dar por dónde empezar, enseñan la
                // convención de nombres de los boorus, que no es evidente.
                egui::ComboBox::from_id_source("booru_samples")
                    .selected_text(t(lang, "booru.samples"))
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        for (label, tag) in booru::SAMPLE_TAGS {
                            if ui.selectable_label(false, *label).on_hover_text(*tag).clicked() {
                                self.booru_tags = tag.to_string();
                                self.booru_page = 1;
                                do_search = true;
                            }
                        }
                    });
            });

            // Filtros
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(t(lang, "booru.min_width")).size(12.0).color(MUTED()));
                ui.add(egui::DragValue::new(&mut self.booru_min_w).suffix(" px").range(0..=8000));
                ui.add_space(10.0);
                ui.label(RichText::new(t(lang, "booru.rating")).size(12.0).color(MUTED()));
                for (code, key) in [("", "booru.rating_all"), ("g", "booru.rating_safe"), ("s", "booru.rating_sensitive")] {
                    let sel = self.booru_rating == code;
                    if ui
                        .selectable_label(sel, RichText::new(t(lang, key)).size(12.0))
                        .clicked()
                    {
                        self.booru_rating = code.to_string();
                    }
                }
            });

            if site.needs_auth && self.settings.booru_key.trim().is_empty() {
                ui.label(RichText::new(t(lang, "booru.needs_auth")).size(11.5).color(AMBER()));
            }
            if self.galdl_cmd.is_none() {
                ui.label(RichText::new(t(lang, "profile.need_galdl")).size(11.5).color(RED()));
            }
        });

        if do_search && !self.booru_searching {
            if let Some(prog) = self.galdl_cmd.clone() {
                self.booru_searching = true;
                self.booru_posts.clear();
                let url = booru::search_url(site, &self.booru_tags);
                let auth = booru::auth_config(site, &self.settings.booru_user, &self.settings.booru_key);
                let tx = self.tx.clone();
                let page = self.booru_page;
                self.booru_epoch += 1;
                let ep = self.booru_epoch;
                self.rt.spawn(booru_search(prog, url, page, 40, auth, tx, ep));
            } else {
                self.toast(t(lang, "profile.need_galdl"));
            }
        }

        if self.booru_posts.is_empty() {
            return;
        }
        ui.add_space(12.0);

        // Filtrado local (resolución y clasificación)
        let min_w = self.booru_min_w;
        let rating = self.booru_rating.clone();
        let visible: Vec<usize> = self
            .booru_posts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.width >= min_w)
            .filter(|(_, p)| rating.is_empty() || p.rating == rating)
            .map(|(i, _)| i)
            .collect();

        let sel_count = visible.iter().filter(|&&i| self.booru_posts[i].selected).count();

        // Barra de acciones (aplicadas tras pintar, para no romper índices)
        let mut set_all: Option<bool> = None;
        let mut page_delta: i32 = 0;
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(860.0));
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(i18n::booru_summary(lang, visible.len(), sel_count)).strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if soft_button(ui, "▶").on_hover_text(t(lang, "booru.next")).clicked() {
                        page_delta = 1;
                    }
                    ui.label(RichText::new(format!("{}", self.booru_page)).color(CYAN()));
                    if self.booru_page > 1
                        && soft_button(ui, "◀").on_hover_text(t(lang, "booru.prev")).clicked()
                    {
                        page_delta = -1;
                    }
                    ui.add_space(10.0);
                    if soft_button(ui, t(lang, "btn.none")).clicked() {
                        set_all = Some(false);
                    }
                    if soft_button(ui, t(lang, "btn.all")).clicked() {
                        set_all = Some(true);
                    }
                });
            });
        });
        ui.add_space(8.0);

        // Rejilla de miniaturas
        const CELL: f32 = 150.0;
        let avail = ui.available_width().min(880.0);
        let cols = ((avail / (CELL + 10.0)).floor() as usize).max(1);
        let client = self.client.clone();
        let tx = self.tx.clone();
        let rt = self.rt.handle().clone();
        let mut pending = std::mem::take(&mut self.booru_pending);
        let mut toggle: Option<usize> = None;

        egui::ScrollArea::vertical()
            .max_height(520.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("booru_grid").spacing([10.0, 10.0]).show(ui, |ui| {
                    for (n, &i) in visible.iter().enumerate() {
                        let p = &self.booru_posts[i];
                        // Descarga perezosa de la miniatura
                        if !self.booru_thumbs.contains_key(&p.id)
                            && !pending.contains(&p.id)
                            && pending.len() < 60
                        {
                            pending.insert(p.id);
                            rt.spawn(fetch_booru_thumb(
                                client.clone(),
                                p.id,
                                p.preview_url.clone(),
                                tx.clone(),
                            ));
                        }

                        let sel = p.selected;
                        egui::Frame::none()
                            .fill(if sel { ACCENT().gamma_multiply(0.25) } else { CARD() })
                            .rounding(Rounding::same(10.0))
                            .stroke(Stroke::new(
                                if sel { 2.0f32 } else { 1.0f32 },
                                if sel { ACCENT() } else { CARD_HOVER() },
                            ))
                            .inner_margin(Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.set_width(CELL);
                                ui.vertical(|ui| {
                                    let resp = if let Some(tex) = self.booru_thumbs.get(&p.id) {
                                        let ts = tex.size_vec2();
                                        let h = (CELL / ts.x.max(1.0) * ts.y).min(CELL);
                                        ui.add(
                                            egui::Image::new(egui::load::SizedTexture::new(
                                                tex.id(),
                                                egui::vec2(CELL, h),
                                            ))
                                            .rounding(Rounding::same(6.0))
                                            .sense(egui::Sense::click()),
                                        )
                                    } else {
                                        ui.allocate_response(
                                            egui::vec2(CELL, CELL * 0.7),
                                            egui::Sense::click(),
                                        )
                                    };
                                    if resp.clicked() {
                                        toggle = Some(i);
                                    }
                                    // Datos útiles sin saturar: resolución y peso
                                    ui.label(
                                        RichText::new(format!("{}×{}", p.width, p.height))
                                            .size(10.5)
                                            .color(if p.width >= 1920 { GREEN() } else { MUTED() }),
                                    );
                                    ui.horizontal(|ui| {
                                        // Marca los vídeos: los boorus también
                                        // alojan webm/mp4 y conviene saberlo
                                        // antes de encolar 40 «imágenes».
                                        if !p.is_image() {
                                            ui.label(RichText::new("🎬").size(10.0).color(CYAN()));
                                        }
                                        ui.label(
                                            RichText::new(format!(".{}", p.ext))
                                                .size(10.0)
                                                .color(MUTED()),
                                        );
                                        if p.file_size > 0 {
                                            ui.label(
                                                RichText::new(fmt_size(p.file_size as f64))
                                                    .size(10.0)
                                                    .color(MUTED()),
                                            );
                                        }
                                    });
                                    // Autor, si el booru lo expone. Se recorta
                                    // para no romper la rejilla.
                                    if !p.artist.is_empty() {
                                        let a: String = p.artist.chars().take(20).collect();
                                        ui.label(RichText::new(a).size(9.5).color(ACCENT()))
                                            .on_hover_text(&p.artist);
                                    }
                                });
                            });

                        if (n + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
            });

        self.booru_pending = pending;
        if let Some(i) = toggle {
            self.booru_posts[i].selected = !self.booru_posts[i].selected;
        }
        if let Some(v) = set_all {
            for &i in &visible {
                self.booru_posts[i].selected = v;
            }
        }
        if page_delta != 0 {
            self.booru_page = (self.booru_page as i32 + page_delta).max(1) as u32;
            if let Some(prog) = self.galdl_cmd.clone() {
                self.booru_searching = true;
                let url = booru::search_url(site, &self.booru_tags);
                let auth = booru::auth_config(site, &self.settings.booru_user, &self.settings.booru_key);
                self.booru_epoch += 1;
                    let ep = self.booru_epoch;
                    self.rt.spawn(booru_search(prog, url, self.booru_page, 40, auth, self.tx.clone(), ep));
            }
        }

        ui.add_space(10.0);
        if primary_button(ui, &i18n::booru_add(lang, sel_count)).clicked() && sel_count > 0 {
            let site_name = site.name.to_string();
            let chosen: Vec<(String, String, String, String)> = visible
                .iter()
                .filter(|&&i| self.booru_posts[i].selected)
                .map(|&i| {
                    let p = &self.booru_posts[i];
                    (
                        p.file_url.clone(),
                        p.id.to_string(),
                        p.preview_url.clone(),
                        // El primer artista etiquetado sirve de carpeta/autor:
                        // así la subcarpeta por autor tiene sentido también aquí.
                        p.artist.split_whitespace().next().unwrap_or("").to_string(),
                    )
                })
                .collect();
            let n = chosen.len();
            for (url, id, thumb, artist) in chosen {
                // El original va por el motor HTTP nativo: máxima calidad y
                // reanudación. La miniatura alimenta la vista de la cola.
                let author = if artist.is_empty() { site_name.clone() } else { artist };
                self.add_url(&url, &author, "", "", &id, &thumb);
            }
            self.toast(i18n::added_to_queue(lang, n));
            self.view = View::Downloads;
        }
    }

    // ---------------- Vista de torrents ----------------

    fn torrents_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "torrent.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(4.0);
        ui.label(RichText::new(t(lang, "torrent.subtitle")).color(MUTED()));
        ui.add_space(14.0);

        // Alta de torrent
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.label(RichText::new(t(lang, "torrent.add_label")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [440.0, 30.0],
                    egui::TextEdit::singleline(&mut self.torrent_input)
                        .hint_text("magnet:?xt=…   ·   https://…/archivo.torrent"),
                );
                if soft_button(ui, t(lang, "btn.paste")).clicked() {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        if let Ok(txt) = cb.get_text() {
                            self.torrent_input = txt.trim().to_string();
                        }
                    }
                }
                if soft_button(ui, t(lang, "torrent.pick_file")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("torrent", &["torrent"]).pick_file() {
                        self.torrent_input = p.to_string_lossy().into_owned();
                    }
                }
            });
            ui.add_space(8.0);
            if self.torrent_adding {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(t(lang, "torrent.adding")).color(CYAN()));
                });
            } else if primary_button(ui, t(lang, "torrent.add_btn")).clicked() {
                let src = std::mem::take(&mut self.torrent_input);
                self.add_torrent(src);
            }
            ui.label(RichText::new(t(lang, "torrent.legal")).size(11.0).color(AMBER()));
        });
        ui.add_space(12.0);

        // Ajustes (carpeta + velocidad) plegados por defecto: no saturan y
        // quedan a un clic cuando hacen falta.
        let session_live = self.torrent_client.is_some();
        egui::CollapsingHeader::new(RichText::new(t(lang, "torrent.options")).size(12.5).color(MUTED()))
            .id_source("torrent_opts")
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(t(lang, "torrent.folder_label")).size(11.0).color(MUTED()).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let shown = self.settings.torrent_folder().to_string_lossy().into_owned();
                    let mut dir = if self.settings.torrent_dir.trim().is_empty() { shown } else { self.settings.torrent_dir.clone() };
                    if ui.add_sized([380.0, 28.0], egui::TextEdit::singleline(&mut dir)).changed() {
                        self.settings.torrent_dir = dir;
                    }
                    if soft_button(ui, t(lang, "btn.browse")).clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.settings.torrent_dir = d.to_string_lossy().into_owned();
                        }
                    }
                    if soft_button(ui, t(lang, "btn.open")).clicked() {
                        let _ = open::that(self.settings.torrent_folder());
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("↓ {}", t(lang, "torrent.down_limit"))).size(12.0).color(MUTED()));
                    ui.add(egui::DragValue::new(&mut self.settings.torrent_down_kbps).suffix(" KiB/s").range(0..=1_000_000));
                    ui.add_space(12.0);
                    ui.label(RichText::new(format!("↑ {}", t(lang, "torrent.up_limit"))).size(12.0).color(MUTED()));
                    ui.add(egui::DragValue::new(&mut self.settings.torrent_up_kbps).suffix(" KiB/s").range(0..=1_000_000));
                    ui.label(RichText::new(t(lang, "torrent.limit_zero")).size(11.0).color(MUTED()));
                });
                if session_live && (self.settings.torrent_down_kbps > 0 || self.settings.torrent_up_kbps > 0) {
                    ui.label(RichText::new(t(lang, "torrent.limit_restart")).size(11.0).color(AMBER()));
                }
            });
        ui.add_space(10.0);

        if self.torrents.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("🌀").size(42.0));
                ui.add_space(6.0);
                ui.label(RichText::new(t(lang, "torrent.empty")).size(15.0).color(MUTED()));
            });
            return;
        }

        // Chip compacto (icono + valor) con fondo tenue del color dado
        fn chip(ui: &mut egui::Ui, text: String, color: Color32) {
            egui::Frame::none()
                .fill(color.gamma_multiply(0.14))
                .rounding(Rounding::same(7.0))
                .inner_margin(Margin::symmetric(8.0, 2.0))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).size(11.5).color(color));
                });
        }

        // Acciones diferidas (fuera del préstamo de la lista)
        enum TAct {
            Pause(usize),
            Resume(usize),
            Remove(usize, bool),
        }
        let mut acts: Vec<TAct> = Vec::new();

        for (idx, h) in self.torrents.iter().enumerate() {
            let snap = h.snapshot();
            // Velocidad estimada por delta de bytes
            let now = Instant::now();
            let speed = {
                let e = self.torrent_speed.entry(h.id).or_insert((snap.downloaded, now, 0.0));
                let dt = now.duration_since(e.1).as_secs_f64();
                if dt >= 0.5 {
                    let db = snap.downloaded.saturating_sub(e.0) as f64;
                    e.2 = db / dt;
                    e.0 = snap.downloaded;
                    e.1 = now;
                }
                e.2
            };
            // ETA a partir de la velocidad actual
            let eta = if speed > 1.0 && !snap.finished {
                fmt_eta((snap.total.saturating_sub(snap.downloaded)) as f64 / speed)
            } else {
                "—".into()
            };

            egui::Frame::none()
                .fill(CARD())
                .rounding(Rounding::same(12.0))
                .inner_margin(Margin::symmetric(16.0, 12.0))
                .stroke(Stroke::new(1.0f32, CARD_HOVER()))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width().min(880.0));

                    // Cabecera: nombre + acciones a la derecha
                    ui.horizontal(|ui| {
                        let name: String = h.display_name().chars().take(72).collect();
                        ui.label(RichText::new(name).size(14.0).color(TEXT()).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("🗑").on_hover_text(t(lang, "torrent.remove")).clicked() {
                                acts.push(TAct::Remove(idx, false));
                            }
                            if snap.state == self::torrents::State::Paused {
                                if ui.small_button("▶").on_hover_text(t(lang, "tip.start")).clicked() {
                                    acts.push(TAct::Resume(idx));
                                }
                            } else if !snap.finished
                                && ui.small_button("⏸").on_hover_text(t(lang, "tip.pause")).clicked()
                            {
                                acts.push(TAct::Pause(idx));
                            }
                            // Porcentaje grande a la derecha del nombre
                            ui.label(
                                RichText::new(format!("{:.0}%", snap.progress * 100.0))
                                    .size(13.0)
                                    .strong()
                                    .color(if snap.finished { GREEN() } else { CYAN() }),
                            );
                        });
                    });

                    ui.add_space(6.0);
                    // Barra de progreso fina
                    ui.add(
                        egui::ProgressBar::new(snap.progress)
                            .desired_height(7.0)
                            .fill(if snap.finished { GREEN() } else { ACCENT() }),
                    );
                    ui.add_space(8.0);

                    // Fila de stats con chips
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;

                        // Estado
                        let (state_txt, state_col) = match snap.state {
                            self::torrents::State::Initializing if snap.downloaded > 0 => (t(lang, "torrent.state_down"), CYAN()),
                            self::torrents::State::Initializing => (t(lang, "torrent.state_init"), MUTED()),
                            self::torrents::State::Live if snap.finished => (t(lang, "torrent.state_seeding"), GREEN()),
                            self::torrents::State::Live => (t(lang, "torrent.state_down"), CYAN()),
                            self::torrents::State::Paused => (t(lang, "status.paused"), AMBER()),
                            self::torrents::State::Error => (t(lang, "status.error"), RED()),
                        };
                        chip(ui, state_txt.to_string(), state_col);

                        if speed > 0.0 && !snap.finished {
                            chip(ui, format!("↓ {}/s", fmt_size(speed)), CYAN());
                        }
                        let peer_col = if snap.peers > 0 { CYAN() } else { MUTED() };
                        let r = ui.scope(|ui| chip(ui, format!("👥 {}", snap.peers), peer_col)).response;
                        r.on_hover_text(t(lang, "torrent.peers_tip"));

                        if snap.state == self::torrents::State::Live && !snap.finished {
                            chip(ui, format!("⏱ {eta}"), MUTED());
                        }
                        if snap.uploaded > 0 {
                            chip(ui, format!("↑ {}", fmt_size(snap.uploaded as f64)), MUTED());
                        }

                        // Tamaño a la derecha
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} / {}",
                                    fmt_size(snap.downloaded as f64),
                                    fmt_size(snap.total as f64)
                                ))
                                .size(11.5)
                                .color(MUTED()),
                            );
                        });
                    });

                    if let Some(err) = &snap.error {
                        ui.add_space(4.0);
                        ui.label(RichText::new(err).size(11.0).color(RED()));
                    }
                });
            ui.add_space(10.0);
        }

        // Aplicar acciones
        for a in acts {
            match a {
                TAct::Pause(i) => {
                    if let (Some(c), Some(h)) = (self.torrent_client.clone(), self.torrents.get(i)) {
                        let inner = h.inner.clone();
                        self.rt.spawn(async move { c.pause(&inner).await });
                    }
                }
                TAct::Resume(i) => {
                    if let (Some(c), Some(h)) = (self.torrent_client.clone(), self.torrents.get(i)) {
                        let inner = h.inner.clone();
                        self.rt.spawn(async move { c.resume(&inner).await });
                    }
                }
                TAct::Remove(i, del) => {
                    if let (Some(c), Some(h)) = (self.torrent_client.clone(), self.torrents.get(i)) {
                        let inner = h.inner.clone();
                        self.rt.spawn(async move { c.remove(&inner, del).await });
                        let id = h.id;
                        self.torrent_speed.remove(&id);
                        self.torrents.remove(i);
                    }
                }
            }
        }

        // Repintar en vivo mientras haya torrents activos
        ui.ctx().request_repaint_after(Duration::from_millis(700));
    }

    // ---------------- Vista de ajustes ----------------

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(RichText::new(t(lang, "set.title")).size(24.0).strong().color(Color32::WHITE));
        ui.add_space(16.0);

        // ---- Idioma ----
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.language")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t(lang, "set.language_label"));
                for l in Lang::ALL {
                    let selected = self.settings.lang == l;
                    let btn = egui::Button::new(
                        RichText::new(l.label()).color(if selected { Color32::WHITE } else { MUTED() }),
                    )
                    .fill(if selected { ACCENT() } else { CARD_HOVER() })
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

        // ---- Tema / skin ----
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.theme")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for th in Theme::ALL {
                    let selected = self.settings.theme == th;
                    // Cada botón se tiñe con el acento de SU tema: se ve al vuelo
                    let swatch = th.palette().accent;
                    let btn = egui::Button::new(
                        RichText::new(th.label(lang)).color(if selected { Color32::WHITE } else { MUTED() }),
                    )
                    .fill(if selected { swatch } else { CARD_HOVER() })
                    .stroke(Stroke::new(1.0f32, swatch.gamma_multiply(0.7)))
                    .rounding(Rounding::same(8.0));
                    let resp = ui.add(btn);
                    gloss_paint(ui, &resp);
                    if resp.clicked() && !selected {
                        self.settings.theme = th;
                        // Aplicar en caliente: paleta primero, luego el estilo
                        set_palette(th);
                        apply_theme(ui.ctx());
                    }
                }
            });
            ui.label(RichText::new(t(lang, "set.theme_note")).size(11.5).color(MUTED()));

            // ---- Imagen de fondo personalizada ----
            ui.add_space(10.0);
            ui.label(RichText::new(t(lang, "set.bg_image")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if soft_button(ui, t(lang, "set.bg_pick")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Imagen", &["png", "jpg", "jpeg", "webp", "bmp"])
                        .pick_file()
                    {
                        self.settings.bg_image = p.to_string_lossy().into_owned();
                    }
                }
                if !self.settings.bg_image.is_empty()
                    && soft_button(ui, t(lang, "set.bg_clear")).clicked()
                {
                    self.settings.bg_image.clear();
                }
            });
            if !self.settings.bg_image.is_empty() {
                // Nombre del archivo, no la ruta entera: cabe y se lee mejor
                let name = std::path::Path::new(&self.settings.bg_image)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.label(RichText::new(name).size(11.0).color(CYAN()));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t(lang, "set.bg_opacity")).size(12.0).color(MUTED()));
                    ui.add(
                        egui::Slider::new(&mut self.settings.bg_opacity, 0.0..=0.85)
                            .show_value(false),
                    );
                    ui.label(
                        RichText::new(format!("{:.0}%", self.settings.bg_opacity * 100.0))
                            .size(12.0)
                            .color(MUTED()),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t(lang, "set.bg_blur")).size(12.0).color(MUTED()));
                    let resp = ui.add(
                        egui::Slider::new(&mut self.settings.bg_blur, 0.0..=24.0).show_value(false),
                    );
                    // Difuminar cuesta CPU: se recalcula al SOLTAR, no mientras
                    // se arrastra, o la interfaz se atascaría.
                    if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                        self.bg_dirty = true;
                    }
                    ui.label(
                        RichText::new(if self.settings.bg_blur < 0.1 {
                            t(lang, "set.bg_blur_off").to_string()
                        } else {
                            format!("{:.0}", self.settings.bg_blur)
                        })
                        .size(12.0)
                        .color(MUTED()),
                    );
                });
            }
            ui.label(RichText::new(t(lang, "set.bg_note")).size(11.5).color(MUTED()));
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.folder")).size(11.0).color(MUTED()).strong());
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
            ui.label(RichText::new(t(lang, "set.downloads")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t(lang, "set.concurrency"));
                ui.add(egui::Slider::new(&mut self.settings.concurrency, 1..=8));
            });
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.linkgrabber")).size(11.0).color(MUTED()).strong());
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
            ui.label(RichText::new(t(lang, "set.receiver")).size(11.0).color(MUTED()).strong());
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
            ui.label(RichText::new(t(lang, "cap.note_restart")).size(11.5).color(MUTED()));
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.cookies")).size(11.0).color(MUTED()).strong());
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
                ui.label(RichText::new(t(lang, "set.cookies_note")).size(11.5).color(MUTED()));
                ui.label(RichText::new(t(lang, "set.cookies_warn")).size(11.5).color(AMBER()));
            }
            ui.add_space(6.0);
            ui.label(RichText::new(t(lang, "set.cookies_file")).size(11.5).color(MUTED()));
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
                ui.label(RichText::new(t(lang, "set.cookies_file_note")).size(11.5).color(MUTED()));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.history")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            let archive = galdl_archive_path();
            let size = std::fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
            ui.label(
                RichText::new(if size > 0 {
                    format!("{}  ·  {}", archive.display(), fmt_size(size as f64))
                } else {
                    archive.display().to_string()
                })
                .size(11.0)
                .color(MUTED()),
            );
            if soft_button(ui, t(lang, "btn.clear_history")).clicked() {
                let msg = if std::fs::remove_file(&archive).is_ok() {
                    t(lang, "toast.history_cleared")
                } else {
                    t(lang, "toast.history_empty")
                };
                self.toast(msg);
            }
            ui.label(RichText::new(t(lang, "set.history_note")).size(11.5).color(MUTED()));
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "eng.ytdlp")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            if self.ytdlp_installing {
                ui.add(
                    egui::ProgressBar::new(self.ytdlp_progress)
                        .fill(CYAN())
                        .show_percentage(),
                );
                ui.label(RichText::new(t(lang, "eng.ytdlp_downloading")).color(MUTED()));
            } else if self.ytdlp_ok == Some(true) {
                ui.label(RichText::new(t(lang, "eng.ytdlp_ok")).color(GREEN()));
                if let Some(cmd) = &self.ytdlp_cmd {
                    ui.label(RichText::new(cmd.as_str()).size(11.5).color(MUTED()));
                }
                if soft_button(ui, t(lang, "btn.update_latest")).clicked() {
                    self.ytdlp_installing = true;
                    self.ytdlp_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_ytdlp(client, tx));
                }
            } else {
                ui.label(RichText::new(t(lang, "eng.ytdlp_missing")).color(AMBER()));
                if primary_button(ui, t(lang, "eng.install_ytdlp")).clicked() {
                    self.ytdlp_installing = true;
                    self.ytdlp_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_ytdlp(client, tx));
                }
                ui.label(RichText::new(t(lang, "eng.ytdlp_note")).size(11.5).color(MUTED()));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "eng.galdl")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            if self.galdl_installing {
                ui.add(
                    egui::ProgressBar::new(self.galdl_progress)
                        .fill(CYAN())
                        .show_percentage(),
                );
                ui.label(RichText::new(t(lang, "eng.galdl_downloading")).color(MUTED()));
            } else if let Some(cmd) = self.galdl_cmd.clone() {
                ui.label(RichText::new(t(lang, "eng.galdl_ok")).color(GREEN()));
                ui.label(RichText::new(cmd).size(11.5).color(MUTED()));
                if soft_button(ui, t(lang, "btn.update_latest")).clicked() {
                    self.galdl_installing = true;
                    self.galdl_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_gallerydl(client, tx));
                }
            } else {
                ui.label(RichText::new(t(lang, "eng.galdl_missing")).color(AMBER()));
                if primary_button(ui, t(lang, "eng.install_galdl")).clicked() {
                    self.galdl_installing = true;
                    self.galdl_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_gallerydl(client, tx));
                }
                ui.label(RichText::new(t(lang, "eng.galdl_note")).size(11.5).color(MUTED()));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "eng.ffmpeg")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            if self.ffmpeg_installing {
                ui.add(
                    egui::ProgressBar::new(self.ffmpeg_progress)
                        .fill(CYAN())
                        .show_percentage(),
                );
                ui.label(RichText::new(t(lang, "eng.ffmpeg_downloading")).color(MUTED()));
            } else if let Some(cmd) = self.ffmpeg_cmd.clone() {
                ui.label(RichText::new(t(lang, "eng.ffmpeg_ok")).color(GREEN()));
                ui.label(RichText::new(cmd).size(11.5).color(MUTED()));
                ui.label(RichText::new(t(lang, "eng.ffmpeg_quality_on")).size(11.5).color(CYAN()));
            } else {
                ui.label(RichText::new(t(lang, "eng.ffmpeg_missing")).color(AMBER()));
                if primary_button(ui, t(lang, "eng.install_ffmpeg")).clicked() {
                    self.ffmpeg_installing = true;
                    self.ffmpeg_progress = 0.0;
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.rt.spawn(install_ffmpeg(client, tx));
                }
                ui.label(RichText::new(t(lang, "eng.ffmpeg_note")).size(11.5).color(MUTED()));
            }
        });
        ui.add_space(12.0);

        // ---- Credenciales de boorus ----
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.booru")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t(lang, "set.booru_user")).size(12.0).color(MUTED()));
                ui.add_sized([200.0, 26.0], egui::TextEdit::singleline(&mut self.settings.booru_user));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(t(lang, "set.booru_key")).size(12.0).color(MUTED()));
                // password(true): la clave no queda a la vista de nadie que
                // mire la pantalla ni en una captura.
                ui.add_sized(
                    [200.0, 26.0],
                    egui::TextEdit::singleline(&mut self.settings.booru_key).password(true),
                );
            });
            ui.label(RichText::new(t(lang, "set.booru_note")).size(11.5).color(MUTED()));
        });
        ui.add_space(12.0);

        // ---- Manejador de enlaces magnet ----
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "set.magnet")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            let is_handler = is_magnet_handler();
            if is_handler {
                ui.label(RichText::new(t(lang, "set.magnet_on")).color(GREEN()));
                if soft_button(ui, t(lang, "set.magnet_unregister")).clicked() {
                    match set_magnet_handler(false) {
                        Ok(()) => self.toast(t(lang, "set.magnet_removed")),
                        Err(e) => self.toast(e.to_string()),
                    }
                }
            } else {
                ui.label(RichText::new(t(lang, "set.magnet_off")).color(AMBER()));
                if primary_button(ui, t(lang, "set.magnet_register")).clicked() {
                    match set_magnet_handler(true) {
                        Ok(()) => self.toast(t(lang, "set.magnet_done")),
                        Err(e) => self.toast(e.to_string()),
                    }
                }
            }
            ui.label(RichText::new(t(lang, "set.magnet_note")).size(11.5).color(MUTED()));
            // Windows protege la asociación por defecto con UserChoice: si ya
            // hay otro cliente puesto, hay que cambiarlo a mano en Configuración.
            ui.add_space(4.0);
            ui.label(RichText::new(t(lang, "set.magnet_userchoice")).size(11.5).color(AMBER()));
            if soft_button(ui, t(lang, "set.magnet_open_settings")).clicked() {
                let _ = open::that("ms-settings:defaultapps");
            }
        });
        ui.add_space(12.0);

        // ---- cyberdrop-dl (opcional, hosters difíciles) ----
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "eng.cyberdrop")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            if self.cyberdrop_installing {
                ui.add(
                    egui::ProgressBar::new(self.cyberdrop_progress)
                        .fill(CYAN())
                        .show_percentage(),
                );
                ui.label(RichText::new(t(lang, "eng.cyberdrop_downloading")).color(MUTED()));
            } else if let Some(cmd) = self.cyberdrop_cmd.clone() {
                ui.label(RichText::new(t(lang, "eng.cyberdrop_ok")).color(GREEN()));
                ui.label(RichText::new(cmd).size(11.5).color(MUTED()));
            } else {
                ui.label(RichText::new(t(lang, "eng.cyberdrop_missing")).color(AMBER()));
                if primary_button(ui, t(lang, "eng.install_cyberdrop")).clicked() {
                    self.cyberdrop_installing = true;
                    self.cyberdrop_progress = 0.0;
                    let tx = self.tx.clone();
                    self.rt.spawn(install_cyberdrop(tx));
                }
                ui.label(RichText::new(t(lang, "eng.cyberdrop_note")).size(11.5).color(MUTED()));
            }
        });
        ui.add_space(12.0);

        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.0));
            ui.label(RichText::new(t(lang, "about")).size(11.0).color(MUTED()).strong());
            ui.add_space(4.0);
            ui.label(format!(
                "Todo Downloader v{} — By Eric V. Gramunt",
                env!("CARGO_PKG_VERSION")
            ));
            ui.label(RichText::new(t(lang, "about.tech")).color(MUTED()));
        });
    }
}

// ============================= Tests =============================
//
// Lógica pura, sin red y sin subprocesos: se ejecutan igual en Windows,
// Linux y macOS. Cubren las decisiones que rompieron YouTube y el routing.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_extrae_el_dominio() {
        assert_eq!(host_of("https://www.youtube.com/watch?v=abc").as_deref(), Some("www.youtube.com"));
        assert_eq!(host_of("https://weibo.com/tv/show/1034:53278").as_deref(), Some("weibo.com"));
        assert_eq!(host_of("https://passport.weibo.com/visitor/visitor?a=1").as_deref(), Some("passport.weibo.com"));
        // Puerto, userinfo, mayúsculas y punto final no deben confundirlo
        assert_eq!(host_of("https://EXAMPLE.com:8443/x").as_deref(), Some("example.com"));
        assert_eq!(host_of("https://user:pass@weibo.com/x").as_deref(), Some("weibo.com"));
        assert_eq!(host_of("https://weibo.com./x").as_deref(), Some("weibo.com"));
        // Sin esquema no es una URL absoluta
        assert_eq!(host_of("weibo.com/tv/show/1"), None);
        assert_eq!(host_of(""), None);
    }

    #[test]
    fn host_matches_rechaza_dominios_impostores() {
        assert!(host_matches("weibo.com", "weibo.com"));
        assert!(host_matches("passport.weibo.com", "weibo.com"));
        assert!(host_matches("m.weibo.cn", "weibo.cn"));
        // Lo que la comprobación por subcadena dejaba pasar
        assert!(!host_matches("weibo.com.atacante.net", "weibo.com"));
        assert!(!host_matches("notweibo.com", "weibo.com"));
        assert!(!host_matches("weibo.company", "weibo.com"));
        assert!(!host_matches("pixeldrain.com.evil.net", "pixeldrain.com"));
    }

    #[test]
    fn youtube_no_recibe_cookies_de_entrada() {
        // Regresión de yt-dlp#16569: con cookies de cuenta, YouTube exige un
        // PO Token y descarta todos los formatos.
        assert!(!needs_cookies_upfront("https://www.youtube.com/watch?v=95wP2VKGEXE"));
        assert!(!needs_cookies_upfront("https://youtu.be/95wP2VKGEXE"));
        // Tampoco los sitios que funcionan bien sin sesión
        assert!(!needs_cookies_upfront("https://www.tiktok.com/@user/video/123"));
        assert!(!needs_cookies_upfront("https://www.bilibili.com/video/BV1xx411c7mD"));
    }

    #[test]
    fn instagram_y_redes_si_reciben_cookies_de_entrada() {
        assert!(needs_cookies_upfront("https://www.instagram.com/p/Cxyz/"));
        assert!(needs_cookies_upfront("https://instagram.com/usuario"));
        assert!(needs_cookies_upfront("https://weibo.com/u/1234567"));
        assert!(needs_cookies_upfront("https://m.weibo.cn/status/123"));
        assert!(needs_cookies_upfront("https://x.com/user/status/1"));
        // Un dominio que solo *contiene* el nombre no cuenta
        assert!(!needs_cookies_upfront("https://instagram.com.atacante.net/p/1"));
    }

    #[test]
    fn errores_de_autenticacion_se_distinguen_de_los_de_formato() {
        assert!(needs_auth_error("ERROR: [youtube] abc: Sign in to confirm you're not a bot"));
        assert!(needs_auth_error("ERROR: Private video. Sign in if you've been granted access"));
        assert!(needs_auth_error("ERROR: unable to download: HTTP Error 401: Unauthorized"));
        assert!(needs_auth_error("ERROR: This video is age-restricted"));

        // El error que provocaba el bug NO es de autenticación: si lo fuera,
        // reintentaríamos con cookies y volveríamos a caer en la misma trampa.
        assert!(!needs_auth_error(
            "ERROR: [youtube] abc: Requested format is not available. Use --list-formats"
        ));
        assert!(!needs_auth_error("ERROR: Unsupported URL: https://passport.weibo.com/"));
    }

    #[test]
    fn is_cookie_error_no_confunde_formato_con_cookies() {
        assert!(is_cookie_error("Failed to decrypt with DPAPI"));
        assert!(is_cookie_error("could not copy the cookie database"));
        assert!(!is_cookie_error("ERROR: Requested format is not available"));
        assert!(!is_cookie_error("ERROR: HTTP Error 403: Forbidden"));
    }

    #[test]
    fn sin_ffmpeg_nunca_se_piden_flujos_separados() {
        // Pedir bv*+ba sin fusionador aborta con OSError [Errno 2]
        let sin = format_selector(false);
        assert!(!sin.contains('+'), "selector sin ffmpeg no debe fusionar: {sin}");
        assert_eq!(sin, "b");
        // Con ffmpeg sí, que es como se pasa de 720p en YouTube
        assert_eq!(format_selector(true), "bv*+ba/b");
    }

    #[test]
    fn el_muro_de_fotos_solo_se_propone_para_perfiles_de_weibo() {
        assert_eq!(
            weibo_album_url("https://weibo.com/u/2304291523?tabtype=feed").as_deref(),
            Some("https://weibo.com/u/2304291523?tabtype=album")
        );
        assert_eq!(
            weibo_album_url("https://weibo.com/u/2304291523").as_deref(),
            Some("https://weibo.com/u/2304291523?tabtype=album")
        );
        // Ya se probó el álbum: no se reintenta en bucle
        assert_eq!(weibo_album_url("https://weibo.com/u/123?tabtype=album"), None);
        // Y la vuelta atrás solo aplica desde el álbum, nunca al revés
        assert_eq!(
            weibo_feed_url("https://weibo.com/u/123?tabtype=album").as_deref(),
            Some("https://weibo.com/u/123?tabtype=feed")
        );
        assert_eq!(weibo_feed_url("https://weibo.com/u/123?tabtype=feed"), None);
        assert_eq!(weibo_feed_url("https://www.instagram.com/alguien/"), None);
        // No es un perfil
        assert_eq!(weibo_album_url("https://weibo.com/tv/show/1034:53278"), None);
        // Otros sitios
        assert_eq!(weibo_album_url("https://www.instagram.com/alguien/"), None);
        // Dominio impostor
        assert_eq!(weibo_album_url("https://weibo.com.atacante.net/u/1"), None);
    }

    #[test]
    fn author_from_url_agrupa_perfiles() {
        // El caso que dejaba todo suelto en la raíz
        assert_eq!(author_from_url("https://www.instagram.com/fate_stay_art/posts/"), "fate_stay_art");
        assert_eq!(author_from_url("https://instagram.com/fate_stay_art"), "fate_stay_art");
        assert_eq!(author_from_url("https://www.tiktok.com/@usuario/video/123"), "usuario");
        assert_eq!(author_from_url("https://x.com/alguien/status/1"), "alguien");
        assert_eq!(author_from_url("https://weibo.com/u/1234567"), "weibo_1234567");
        assert_eq!(author_from_url("https://www.youtube.com/@canal/videos"), "canal");
    }

    #[test]
    fn author_from_url_no_inventa_carpetas() {
        // Posts sueltos: no hay perfil, no debe crearse carpeta
        assert_eq!(author_from_url("https://www.instagram.com/p/Cxyz/"), "");
        assert_eq!(author_from_url("https://www.instagram.com/reel/Cxyz/"), "");
        assert_eq!(author_from_url("https://www.instagram.com/stories/x/1"), "");
        // Vídeos de YouTube: «watch» no es un usuario
        assert_eq!(author_from_url("https://www.youtube.com/watch?v=95wP2VKGEXE"), "");
        assert_eq!(author_from_url("https://youtu.be/95wP2VKGEXE"), "");
        // Weibo sin /u/ y sitios sin concepto de perfil
        assert_eq!(author_from_url("https://weibo.com/tv/show/1034:53278"), "");
        assert_eq!(author_from_url("https://www.bilibili.com/video/BV1xx411c7mD"), "");
        assert_eq!(author_from_url("https://cdn.example.com/a/b/c.mp4"), "");
        // Ni URLs raras ni dominios impostores
        assert_eq!(author_from_url("https://instagram.com.atacante.net/victima/"), "");
        assert_eq!(author_from_url("no-es-una-url"), "");
        assert_eq!(author_from_url("https://www.instagram.com/"), "");
    }

    #[test]
    fn author_from_url_rechaza_segmentos_no_plausibles() {
        // Nada que pueda escaparse de la carpeta de destino ni reventar la ruta
        assert_eq!(author_from_url("https://instagram.com/..%2F..%2Fetc/"), "");
        assert_eq!(author_from_url("https://instagram.com/a b c/"), "");
        let largo = "x".repeat(60);
        assert_eq!(author_from_url(&format!("https://instagram.com/{largo}/")), "");
    }

    #[test]
    fn sanitize_neutraliza_nombres_peligrosos() {
        assert!(!sanitize("../../etc/passwd", 60).contains('/'));
        assert!(!sanitize("a\\b:c*d?e", 60).contains('\\'));
        assert_eq!(sanitize("CON", 60), "_CON");
        assert_eq!(sanitize("nul.txt", 60), "_nul.txt");
        assert_eq!(sanitize("   ", 60), "video");
        assert_eq!(sanitize("nombre.", 60), "nombre");
    }
}
