//! Kemono y Coomer: listar un creador o un post por su API — By Eric V. Gramunt
//!
//! Los dos sitios son archivos públicos de contenido de Patreon, Fanbox,
//! Fantia, Gumroad, Boosty, SubscribeStar, DLsite y Discord (Kemono) y de
//! OnlyFans, Fansly y CandFans (Coomer). Comparten el mismo software y por
//! tanto la misma API, que está documentada en `kemono.cr/documentation/api`
//! y es ABIERTA: no pide cuenta, ni cookies, ni clave.
//!
//! POR QUÉ NO SE DELEGA EN gallery-dl. Su extractor `kemono` funciona y se
//! conserva como respaldo —y como motor para descargar un creador ENTERO desde
//! Descargas—, pero para la rejilla de Perfil tiene tres pegas:
//!
//! 1. Recorre al creador de principio a fin en una pasada. Un creador con 542
//!    publicaciones de cuarenta imágenes son veinte mil fichas de golpe; aquí
//!    se trae de cincuenta en cincuenta publicaciones y «Cargar más» pide la
//!    siguiente tanda.
//! 2. Cada arranque descomprime un Python entero; la API contesta en menos de
//!    un segundo y una sola petición trae la página completa.
//! 3. Su listado no trae miniaturas. El sitio sí las sirve, en
//!    `img.kemono.cr/thumbnail/data/…`, y con ellas la rejilla deja de bajar
//!    PNG de varios megas para pintar un recuadro de 180 píxeles.
//!
//! LA CABECERA `Accept: text/css` NO ES UN ERROR. Sin ella la API responde 403
//! con un texto que lo dice a las claras: «If you want to scrape, use
//! "Accept: text/css" header in your requests for now». Es la forma que tiene
//! el sitio de distinguir a un cliente que ha leído su documentación de un
//! navegador que ha caído ahí por casualidad; gallery-dl manda exactamente la
//! misma. Está MEDIDO contra el sitio: sin la cabecera, 403; con ella, 200.
//!
//! Los archivos viven en `{root}/data{path}`, con el `path` empezando por
//! dos niveles de hash (`/c8/90/c890…be.png`). Ese enlace responde con una
//! redirección al servidor de ficheros que toque (`n2.kemono.cr`, …) y NO
//! caduca: la ruta es el hash del contenido, así que vale hoy y dentro de un
//! año. Por eso la rejilla no enseña el aviso de «los enlaces caducan».

use crate::gallery::GalleryItem;
use serde_json::Value;

/// Valor de `Accept` que la API exige. Ver el comentario del módulo.
pub const ACCEPT: &str = "text/css";

/// Publicaciones por página de la API. No es configurable: el servidor
/// siempre devuelve cincuenta y pagina por `?o=` (offset).
pub const POR_PAGINA: u32 = 50;

/// Los dos sitios, que son el mismo software en dos dominios.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Sitio {
    Kemono,
    Coomer,
}

impl Sitio {
    pub const ALL: [Sitio; 2] = [Sitio::Kemono, Sitio::Coomer];

    pub fn nombre(self) -> &'static str {
        match self {
            Sitio::Kemono => "Kemono",
            Sitio::Coomer => "Coomer",
        }
    }

    /// Dominio canónico. Los dos han cambiado de dominio varias veces
    /// (`.party` → `.su` → `.cr`/`.st`), así que cualquiera de los antiguos se
    /// reconoce al clasificar y se reescribe a este al pedir.
    pub fn root(self) -> &'static str {
        match self {
            Sitio::Kemono => "https://kemono.cr",
            Sitio::Coomer => "https://coomer.st",
        }
    }

    /// Servidor de miniaturas. Comprobado: `img.kemono.cr/thumbnail/data/…`
    /// responde `image/png` con la imagen reducida (45 KB donde el original
    /// pesa varios megas).
    pub fn img(self) -> &'static str {
        match self {
            Sitio::Kemono => "https://img.kemono.cr",
            Sitio::Coomer => "https://img.coomer.st",
        }
    }

    /// Servicios que indexa cada sitio, para el filtro de la pestaña. Los
    /// números salen de la lista de creadores del 21 de octubre de 2026:
    /// Patreon 63.790, Fanbox 23.870, Gumroad 12.541, Discord 4.757, Fantia
    /// 2.031, Boosty 732, SubscribeStar 241, DLsite 101.
    pub fn servicios(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Sitio::Kemono => &[
                ("patreon", "Patreon"),
                ("fanbox", "Fanbox"),
                ("fantia", "Fantia"),
                ("gumroad", "Gumroad"),
                ("boosty", "Boosty"),
                ("subscribestar", "SubscribeStar"),
                ("dlsite", "DLsite"),
                ("afdian", "Afdian"),
                ("discord", "Discord"),
            ],
            Sitio::Coomer => &[
                ("onlyfans", "OnlyFans"),
                ("fansly", "Fansly"),
                ("candfans", "CandFans"),
            ],
        }
    }

    /// Nombre legible de un servicio, o la clave tal cual si no está en la
    /// tabla: la lista de creadores puede traer uno nuevo antes que nosotros.
    pub fn servicio_legible(self, clave: &str) -> &str {
        Sitio::ALL
            .iter()
            .flat_map(|s| s.servicios().iter())
            .find(|(k, _)| *k == clave)
            .map(|(_, n)| *n)
            .unwrap_or(clave)
    }

    fn de_host(host: &str) -> Option<Sitio> {
        let h = host
            .strip_prefix("www.")
            .or_else(|| host.strip_prefix("beta."))
            .unwrap_or(host);
        match h {
            "kemono.cr" | "kemono.su" | "kemono.st" | "kemono.party" => Some(Sitio::Kemono),
            "coomer.st" | "coomer.su" | "coomer.cr" | "coomer.party" => Some(Sitio::Coomer),
            _ => None,
        }
    }
}

/// Qué es la URL que ha pegado el usuario.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Destino {
    /// Todas las publicaciones de un creador, opcionalmente filtradas por
    /// texto (`?q=`) o etiqueta (`?tag=`), que la API acepta tal cual.
    Creador {
        sitio: Sitio,
        service: String,
        user: String,
        q: String,
        tag: String,
    },
    /// Una publicación suelta.
    Post {
        sitio: Sitio,
        service: String,
        user: String,
        post: String,
    },
    /// Del sitio, pero no de las dos formas de arriba: un servidor de Discord,
    /// la portada, la búsqueda global. gallery-dl sabe de algunas; aquí no se
    /// intenta y se deja que siga por su camino de siempre.
    Otro(Sitio),
}

impl Destino {
    pub fn sitio(&self) -> Sitio {
        match self {
            Destino::Creador { sitio, .. } | Destino::Post { sitio, .. } | Destino::Otro(sitio) => {
                *sitio
            }
        }
    }

    /// ¿Se puede listar desde aquí, sin gallery-dl?
    pub fn nativo(&self) -> bool {
        !matches!(self, Destino::Otro(_))
    }

    /// Página de la publicación o del creador en el sitio, con el dominio
    /// canónico y, en un creador, los filtros que llevara.
    ///
    /// Es la URL que se guarda como la de la galería: quien pega un enlace de
    /// `kemono.su` o `kemono.party` ve cómo se reescribe al dominio actual,
    /// que es el que responde.
    pub fn url_web(&self) -> String {
        match self {
            Destino::Creador {
                sitio,
                service,
                user,
                q,
                tag,
            } => {
                let mut u = format!("{}/{service}/user/{user}", sitio.root());
                let mut params: Vec<String> = Vec::new();
                if !q.trim().is_empty() {
                    params.push(format!("q={}", query_escape(q.trim())));
                }
                if !tag.trim().is_empty() {
                    params.push(format!("tag={}", query_escape(tag.trim())));
                }
                if !params.is_empty() {
                    u.push('?');
                    u.push_str(&params.join("&"));
                }
                u
            }
            Destino::Post {
                sitio,
                service,
                user,
                post,
            } => {
                format!("{}/{service}/user/{user}/post/{post}", sitio.root())
            }
            Destino::Otro(sitio) => sitio.root().to_string(),
        }
    }

    /// `GET /api/v1/{service}/user/{id}/profile`: nombre y número de posts.
    pub fn api_perfil(&self) -> Option<String> {
        match self {
            Destino::Creador {
                sitio,
                service,
                user,
                ..
            }
            | Destino::Post {
                sitio,
                service,
                user,
                ..
            } => Some(format!(
                "{}/api/v1/{service}/user/{user}/profile",
                sitio.root()
            )),
            Destino::Otro(_) => None,
        }
    }

    /// `GET /api/v1/{service}/user/{id}/posts?o=…`: una página de cincuenta.
    pub fn api_posts(&self, pagina: u32) -> Option<String> {
        let Destino::Creador {
            sitio,
            service,
            user,
            q,
            tag,
        } = self
        else {
            return None;
        };
        let offset = pagina.saturating_sub(1) * POR_PAGINA;
        let mut u = format!(
            "{}/api/v1/{service}/user/{user}/posts?o={offset}",
            sitio.root()
        );
        if !q.trim().is_empty() {
            u.push_str("&q=");
            u.push_str(&query_escape(q.trim()));
        }
        if !tag.trim().is_empty() {
            u.push_str("&tag=");
            u.push_str(&query_escape(tag.trim()));
        }
        Some(u)
    }

    /// `GET /api/v1/{service}/user/{id}/post/{post}`: la publicación entera,
    /// con su `content` (de donde salen las imágenes incrustadas).
    pub fn api_post(&self) -> Option<String> {
        let Destino::Post {
            sitio,
            service,
            user,
            post,
        } = self
        else {
            return None;
        };
        Some(format!(
            "{}/api/v1/{service}/user/{user}/post/{post}",
            sitio.root()
        ))
    }
}

/// Escapa lo mínimo para que un término de búsqueda viaje en una query.
fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn host_de(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// ¿Es una URL de Kemono o de Coomer, sea del tipo que sea?
pub fn es_kemono(url: &str) -> bool {
    clasificar(url).is_some()
}

/// ¿Es un ARCHIVO del sitio, y no una página?
///
/// `kemono.cr/data/…`, el servidor al que redirige (`n2.kemono.cr/data/…`) y
/// las miniaturas (`img.kemono.cr/thumbnail/…`). Importa porque el enrutado
/// mira los hosts de galería ANTES que la extensión, y sin esto cada archivo
/// marcado en la rejilla se habría mandado a gallery-dl en vez de al motor
/// HTTP nativo, que es el que reanuda y el que abre varias conexiones.
pub fn es_archivo(url: &str) -> bool {
    let low = url.trim().to_ascii_lowercase();
    let Some(host) = host_de(&low) else {
        return false;
    };
    let de_los_sitios = [
        "kemono.cr",
        "kemono.su",
        "kemono.party",
        "coomer.st",
        "coomer.su",
        "coomer.party",
    ]
    .iter()
    .any(|d| host == *d || host.ends_with(&format!(".{d}")));
    if !de_los_sitios {
        return false;
    }
    let ruta = low.split_once("://").map(|(_, r)| r).unwrap_or("");
    let ruta = ruta.split(['?', '#']).next().unwrap_or("");
    let ruta = ruta.split_once('/').map(|(_, r)| r).unwrap_or("");
    ruta.starts_with("data/") || ruta.starts_with("thumbnail/")
}

/// ¿De qué sitio es un ARCHIVO? Admite subdominios (`n2.kemono.cr`,
/// `img.coomer.st`), al contrario que `clasificar`, que solo reconoce las
/// páginas del dominio principal.
pub fn sitio_de_archivo(url: &str) -> Option<Sitio> {
    let low = url.trim().to_ascii_lowercase();
    let host = host_de(&low)?;
    let es = |dominios: &[&str]| {
        dominios
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    };
    if es(&["kemono.cr", "kemono.su", "kemono.st", "kemono.party"]) {
        Some(Sitio::Kemono)
    } else if es(&["coomer.st", "coomer.su", "coomer.cr", "coomer.party"]) {
        Some(Sitio::Coomer)
    } else {
        None
    }
}

/// Extensiones con vista previa en el servidor de miniaturas. Es el mismo
/// criterio que usa gallery-dl (`util.EXTS_IMAGE`): un vídeo o un zip no
/// tienen miniatura, y pedirla devolvería un 404.
const CON_MINIATURA: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "avif", "bmp"];

/// La vista previa de un archivo: `https://img.<sitio>/thumbnail/data/…`.
///
/// POR QUÉ EXISTE. Desde junio de 2026 los servidores de ficheros originales
/// de Kemono y Coomer (`n1…n4.kemono.cr`, `n3.coomer.st`) no entregan nada:
/// conexión cortada o `503 upstream connect error … connection timeout`.
/// gallery-dl lo documenta (mikf/gallery-dl#9442: «it's currently impossible
/// to download the actual files») y desde la 1.32.3 baja por defecto esta
/// misma miniatura. Medido el 24 de octubre de 2026: el original sigue en
/// 503 y la miniatura responde 200, un JPEG de 533×800 en Kemono y de 449×800
/// en Coomer. No es el original; es lo único que hay mientras dure la avería.
///
/// `None` si no es un archivo del sitio o si no es una imagen.
pub fn url_miniatura(url: &str) -> Option<String> {
    let sitio = sitio_de_archivo(url)?;
    let tras_esquema = url.trim().split_once("://")?.1;
    let ruta = tras_esquema.split(['?', '#']).next()?;
    let ruta = &ruta[ruta.find('/')?..];
    let ruta = ruta.strip_prefix("/thumbnail").unwrap_or(ruta);
    let ruta = ruta.strip_prefix("/data")?;
    if !es_ruta_hash(ruta) || !CON_MINIATURA.contains(&extension_de(ruta).as_str()) {
        return None;
    }
    Some(format!("{}/thumbnail/data{ruta}", sitio.img()))
}

/// Nombre con el que se guarda la vista previa: el del original, sin su
/// extensión, con `_preview.jpg`. Que se vea en el nombre es la mitad del
/// arreglo: un JPEG de 533 píxeles guardado como `….png` sería un fallo
/// silencioso de los que este proyecto lleva tiempo erradicando.
pub fn nombre_preview(nombre: &str) -> String {
    let base = match nombre.rsplit_once('.') {
        Some((b, e)) if !b.is_empty() && !e.is_empty() && e.len() <= 12 && !e.contains(['/', '\\']) => b,
        _ => nombre,
    };
    format!("{base}_preview.jpg")
}

/// Cuánto se recuerda que los originales de un sitio no responden.
///
/// Diez minutos: con la avería activa, cada intento cuesta hasta 15 s de
/// espera del servidor, y cuarenta archivos en cola eran diez minutos
/// mirando filas al 0 %. Si el sitio se recupera, en diez minutos se vuelve
/// a probar el original sin que nadie tenga que tocar nada.
pub const CAIDA_SEGUNDOS: u64 = 10 * 60;

/// Hasta cuándo (segundos de época) se dan por caídos los originales de cada
/// sitio. 0 = no se sabe de ninguna caída. Atómico porque lo leen y escriben
/// las tareas de descarga en paralelo.
static CAIDOS_HASTA: [std::sync::atomic::AtomicU64; 2] =
    [std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0)];

fn indice(sitio: Sitio) -> usize {
    match sitio {
        Sitio::Kemono => 0,
        Sitio::Coomer => 1,
    }
}

fn ahora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Anota que los originales del sitio no responden, desde `ahora`.
pub fn marcar_originales_caidos_en(sitio: Sitio, ahora: u64) {
    CAIDOS_HASTA[indice(sitio)].store(ahora + CAIDA_SEGUNDOS, std::sync::atomic::Ordering::Relaxed);
}

/// ¿Están dados por caídos los originales del sitio en `ahora`?
pub fn originales_caidos_en(sitio: Sitio, ahora: u64) -> bool {
    ahora < CAIDOS_HASTA[indice(sitio)].load(std::sync::atomic::Ordering::Relaxed)
}

/// Olvida una caída: el original acaba de bajar bien.
pub fn marcar_originales_vivos(sitio: Sitio) {
    CAIDOS_HASTA[indice(sitio)].store(0, std::sync::atomic::Ordering::Relaxed);
}

pub fn marcar_originales_caidos(sitio: Sitio) {
    marcar_originales_caidos_en(sitio, ahora());
}

pub fn originales_caidos(sitio: Sitio) -> bool {
    originales_caidos_en(sitio, ahora())
}

/// ¿Este error del original significa que el SERVIDOR no responde, y no que
/// el archivo no exista o que se haya pedido demasiado deprisa?
///
/// Conexión cortada, timeout o 5xx: avería, y cuenta para el disyuntor. Un 404
/// es de ese archivo concreto, y un 429 es ritmo: ninguno de los dos dice nada
/// de los demás archivos de la cola.
pub fn error_es_caida(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    if e.contains("http 4") {
        return false;
    }
    e.contains("http 5")
        || e.contains("error sending request")
        || e.contains("connection")
        || e.contains("timed out")
        || e.contains("timeout")
        || e.contains("dns")
        || e.contains("reset")
}

/// Reconoce la URL. `None` si no es de ninguno de los dos sitios.
///
/// Comparación por host, no por subcadena: `kemono.cr.atacante.net` no es
/// Kemono, y una ruta que contenga la palabra tampoco.
pub fn clasificar(url: &str) -> Option<Destino> {
    let url = url.trim();
    let low = url.to_ascii_lowercase();
    let host = host_de(&low)?;
    let sitio = Sitio::de_host(host)?;

    // La ruta, sin query ni fragmento, en segmentos.
    let tras_host = low.split_once("://")?.1;
    let ruta = tras_host.split(['?', '#']).next().unwrap_or("");
    let ruta = ruta.split_once('/').map(|(_, r)| r).unwrap_or("");
    let seg: Vec<&str> = ruta.split('/').filter(|s| !s.is_empty()).collect();

    // /{service}/user/{id}[/post/{pid}]
    if seg.len() >= 3 && seg[1] == "user" && seg[0] != "discord" {
        let service = seg[0].to_string();
        let user = seg[2].to_string();
        if seg.len() >= 5 && seg[3] == "post" {
            return Some(Destino::Post {
                sitio,
                service,
                user,
                post: seg[4].to_string(),
            });
        }
        if seg.len() == 3 {
            // Los filtros se leen de la URL ORIGINAL, no de la pasada a
            // minúsculas: un término de búsqueda con mayúsculas es del usuario.
            let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
            let query = query.split('#').next().unwrap_or("");
            let mut q = String::new();
            let mut tag = String::new();
            for par in query.split('&') {
                let (k, v) = par.split_once('=').unwrap_or((par, ""));
                let v = query_unescape(v);
                match k {
                    "q" => q = v,
                    "tag" => tag = v,
                    _ => {}
                }
            }
            return Some(Destino::Creador {
                sitio,
                service,
                user,
                q,
                tag,
            });
        }
    }
    Some(Destino::Otro(sitio))
}

/// Lo contrario de `query_escape`, para leer `?q=` de una URL pegada.
fn query_unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Lo que dice `/profile` de un creador.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Perfil {
    pub nombre: String,
    pub post_count: u64,
}

pub fn perfil_de_json(txt: &str) -> Result<Perfil, String> {
    let v: Value = serde_json::from_str(txt).map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    Ok(Perfil {
        nombre: texto(&v, "name"),
        post_count: v.get("post_count").and_then(|n| n.as_u64()).unwrap_or(0),
    })
}

fn texto(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Extensión de un nombre o de una ruta, en minúsculas y sin el punto.
fn extension_de(nombre: &str) -> String {
    let base = nombre.rsplit('/').next().unwrap_or(nombre);
    match base.rsplit_once('.') {
        Some((_, e))
            if !e.is_empty() && e.len() <= 12 && e.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            e.to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

/// Rutas de imágenes incrustadas en el HTML de `content`.
///
/// El sitio las escribe como `<img src="/ab/cd/hash.jpg">` o con el dominio
/// delante; gallery-dl las llama «inline» y las descarga aparte de los
/// adjuntos. Las de `/inline/…` son el formato antiguo y también se aceptan.
fn incrustadas(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut resto = content;
    while let Some(i) = resto.find("src=\"") {
        resto = &resto[i + 5..];
        let Some(fin) = resto.find('"') else { break };
        let src = &resto[..fin];
        resto = &resto[fin..];
        // Quitar el dominio si lo lleva: la ruta es lo único que importa, y
        // se vuelve a montar sobre el root canónico.
        let path = if let Some(rest) = src.split_once("://") {
            match rest.1.find('/') {
                Some(p) => &rest.1[p..],
                None => continue,
            }
        } else {
            src
        };
        let path = path.strip_prefix("/data").unwrap_or(path);
        if path.starts_with("/inline/") || es_ruta_hash(path) {
            out.push(path.to_string());
        }
    }
    out
}

/// `/xx/xx/<64 hex>.ext`: la forma de toda ruta de archivo del sitio.
fn es_ruta_hash(path: &str) -> bool {
    let seg: Vec<&str> = path.split('/').collect();
    if seg.len() != 4 || !seg[0].is_empty() {
        return false;
    }
    let hex2 = |s: &str| s.len() == 2 && s.chars().all(|c| c.is_ascii_hexdigit());
    let hash = seg[3].split('.').next().unwrap_or("");
    hex2(seg[1]) && hex2(seg[2]) && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// Un archivo de una publicación, antes de convertirse en ficha.
struct Archivo {
    nombre: String,
    path: String,
}

fn archivo_de(v: &Value) -> Option<Archivo> {
    let path = texto(v, "path");
    if path.is_empty() {
        return None;
    }
    // Alguna publicación antigua trae barras invertidas en la ruta.
    let path = path.replace('\\', "/");
    let nombre = texto(v, "name");
    let nombre = if nombre.is_empty() {
        path.rsplit('/').next().unwrap_or("").to_string()
    } else {
        nombre
    };
    Some(Archivo { nombre, path })
}

/// Las fichas de UNA publicación, en el orden del sitio y sin repetidos.
///
/// `file` y `attachments` se solapan casi siempre: el `file` es la portada y
/// vuelve a aparecer como primer adjunto. Se deduplica por ruta, que es el
/// hash del contenido, así que dos entradas con la misma ruta son el mismo
/// archivo aunque se llamen distinto.
pub fn items_de_post(post: &Value, sitio: Sitio, autor: &str) -> Vec<GalleryItem> {
    let post = post.get("post").unwrap_or(post);
    let id = texto(post, "id");
    let user = texto(post, "user");
    let service = texto(post, "service");
    let titulo = texto(post, "title");
    let fecha = {
        let p = texto(post, "published");
        if p.is_empty() {
            texto(post, "added")
        } else {
            p
        }
    };
    let post_url = format!("{}/{service}/user/{user}/post/{id}", sitio.root());

    let mut archivos: Vec<Archivo> = Vec::new();
    if let Some(f) = post.get("file").and_then(archivo_de) {
        archivos.push(f);
    }
    if let Some(arr) = post.get("attachments").and_then(|a| a.as_array()) {
        archivos.extend(arr.iter().filter_map(archivo_de));
    }
    for path in incrustadas(&texto(post, "content")) {
        let nombre = path.rsplit('/').next().unwrap_or("").to_string();
        archivos.push(Archivo { nombre, path });
    }

    let mut vistos = std::collections::HashSet::new();
    let unicos: Vec<Archivo> = archivos
        .into_iter()
        .filter(|a| vistos.insert(a.path.clone()))
        .collect();
    let total = unicos.len() as u32;

    unicos
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            let ext = {
                let e = extension_de(&a.nombre);
                if e.is_empty() {
                    extension_de(&a.path)
                } else {
                    e
                }
            };
            let es_video = crate::gallery::es_extension_de_video(&ext);
            let es_imagen = matches!(
                ext.as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "avif" | "bmp"
            );
            GalleryItem {
                url: format!("{}/data{}", sitio.root(), a.path),
                filename: a.nombre,
                ext,
                width: 0,
                height: 0,
                filesize: 0,
                is_video: es_video,
                post_id: id.clone(),
                index_in_post: i as u32 + 1,
                count_in_post: total,
                author: autor.to_string(),
                description: titulo.chars().take(160).collect(),
                date: fecha.clone(),
                post_url: post_url.clone(),
                // Solo las imágenes tienen miniatura en el sitio. Un vídeo o
                // un zip se quedan sin ella y la rejilla pinta su marcador.
                thumb_url: if es_imagen {
                    format!("{}/thumbnail/data{}", sitio.img(), a.path)
                } else {
                    String::new()
                },
                selected: false,
            }
        })
        .collect()
}

/// La publicación suelta: `GET …/post/{id}` devuelve `{"post": {…}, …}`.
pub fn items_de_post_json(
    txt: &str,
    sitio: Sitio,
    autor: &str,
) -> Result<Vec<GalleryItem>, String> {
    let v: Value = serde_json::from_str(txt).map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    Ok(items_de_post(&v, sitio, autor))
}

/// Una página del listado: `GET …/posts?o=N` devuelve un array de posts.
///
/// Las publicaciones del listado traen `file` y `attachments` completos, así
/// que no hace falta pedir cada una por separado. Lo que NO traen es el
/// `content` íntegro (viene `substring`, un recorte), de modo que las imágenes
/// incrustadas de una publicación solo aparecen al abrirla suelta.
pub fn items_de_listado(txt: &str, sitio: Sitio, autor: &str) -> Result<Vec<GalleryItem>, String> {
    let v: Value = serde_json::from_str(txt).map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    // La búsqueda global (`/api/v1/posts`) envuelve el array en `posts`; el
    // listado de un creador lo devuelve a pelo. Se aceptan las dos formas.
    let arr = v
        .as_array()
        .or_else(|| v.get("posts").and_then(|p| p.as_array()))
        .ok_or_else(|| {
            if crate::i18n::lang() == crate::i18n::Lang::Es {
                "respuesta inesperada de la API".to_string()
            } else {
                "unexpected reply from the API".to_string()
            }
        })?;
    Ok(arr
        .iter()
        .flat_map(|p| items_de_post(p, sitio, autor))
        .collect())
}

/// Un creador de la lista pública `GET /api/v1/creators`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creador {
    pub id: String,
    pub nombre: String,
    pub service: String,
    /// Cuántos usuarios lo tienen en favoritos: el único indicio de
    /// popularidad que da la API, y por eso el orden de los resultados.
    pub favoritos: u64,
    /// Última actualización, en segundos de época.
    pub actualizado: u64,
}

impl Creador {
    pub fn url(&self, sitio: Sitio) -> String {
        format!("{}/{}/user/{}", sitio.root(), self.service, self.id)
    }
}

/// Parsea la lista de creadores. Son unas 108.000 entradas y 3,4 MB en
/// Kemono (5,9 MB en Coomer), medido el 21 de octubre de 2026: se baja una
/// vez por sesión y se busca en memoria.
pub fn creadores_de_json(txt: &str) -> Result<Vec<Creador>, String> {
    let v: Value = serde_json::from_str(txt).map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    let arr = v
        .as_array()
        .ok_or_else(|| "respuesta inesperada".to_string())?;
    Ok(arr
        .iter()
        .filter_map(|c| {
            let id = texto(c, "id");
            let nombre = texto(c, "name");
            if id.is_empty() || nombre.is_empty() {
                return None;
            }
            Some(Creador {
                id,
                nombre,
                service: texto(c, "service"),
                favoritos: c.get("favorited").and_then(|n| n.as_u64()).unwrap_or(0),
                actualizado: c.get("updated").and_then(|n| n.as_u64()).unwrap_or(0),
            })
        })
        .collect())
}

/// Busca por nombre (subcadena, sin distinguir mayúsculas), filtra por
/// servicio si se pide, y ordena por favoritos de más a menos.
///
/// Los que EMPIEZAN por el término van antes que los que solo lo contienen:
/// buscando «miku» se quiere a «Miku» por delante de «Nekomiku», aunque el
/// segundo tenga más favoritos.
pub fn buscar<'a>(
    lista: &'a [Creador],
    termino: &str,
    service: &str,
    max: usize,
) -> Vec<&'a Creador> {
    let t = termino.trim().to_lowercase();
    if t.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(&Creador, bool)> = lista
        .iter()
        .filter(|c| service.is_empty() || c.service == service)
        .filter_map(|c| {
            let n = c.nombre.to_lowercase();
            if n.contains(&t) || c.id == t {
                Some((c, n.starts_with(&t)))
            } else {
                None
            }
        })
        .collect();
    hits.sort_by(|(a, pa), (b, pb)| {
        pb.cmp(pa)
            .then(b.favoritos.cmp(&a.favoritos))
            .then(a.nombre.cmp(&b.nombre))
    });
    hits.into_iter().take(max).map(|(c, _)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const POST: &str = include_str!("testdata/kemono_post.json");
    const LISTADO: &str = include_str!("testdata/kemono_listado.json");

    #[test]
    fn reconoce_creador_post_y_dominios_viejos() {
        let c = clasificar("https://kemono.cr/patreon/user/135298579").unwrap();
        assert_eq!(
            c,
            Destino::Creador {
                sitio: Sitio::Kemono,
                service: "patreon".into(),
                user: "135298579".into(),
                q: String::new(),
                tag: String::new(),
            }
        );
        let p = clasificar("https://kemono.cr/patreon/user/135298579/post/121604542").unwrap();
        assert_eq!(
            p,
            Destino::Post {
                sitio: Sitio::Kemono,
                service: "patreon".into(),
                user: "135298579".into(),
                post: "121604542".into(),
            }
        );
        // Dominios antiguos y variantes: se reconocen y se reescriben al
        // canónico en las URLs que se construyen.
        for u in [
            "https://kemono.su/fanbox/user/123",
            "https://www.kemono.party/fanbox/user/123/",
            "https://beta.kemono.cr/fanbox/user/123",
        ] {
            let d = clasificar(u).unwrap_or_else(|| panic!("{u}"));
            assert!(d.nativo(), "{u}");
            assert_eq!(d.url_web(), "https://kemono.cr/fanbox/user/123", "{u}");
        }
        let co = clasificar("https://coomer.su/onlyfans/user/alguien").unwrap();
        assert_eq!(co.sitio(), Sitio::Coomer);
        assert_eq!(co.url_web(), "https://coomer.st/onlyfans/user/alguien");

        // Lo que es del sitio pero no se lista desde aquí.
        let d = clasificar("https://kemono.cr/discord/server/123/456").unwrap();
        assert!(!d.nativo());
        assert!(!clasificar("https://kemono.cr/").unwrap().nativo());
        assert!(!clasificar("https://kemono.cr/posts?q=x").unwrap().nativo());

        // Impostores: por host, nunca por subcadena.
        assert!(clasificar("https://kemono.cr.atacante.net/patreon/user/1").is_none());
        assert!(clasificar("https://ejemplo.com/kemono.cr/patreon/user/1").is_none());
        assert!(clasificar("https://notkemono.cr/patreon/user/1").is_none());
        assert!(!es_kemono("https://www.patreon.com/c/alguien"));

        // Archivos frente a páginas.
        assert!(es_archivo(
            "https://kemono.cr/data/c8/90/c890722a0ddf757cc84a8bc13002464557738cd70cb30079140344d4e9384cbe.png"
        ));
        assert!(es_archivo(
            "https://n2.kemono.cr/data/c8/90/x.png?f=nombre.png"
        ));
        assert!(es_archivo(
            "https://img.coomer.st/thumbnail/data/c8/90/x.png"
        ));
        assert!(!es_archivo("https://kemono.cr/patreon/user/135298579"));
        assert!(!es_archivo("https://kemono.cr.atacante.net/data/x.png"));
        assert!(!es_archivo("https://ejemplo.com/data/x.png"));
    }

    #[test]
    fn la_vista_previa_se_deriva_del_archivo_y_solo_para_imagenes() {
        let h = "/c8/90/c890722a0ddf757cc84a8bc13002464557738cd70cb30079140344d4e9384cbe";
        // Desde la portada, desde el servidor de ficheros y desde un dominio viejo.
        for u in [
            format!("https://kemono.cr/data{h}.png"),
            format!("https://n2.kemono.cr/data{h}.png?f=5-2-2025-1.png"),
            format!("https://kemono.su/data{h}.png"),
        ] {
            assert_eq!(
                url_miniatura(&u).as_deref(),
                Some(format!("https://img.kemono.cr/thumbnail/data{h}.png").as_str()),
                "{u}"
            );
        }
        assert_eq!(
            url_miniatura(&format!("https://n3.coomer.st/data{h}.jpg")).as_deref(),
            Some(format!("https://img.coomer.st/thumbnail/data{h}.jpg").as_str())
        );
        // Una miniatura ya es una miniatura: no se dobla el prefijo.
        assert_eq!(
            url_miniatura(&format!("https://img.kemono.cr/thumbnail/data{h}.png")).as_deref(),
            Some(format!("https://img.kemono.cr/thumbnail/data{h}.png").as_str())
        );
        // Sin miniatura: vídeo, zip, ruta que no es hash, otro sitio, página.
        assert!(url_miniatura(&format!("https://kemono.cr/data{h}.mp4")).is_none());
        assert!(url_miniatura(&format!("https://kemono.cr/data{h}.zip")).is_none());
        assert!(url_miniatura("https://kemono.cr/data/c8/90/corto.png").is_none());
        assert!(url_miniatura(&format!("https://otro.example/data{h}.png")).is_none());
        assert!(url_miniatura(&format!("https://kemono.cr.atacante.net/data{h}.png")).is_none());
        assert!(url_miniatura("https://kemono.cr/patreon/user/1/post/2").is_none());

        assert_eq!(sitio_de_archivo("https://n4.kemono.cr/data/x.png"), Some(Sitio::Kemono));
        assert_eq!(sitio_de_archivo("https://img.coomer.st/thumbnail/x"), Some(Sitio::Coomer));
        assert_eq!(sitio_de_archivo("https://notkemono.cr/data/x.png"), None);
    }

    #[test]
    fn el_nombre_de_la_vista_previa_dice_lo_que_es() {
        assert_eq!(
            nombre_preview("Thebardspirit_121604542_5-2-2025-1.png"),
            "Thebardspirit_121604542_5-2-2025-1_preview.jpg"
        );
        assert_eq!(nombre_preview("foto.JPEG"), "foto_preview.jpg");
        assert_eq!(nombre_preview("sin_extension"), "sin_extension_preview.jpg");
        assert_eq!(nombre_preview(".oculto"), ".oculto_preview.jpg");
    }

    #[test]
    fn el_disyuntor_recuerda_la_caida_diez_minutos_y_por_sitio() {
        // Coomer y no Kemono: los tests corren en paralelo y la descarga real
        // usa el reloj; con un sitio que ningún otro test toca no se pisan.
        let t0 = 1_800_000_000;
        marcar_originales_vivos(Sitio::Coomer);
        assert!(!originales_caidos_en(Sitio::Coomer, t0));
        marcar_originales_caidos_en(Sitio::Coomer, t0);
        assert!(originales_caidos_en(Sitio::Coomer, t0 + 1));
        assert!(originales_caidos_en(Sitio::Coomer, t0 + CAIDA_SEGUNDOS - 1));
        assert!(!originales_caidos_en(Sitio::Coomer, t0 + CAIDA_SEGUNDOS));
        marcar_originales_vivos(Sitio::Coomer);
        assert!(!originales_caidos_en(Sitio::Coomer, t0 + 1));

        // Qué cuenta como caída y qué no.
        for e in [
            "HTTP 503",
            "error sending request for url (https://n1.kemono.cr/data/…)",
            "operation timed out",
            "connection reset by peer",
        ] {
            assert!(error_es_caida(e), "{e}");
        }
        for e in ["HTTP 404", "HTTP 429", "HTTP 403", ""] {
            assert!(!error_es_caida(e), "{e}");
        }
    }

    #[test]
    fn los_filtros_de_la_url_llegan_a_la_api() {
        let d = clasificar("https://kemono.cr/patreon/user/135298579?q=Sango%20C.C.&tag=Frieren")
            .unwrap();
        let Destino::Creador { q, tag, .. } = &d else {
            panic!()
        };
        assert_eq!(q, "Sango C.C.");
        assert_eq!(tag, "Frieren");
        // Y la URL canónica conserva los filtros, escapados.
        assert_eq!(
            d.url_web(),
            "https://kemono.cr/patreon/user/135298579?q=Sango+C.C.&tag=Frieren"
        );
        assert_eq!(clasificar(&d.url_web()).unwrap(), d, "ida y vuelta");
        assert_eq!(
            d.api_posts(1).unwrap(),
            "https://kemono.cr/api/v1/patreon/user/135298579/posts?o=0&q=Sango+C.C.&tag=Frieren"
        );
        // La paginación va por offset de cincuenta: la página 3 empieza en 100.
        let d = clasificar("https://kemono.cr/patreon/user/135298579").unwrap();
        assert_eq!(
            d.api_posts(3).unwrap(),
            "https://kemono.cr/api/v1/patreon/user/135298579/posts?o=100"
        );
        assert_eq!(
            d.api_posts(0).unwrap(),
            "https://kemono.cr/api/v1/patreon/user/135298579/posts?o=0"
        );
        assert_eq!(
            d.api_perfil().unwrap(),
            "https://kemono.cr/api/v1/patreon/user/135298579/profile"
        );
        assert!(d.api_post().is_none());

        let p = clasificar("https://kemono.cr/patreon/user/135298579/post/121604542").unwrap();
        assert_eq!(
            p.api_post().unwrap(),
            "https://kemono.cr/api/v1/patreon/user/135298579/post/121604542"
        );
        assert!(p.api_posts(1).is_none());
    }

    /// La muestra es la respuesta real de `/post/121604542` recortada a cinco
    /// adjuntos reales, con el `file` repetido como primer adjunto (así viene del
    /// sitio) y dos imágenes incrustadas en el `content` más un vídeo.
    #[test]
    fn un_post_da_sus_archivos_sin_repetir_y_con_miniatura() {
        let items = items_de_post_json(POST, Sitio::Kemono, "Thebardspirit").unwrap();
        // file (1) + adjuntos (1 repetido + 4 nuevos + clip.mp4) + 2 inline = 8 únicos.
        assert_eq!(
            items.len(),
            8,
            "{:#?}",
            items.iter().map(|i| &i.url).collect::<Vec<_>>()
        );
        let urls: Vec<&str> = items.iter().map(|i| i.url.as_str()).collect();
        let unicas: std::collections::HashSet<&str> = urls.iter().copied().collect();
        assert_eq!(urls.len(), unicas.len(), "hay repetidos");

        let primero = &items[0];
        assert_eq!(
            primero.url,
            "https://kemono.cr/data/c8/90/c890722a0ddf757cc84a8bc13002464557738cd70cb30079140344d4e9384cbe.png"
        );
        assert_eq!(
            primero.thumb_url,
            "https://img.kemono.cr/thumbnail/data/c8/90/c890722a0ddf757cc84a8bc13002464557738cd70cb30079140344d4e9384cbe.png"
        );
        assert_eq!(primero.filename, "5-2-2025-1.png");
        assert_eq!(primero.ext, "png");
        assert!(!primero.is_video);
        assert_eq!(primero.post_id, "121604542");
        assert_eq!(
            primero.post_url,
            "https://kemono.cr/patreon/user/135298579/post/121604542"
        );
        assert_eq!(primero.author, "Thebardspirit");
        assert_eq!(primero.description, "[42 P] C.C.");
        assert_eq!(primero.date, "2025-02-06T15:00:11");
        assert_eq!(primero.index_in_post, 1);
        assert_eq!(primero.count_in_post, 8);

        // El vídeo: sin miniatura (el sitio no la sirve) y marcado como tal.
        let video = items.iter().find(|i| i.ext == "mp4").expect("clip.mp4");
        assert!(video.is_video);
        assert!(video.thumb_url.is_empty());

        // Las incrustadas: con y sin dominio delante, las dos acaban en el
        // root canónico y con nombre sacado de la ruta.
        assert!(urls.contains(
            &"https://kemono.cr/data/ab/cd/abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789.jpg"
        ));
        assert!(urls.contains(
            &"https://kemono.cr/data/12/34/1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef.png"
        ));
    }

    #[test]
    fn un_listado_da_los_archivos_de_cada_post() {
        let items = items_de_listado(LISTADO, Sitio::Kemono, "Thebardspirit").unwrap();
        assert!(items.len() >= 6, "{}", items.len());
        // Tres publicaciones distintas.
        let posts: std::collections::HashSet<&str> =
            items.iter().map(|i| i.post_id.as_str()).collect();
        assert_eq!(posts.len(), 3);
        // Dentro de una publicación, el `file` repetido como adjunto no sale dos veces.
        for p in &posts {
            let de_este: Vec<&GalleryItem> = items.iter().filter(|i| i.post_id == *p).collect();
            let rutas: std::collections::HashSet<&str> =
                de_este.iter().map(|i| i.url.as_str()).collect();
            assert_eq!(rutas.len(), de_este.len(), "post {p} con repetidos");
            assert!(de_este
                .iter()
                .all(|i| i.count_in_post == de_este.len() as u32));
        }
        // Ninguna ficha sin extensión ni sin fecha.
        assert!(items
            .iter()
            .all(|i| !i.ext.is_empty() && !i.date.is_empty()));

        // La forma envuelta de la búsqueda global también se entiende.
        let envuelto = format!("{{\"count\":3,\"posts\":{LISTADO}}}");
        assert_eq!(
            items_de_listado(&envuelto, Sitio::Kemono, "x")
                .unwrap()
                .len(),
            items.len()
        );

        // Un error de la API se propaga como texto, no como lista vacía.
        assert!(items_de_listado("{\"error\":\"Not Found\"}", Sitio::Kemono, "x").is_err());
    }

    #[test]
    fn el_perfil_trae_nombre_y_numero_de_posts() {
        let p = perfil_de_json(
            "{\"id\":\"135298579\",\"name\":\"Thebardspirit\",\"service\":\"patreon\",\"post_count\":542}",
        )
        .unwrap();
        assert_eq!(
            p,
            Perfil {
                nombre: "Thebardspirit".into(),
                post_count: 542
            }
        );
        assert!(perfil_de_json("{\"error\":\"Creator not found\"}").is_err());
    }

    #[test]
    fn la_busqueda_de_creadores_prefiere_el_prefijo_y_luego_los_favoritos() {
        let lista = creadores_de_json(
            r#"[
              {"id":"1","name":"Nekomiku","service":"patreon","indexed":1,"updated":2,"favorited":900},
              {"id":"2","name":"Miku Art","service":"fanbox","indexed":1,"updated":2,"favorited":10},
              {"id":"3","name":"miku","service":"patreon","indexed":1,"updated":2,"favorited":50},
              {"id":"4","name":"Otro","service":"patreon","indexed":1,"updated":2,"favorited":5000},
              {"id":"","name":"sin id","service":"patreon"}
            ]"#,
        )
        .unwrap();
        assert_eq!(lista.len(), 4, "la entrada sin id se descarta");
        let r = buscar(&lista, "MIKU", "", 10);
        let nombres: Vec<&str> = r.iter().map(|c| c.nombre.as_str()).collect();
        assert_eq!(nombres, ["miku", "Miku Art", "Nekomiku"]);
        // Filtro por servicio.
        let r = buscar(&lista, "miku", "fanbox", 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].url(Sitio::Kemono), "https://kemono.cr/fanbox/user/2");
        // Por identificador exacto también se encuentra.
        assert_eq!(buscar(&lista, "4", "", 10)[0].nombre, "Otro");
        // Vacío no devuelve la lista entera.
        assert!(buscar(&lista, "  ", "", 10).is_empty());
        assert_eq!(buscar(&lista, "miku", "", 2).len(), 2);
    }

    #[test]
    fn rutas_incrustadas_y_escape_de_query() {
        assert!(es_ruta_hash(
            "/c8/90/c890722a0ddf757cc84a8bc13002464557738cd70cb30079140344d4e9384cbe.png"
        ));
        assert!(!es_ruta_hash("/c8/90/corto.png"));
        assert!(!es_ruta_hash("https://otro.sitio/x.png"));
        let v = incrustadas(
            r#"<img src="https://cdn.otro.com/foto.jpg"><img src="/inline/abc.png"><img src="https://kemono.cr/data/c8/90/c890722a0ddf757cc84a8bc13002464557738cd70cb30079140344d4e9384cbe.png">"#,
        );
        assert_eq!(
            v,
            [
                "/inline/abc.png",
                "/c8/90/c890722a0ddf757cc84a8bc13002464557738cd70cb30079140344d4e9384cbe.png"
            ]
        );
        assert_eq!(query_escape("C.C. & más"), "C.C.+%26+m%C3%A1s");
        assert_eq!(query_unescape("C.C.+%26+m%C3%A1s"), "C.C. & más");
        assert_eq!(query_unescape("100%"), "100%");
        assert_eq!(extension_de("archivo.SAFETENSORS"), "safetensors");
        assert_eq!(extension_de("sin_extension"), "");
    }
}
