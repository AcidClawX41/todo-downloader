//! Threads (threads.com) sin navegador: un post suelto, entero, y las
//! publicaciones que Threads enseña de un perfil a quien no ha iniciado sesión.
//!
//! LO QUE SE MIDIÓ ANTES DE ESCRIBIR ESTO (24 de octubre de 2026). Ni gallery-dl
//! (1.33.0-dev) ni yt-dlp (2026.10.21) tienen extractor de Threads. Pero la
//! página de un post, pedida como la pide un navegador, ya trae en el HTML las
//! publicaciones con sus medios: los mismos `image_versions2`, `video_versions`
//! y `carousel_media` que lee el userscript, a resolución original (3072×4096
//! en la muestra) y con las URL firmadas del CDN. No hace falta reconstruir la
//! API de Meta ni su `doc_id`, que es lo que hacía inviable un extractor.
//!
//! LA TRAMPA: sin las cuatro cabeceras `Sec-Fetch-*` de una navegación,
//! threads.com responde `200` con la carcasa de su página de error
//! (`Barcelona404ErrorRoot`) y ni un solo medio. Ninguna de las cuatro basta
//! por separado. Con ellas, lo mismo con curl que con reqwest/rustls.
//!
//! EL LÍMITE: sin sesión, de un perfil Threads solo enseña las primeras
//! publicaciones (la consulta de la página pide `first: 4`, y al hacer scroll
//! no pide más: pide iniciar sesión). Para un perfil entero sigue estando el
//! userscript, que lee con la sesión del propio navegador. No se mandan aquí
//! las cookies de Meta del usuario a propósito: una sesión de Instagram usada
//! desde un cliente que no es un navegador es justo lo que Meta marca como
//! actividad sospechosa, y la cuenta es suya.

use crate::gallery::GalleryItem;
use serde_json::Value;

/// Qué se ha pegado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destino {
    /// `threads.com/@usuario`
    Perfil { usuario: String },
    /// `threads.com/@usuario/post/<código>`
    Post { usuario: String, codigo: String },
}

impl Destino {
    /// La dirección que se pide: siempre en `www.threads.com`, aunque se haya
    /// pegado la de `threads.net`, que solo redirige.
    pub fn url_web(&self) -> String {
        match self {
            Destino::Perfil { usuario } => format!("https://www.threads.com/@{usuario}"),
            Destino::Post { usuario, codigo } => format!("https://www.threads.com/@{usuario}/post/{codigo}"),
        }
    }

    pub fn usuario(&self) -> &str {
        match self {
            Destino::Perfil { usuario } | Destino::Post { usuario, .. } => usuario,
        }
    }
}

/// Clasifica una URL de Threads. `None` si no es un perfil ni un post.
pub fn clasificar(url: &str) -> Option<Destino> {
    let resto = url.trim().split_once("://")?.1;
    let (host, ruta) = resto.split_once('/').unwrap_or((resto, ""));
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if host != "threads.com" && host != "threads.net" {
        return None;
    }
    let ruta = ruta.split(['?', '#']).next().unwrap_or("");
    let mut segs = ruta.split('/').filter(|s| !s.is_empty());
    let usuario = segs.next()?.strip_prefix('@')?;
    // Los nombres de Threads son los de Instagram: letras, cifras, `.` y `_`.
    if usuario.is_empty()
        || usuario.len() > 30
        || !usuario.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
    {
        return None;
    }
    let usuario = usuario.to_string();
    match (segs.next(), segs.next()) {
        (Some("post"), Some(codigo)) => {
            let valido = (5..=40).contains(&codigo.len())
                && codigo.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            valido.then(|| Destino::Post { usuario, codigo: codigo.to_string() })
        }
        // `/@usuario`, `/@usuario/media`, `/@usuario/replies`…: el perfil.
        _ => Some(Destino::Perfil { usuario }),
    }
}

/// Cabeceras de una navegación de verdad. Sin las `Sec-Fetch-*` no hay
/// publicaciones en la respuesta: ver la cabecera del módulo.
pub const CABECERAS: &[(&str, &str)] = &[
    ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    ("Accept-Language", "en-US,en;q=0.9"),
    ("Sec-Fetch-Dest", "document"),
    ("Sec-Fetch-Mode", "navigate"),
    ("Sec-Fetch-Site", "none"),
    ("Sec-Fetch-User", "?1"),
    ("Upgrade-Insecure-Requests", "1"),
];

/// Lo que se ha sacado de una página.
#[derive(Debug, Default)]
pub struct Lectura {
    pub items: Vec<GalleryItem>,
    /// Publicaciones de esa cuenta presentes en la página, tengan medios o no
    /// (muchas son solo texto).
    pub publicaciones: usize,
    /// El perfil tiene más publicaciones de las que Threads ha enseñado.
    pub hay_mas: bool,
}

/// Lee las publicaciones de una página de Threads.
///
/// `None` = Threads no ha servido la página: la carcasa de error sin datos, que
/// es lo que llega cuando el post se ha borrado, la cuenta es privada o Meta no
/// ha tomado la petición por una navegación.
pub fn leer_html(html: &str, destino: &Destino) -> Option<Lectura> {
    let mut publicaciones: Vec<Value> = Vec::new();
    for bloque in bloques_json(html) {
        if !bloque.contains("image_versions2") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(bloque) {
            let mut halladas = Vec::new();
            cosechar(&v, &mut halladas, 0);
            publicaciones.extend(halladas.into_iter().cloned());
        }
    }
    if publicaciones.is_empty() && html.contains("Barcelona404ErrorRoot") {
        return None;
    }
    if publicaciones.is_empty() && !html.contains(&format!("\"username\":\"{}\"", destino.usuario())) {
        // Ni publicaciones ni rastro de la cuenta: tampoco se ha servido.
        return None;
    }

    // Qué publicaciones son las que se han pedido.
    let autor_de = |p: &Value| texto(p.get("user"), "username");
    let elegidas: Vec<&Value> = match destino {
        Destino::Perfil { usuario } => {
            publicaciones.iter().filter(|p| autor_de(p).eq_ignore_ascii_case(usuario)).collect()
        }
        // SOLO el post pedido. La página trae además las respuestas y unas
        // «publicaciones relacionadas» de otras cuentas, y podría traer otras
        // del mismo autor que nadie ha pedido. Un hilo entero del autor se
        // captura con el userscript, que lee lo que la página va cargando.
        Destino::Post { codigo, .. } => {
            publicaciones.iter().filter(|p| texto(Some(p), "code") == *codigo).take(1).collect()
        }
    };

    let mut vistas_pk = std::collections::HashSet::new();
    let mut vistas_url = std::collections::HashSet::new();
    let mut lectura = Lectura { hay_mas: html.contains("\"has_next_page\":true"), ..Default::default() };
    for p in elegidas {
        let pk = texto(Some(p), "pk");
        let clave = if pk.is_empty() { texto(Some(p), "code") } else { pk };
        if !vistas_pk.insert(clave) {
            continue;
        }
        lectura.publicaciones += 1;
        for item in items_de_publicacion(p) {
            if vistas_url.insert(sin_query(&item.url).to_string()) {
                lectura.items.push(item);
            }
        }
    }
    Some(lectura)
}

/// ¿La página es el muro de «inicia sesión» de Threads? Algunos perfiles no se
/// enseñan a visitantes (medido: @nasawebb, @nasahubble y otros devuelven
/// `BarcelonaLoginRoot` y ni una publicación, mientras @nasa sí se ve). Solo
/// significa algo cuando NO ha salido ninguna publicación: una página normal
/// también lleva su botón de «Log in».
pub fn pide_sesion(html: &str) -> bool {
    html.contains("BarcelonaLoginRoot")
}

/// Las URL nuevas —archivo y, si la tiene, su pista de audio— del mismo
/// archivo en una lectura fresca de su post: para un enlace firmado que ha
/// caducado.
///
/// Se busca por la RUTA, que no cambia al volver a firmar (solo cambian `oh` y
/// `oe` en la query). Si no aparece, por su posición en el carrusel.
pub fn url_fresca(lectura: &Lectura, url_vieja: &str, codigo: &str, num: u32) -> Option<(String, String)> {
    let ruta = sin_query(url_vieja);
    lectura
        .items
        .iter()
        .find(|i| sin_query(&i.url) == ruta)
        .or_else(|| {
            (num > 0).then_some(())?;
            lectura.items.iter().find(|i| i.post_id == codigo && i.index_in_post == num)
        })
        .map(|i| (i.url.clone(), i.audio_url.clone()))
        .filter(|(u, _)| u != url_vieja)
}

// ------------------------------- Internos -------------------------------

/// El contenido de cada `<script type="application/json" …>…</script>`.
fn bloques_json(html: &str) -> impl Iterator<Item = &str> {
    const APERTURA: &str = "<script type=\"application/json\"";
    let mut resto = html;
    std::iter::from_fn(move || {
        let ini = resto.find(APERTURA)?;
        let tras = &resto[ini + APERTURA.len()..];
        let cuerpo = &tras[tras.find('>')? + 1..];
        let fin = cuerpo.find("</script>")?;
        resto = &cuerpo[fin..];
        Some(&cuerpo[..fin])
    })
}

/// «Verdadero» como en JavaScript, para que la búsqueda por forma sea la misma
/// que la del userscript: un objeto o una lista vacíos SÍ cuentan.
fn verdad(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Recorre CUALQUIER JSON buscando publicaciones por su FORMA, no por su ruta
/// (`data.mediaData.edges[].node.thread_items[].post` es de Meta y la cambia
/// cuando quiere): medios Y algo que la identifique. Igual que `tdThHarvest`.
fn cosechar<'a>(v: &'a Value, out: &mut Vec<&'a Value>, prof: u32) {
    if prof > 40 {
        return;
    }
    match v {
        Value::Array(a) => a.iter().for_each(|x| cosechar(x, out, prof + 1)),
        Value::Object(o) => {
            let medios =
                verdad(o.get("image_versions2")) || verdad(o.get("video_versions")) || verdad(o.get("carousel_media"));
            let identidad = verdad(o.get("code"))
                || verdad(o.get("caption"))
                || verdad(o.get("user"))
                || verdad(o.get("carousel_media"));
            if medios && identidad {
                out.push(v);
                return; // sus hijos ya los mira `items_de_publicacion`
            }
            o.values().for_each(|x| cosechar(x, out, prof + 1));
        }
        _ => {}
    }
}

/// Texto de una clave, admitiendo números (`pk` llega como cadena o número).
fn texto(v: Option<&Value>, clave: &str) -> String {
    match v.and_then(|v| v.get(clave)) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn numero(v: &Value, clave: &str) -> u32 {
    v.get(clave).and_then(|x| x.as_u64()).map(|x| x.min(u32::MAX as u64) as u32).unwrap_or(0)
}

fn sin_query(u: &str) -> &str {
    u.split(['?', '#']).next().unwrap_or(u)
}

/// Candidatos de imagen ordenados de MENOR a MAYOR área. Por área y no por
/// anchura: 1080×1350 y 1080×1080 empatan en anchura y no son el mismo archivo.
fn candidatos(n: &Value) -> Vec<(&str, u32, u32)> {
    let mut c: Vec<(&str, u32, u32)> = n
        .get("image_versions2")
        .and_then(|i| i.get("candidates"))
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let u = x.get("url")?.as_str()?;
                    u.starts_with("http").then(|| (u, numero(x, "width"), numero(x, "height")))
                })
                .collect()
        })
        .unwrap_or_default();
    c.sort_by_key(|(_, w, h)| u64::from(*w) * u64::from(*h));
    c
}

/// La mejor calidad de un vídeo, sacada de su manifiesto DASH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dash {
    /// La pista de vídeo de más resolución (y, a igualdad, más bitrate).
    pub video: String,
    pub ancho: u32,
    pub alto: u32,
    /// La pista de audio, aparte. Vacía si el manifiesto no trae ninguna.
    pub audio: String,
    /// Bytes de vídeo + audio, si el manifiesto los declara (`FBContentLength`).
    pub bytes: u64,
}

/// El lado largo del MP4 progresivo de Meta. Medido: `video_versions` es
/// `xpv_progressive…C3.1280` (H.264 1280×720 a 1,09 Mbps en la muestra), y el
/// manifiesto del mismo vídeo llega a 1920×1080 en VP9 a 1,38 Mbps. Solo
/// compensa bajar las pistas por separado cuando superan esto.
const LADO_PROGRESIVO: u32 = 1280;

/// La mejor pista de vídeo y la de audio de un manifiesto DASH de Meta
/// (`video_dash_manifest`, un MPD en texto).
///
/// Cada `Representation` lleva su `BaseURL`: un MP4 COMPLETO, firmado igual
/// que el progresivo (medido: los tamaños descargados coinciden con su
/// `FBContentLength`). No hay segmentos que unir; solo dos archivos.
pub fn mejor_dash(mpd: &str) -> Option<Dash> {
    let mut video: Option<(u64, u64, String, u32, u32, u64)> = None; // (lado, bitrate, url, w, h, bytes)
    let mut audio: Option<(u64, String, u64)> = None; // (bitrate, url, bytes)
    for (tipo_conjunto, cuerpo) in bloques_xml(mpd, "AdaptationSet") {
        for (etiqueta, contenido) in bloques_xml(cuerpo, "Representation") {
            let Some(url) = bloques_xml(contenido, "BaseURL")
                .next()
                .map(|(_, u)| desescapar_xml(u.trim()))
                .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            else {
                continue;
            };
            let tipo = atributo(etiqueta, "mimeType")
                .or_else(|| atributo(tipo_conjunto, "mimeType"))
                .or_else(|| atributo(tipo_conjunto, "contentType"))
                .unwrap_or("");
            let num = |a: &str| atributo(etiqueta, a).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let (bitrate, bytes) = (num("bandwidth"), num("FBContentLength"));
            if tipo.starts_with("video") {
                let (w, h) = (num("width") as u32, num("height") as u32);
                let clave = (u64::from(w.max(h)), bitrate);
                if video.as_ref().is_none_or(|v| clave > (v.0, v.1)) {
                    video = Some((clave.0, clave.1, url, w, h, bytes));
                }
            } else if tipo.starts_with("audio") && audio.as_ref().is_none_or(|a| bitrate > a.0) {
                audio = Some((bitrate, url, bytes));
            }
        }
    }
    let (_, _, url, ancho, alto, bytes_v) = video?;
    let (audio, bytes_a) = audio.map(|(_, u, b)| (u, b)).unwrap_or_default();
    Some(Dash { video: url, ancho, alto, audio, bytes: bytes_v + bytes_a })
}

/// Los elementos `<nombre …>…</nombre>` de un trozo de XML: (etiqueta de
/// apertura, contenido). Lo justo para un MPD, que es XML plano y sin
/// comentarios raros; no un analizador de XML.
fn bloques_xml<'a>(xml: &'a str, nombre: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
    let apertura = format!("<{nombre}");
    let cierre = format!("</{nombre}>");
    let mut resto = xml;
    std::iter::from_fn(move || loop {
        let ini = resto.find(&apertura)?;
        let tras = &resto[ini + apertura.len()..];
        // `<Representation` no debe casar con `<RepresentationIndex`.
        if !tras.starts_with([' ', '>', '\t', '\n', '\r', '/']) {
            resto = tras;
            continue;
        }
        let fin_etiqueta = tras.find('>')?;
        let etiqueta = &tras[..fin_etiqueta];
        if etiqueta.ends_with('/') {
            resto = &tras[fin_etiqueta + 1..];
            return Some((etiqueta, ""));
        }
        let cuerpo = &tras[fin_etiqueta + 1..];
        let fin = cuerpo.find(&cierre)?;
        resto = &cuerpo[fin + cierre.len()..];
        return Some((etiqueta, &cuerpo[..fin]));
    })
}

/// El valor de `nombre="…"` dentro de una etiqueta. Con el espacio delante:
/// `width` no debe casar con `FBwidth`.
fn atributo<'a>(etiqueta: &'a str, nombre: &str) -> Option<&'a str> {
    let clave = format!(" {nombre}=\"");
    let ini = etiqueta.find(&clave)? + clave.len();
    let fin = etiqueta[ini..].find('"')?;
    Some(&etiqueta[ini..ini + fin])
}

/// Las cinco entidades de XML. En los `BaseURL` de Meta solo aparece `&amp;`,
/// pero una URL con `&amp;` sin desescapar es una URL que da 403.
fn desescapar_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// El vídeo: el primero de `video_versions`. Threads publica tres (`type`
/// 101, 102 y 103) SIN anchura ni altura, y en la muestra los tres eran el
/// mismo MP4 progresivo con audio.
fn video(n: &Value) -> Option<&str> {
    n.get("video_versions")?.as_array()?.iter().find_map(|v| {
        let u = v.get("url")?.as_str()?;
        u.starts_with("http").then_some(u)
    })
}

/// Las fichas de una publicación: una por archivo, o una por elemento del
/// carrusel.
fn items_de_publicacion(p: &Value) -> Vec<GalleryItem> {
    let autor = texto(p.get("user"), "username");
    let codigo = texto(Some(p), "code");
    let descripcion: String = texto(p.get("caption"), "text").chars().take(160).collect();
    let fecha = p.get("taken_at").and_then(|t| t.as_i64()).map(fecha_de_unix).unwrap_or_default();
    let post_url = if autor.is_empty() || codigo.is_empty() {
        String::new()
    } else {
        format!("https://www.threads.com/@{autor}/post/{codigo}")
    };

    let hijos: Vec<&Value> = match p.get("carousel_media").and_then(|c| c.as_array()) {
        Some(c) if !c.is_empty() => c.iter().collect(),
        _ => vec![p],
    };
    let total = hijos.len() as u32;
    let mut out = Vec::new();
    for (i, n) in hijos.into_iter().enumerate() {
        let cands = candidatos(n);
        let vid = video(n);
        let mayor = cands.last().copied();
        let (url, es_video) = match (vid, mayor) {
            (Some(v), _) => (v, true),
            (None, Some((u, _, _))) => (u, false),
            // Publicación de solo texto: `image_versions2` existe, vacío.
            (None, None) => continue,
        };
        // En un vídeo la imagen es la PORTADA, y su tamaño no es el del vídeo:
        // el del archivo es `original_width`/`original_height`.
        let (w, h) = match (es_video, mayor) {
            (false, Some((_, w, h))) if w > 0 && h > 0 => (w, h),
            _ => {
                let (w, h) = (numero(n, "original_width"), numero(n, "original_height"));
                if w > 0 && h > 0 {
                    (w, h)
                } else {
                    (numero(p, "original_width"), numero(p, "original_height"))
                }
            }
        };
        let ext = if es_video {
            "mp4".to_string()
        } else {
            extension_imagen(url)
        };
        // MÁXIMA CALIDAD: la del manifiesto DASH, cuando supera al MP4
        // progresivo (ver `LADO_PROGRESIVO`). Llega en dos pistas; la descarga
        // las junta con ffmpeg, y sin él se queda el vídeo sin audio.
        let dash = if es_video {
            n.get("video_dash_manifest")
                .and_then(|m| m.as_str())
                .and_then(mejor_dash)
                .filter(|d| d.ancho.max(d.alto) > LADO_PROGRESIVO)
        } else {
            None
        };
        let (url, w, h, audio_url, filesize) = match &dash {
            Some(d) => (d.video.as_str(), d.ancho, d.alto, d.audio.clone(), d.bytes),
            None => (url, w, h, String::new(), 0),
        };
        let carrusel = total > 1;
        out.push(GalleryItem {
            url: url.to_string(),
            // El nombre es «autor_código_n»: el código es el de la URL del
            // post, y la posición separa los archivos de un carrusel.
            filename: if carrusel { (i + 1).to_string() } else { String::new() },
            ext,
            width: w,
            height: h,
            filesize,
            audio_url,
            is_video: es_video,
            post_id: codigo.clone(),
            index_in_post: if carrusel { i as u32 + 1 } else { 0 },
            count_in_post: total.max(1),
            author: autor.clone(),
            description: descripcion.clone(),
            date: fecha.clone(),
            post_url: post_url.clone(),
            // La rejilla pinta ~320 px: el candidato MÁS PEQUEÑO, no el original.
            thumb_url: cands.first().map(|(u, _, _)| u.to_string()).unwrap_or_default(),
            selected: false,
        });
    }
    out
}

/// Extensión de una imagen del CDN de Meta (`…/123_n.jpg?stp=…`): `jpg` si
/// la ruta no la dice.
fn extension_imagen(url: &str) -> String {
    sin_query(url)
        .rsplit('/')
        .next()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png" | "webp" | "heic" | "gif"))
        .unwrap_or_else(|| "jpg".into())
}

/// `taken_at` (segundos Unix) → `AAAA-MM-DD`, en UTC. Algoritmo de días civiles
/// de Howard Hinnant: sin depender de una biblioteca de fechas para esto.
fn fecha_de_unix(s: i64) -> String {
    let dias = s.div_euclid(86_400);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una página como las de threads.com: las publicaciones reales del
    /// fixture, metidas en un `<script type="application/json">` con una ruta
    /// cualquiera por encima. La ruta da igual: se buscan por su forma.
    fn pagina(extra: &str) -> String {
        let posts = include_str!("testdata/threads_posts.json");
        format!(
            "<html><head><title>Threads</title></head><body>\
             <script type=\"application/json\" data-sjs>{{\"require\":[[\"ScheduledServerJS\",\"handle\",null,\
             [{{\"__bbox\":{{\"result\":{{\"data\":{{\"edges\":{posts}}}}}}}}}]]]}}</script>{extra}</body></html>"
        )
    }

    #[test]
    fn clasifica_perfiles_y_posts() {
        assert_eq!(
            clasificar("https://www.threads.com/@nasa"),
            Some(Destino::Perfil { usuario: "nasa".into() })
        );
        assert_eq!(
            clasificar("https://threads.net/@nasa/media?xmt=1"),
            Some(Destino::Perfil { usuario: "nasa".into() })
        );
        let post = clasificar("https://www.threads.com/@netflix/post/De1nYC8Eggo?xmt=AQF0").unwrap();
        assert_eq!(post, Destino::Post { usuario: "netflix".into(), codigo: "De1nYC8Eggo".into() });
        assert_eq!(post.url_web(), "https://www.threads.com/@netflix/post/De1nYC8Eggo");
        assert_eq!(
            clasificar("https://www.threads.net/@a.b_c/post/C2xieeovAIA").unwrap().url_web(),
            "https://www.threads.com/@a.b_c/post/C2xieeovAIA",
            "threads.net se pide en threads.com"
        );
        // Ni otros hosts, ni rutas que no son de una cuenta.
        assert_eq!(clasificar("https://threads.com.atacante.example/@nasa"), None);
        assert_eq!(clasificar("https://nsthreads.com/@nasa"), None);
        assert_eq!(clasificar("https://www.threads.com/"), None);
        assert_eq!(clasificar("https://www.threads.com/search?q=x"), None);
        assert_eq!(clasificar("https://www.threads.com/@../post/x"), None);
        assert_eq!(clasificar("https://www.threads.com/@nasa/post/%2F..%2F"), None);
    }

    /// Un perfil: solo las publicaciones de esa cuenta, un archivo por
    /// elemento de carrusel, y las de solo texto cuentan pero no dan fichas.
    #[test]
    fn un_perfil_da_las_publicaciones_de_su_cuenta() {
        let l = leer_html(&pagina(""), &Destino::Perfil { usuario: "NASA".into() }).unwrap();
        assert_eq!(l.publicaciones, 3, "carrusel + foto + texto; las de otras cuentas fuera");
        assert_eq!(l.items.len(), 5, "4 del carrusel + 1 foto");
        assert!(l.items.iter().all(|i| i.author == "nasa"));

        let c = &l.items[0];
        assert_eq!(c.post_id, "Decc755lYhC");
        assert_eq!((c.index_in_post, c.count_in_post), (1, 4));
        assert_eq!((c.width, c.height), (3024, 4032), "el original, no la miniatura");
        assert_eq!(c.ext, "jpg");
        assert_eq!(c.filename, "1");
        assert_eq!(c.post_url, "https://www.threads.com/@nasa/post/Decc755lYhC");
        assert_ne!(c.thumb_url, c.url, "la miniatura es el candidato pequeño");
        assert!(!c.date.is_empty());
        assert_eq!(l.items[3].position(), "4/4");

        let foto = &l.items[4];
        assert_eq!(foto.post_id, "DeUWxGgjHSK");
        assert_eq!((foto.width, foto.height), (4096, 2731));
        assert_eq!(foto.count_in_post, 1);
        assert_eq!(foto.filename, "", "un archivo suelto no lleva número");
        assert!(!l.hay_mas);
    }

    /// El vídeo: el MP4 de `video_versions` (sin extensión en la ruta), con la
    /// resolución del vídeo y la portada como miniatura.
    #[test]
    fn un_post_de_video_da_el_mp4_con_su_resolucion() {
        let destino = Destino::Post { usuario: "netflix".into(), codigo: "De1nYC8Eggo".into() };
        let l = leer_html(&pagina(""), &destino).unwrap();
        assert_eq!(l.publicaciones, 1, "las de otras cuentas de la página, fuera");
        let v = &l.items[0];
        assert!(v.is_video);
        assert_eq!(v.ext, "mp4");
        assert!(v.url.contains("/o1/v/t16/"), "el vídeo, no su portada");
        assert_eq!((v.width, v.height), (1920, 1080), "original_width/height, no la portada de 640");
        assert!(!v.thumb_url.is_empty() && v.thumb_url != v.url);
        assert_eq!(v.post_url, "https://www.threads.com/@netflix/post/De1nYC8Eggo");
    }

    /// Un post de solo texto existe pero no tiene nada que bajar.
    #[test]
    fn un_post_de_texto_no_da_fichas() {
        let destino = Destino::Post { usuario: "nasa".into(), codigo: "DekCdIIEZCG".into() };
        let l = leer_html(&pagina(""), &destino).unwrap();
        assert!(l.items.is_empty());
        // Y las otras publicaciones de nasa que hay en la página no se cuelan.
        assert_eq!(l.publicaciones, 1);
    }

    /// La carcasa de error que llega sin las cabeceras de navegación, o con un
    /// post borrado: no es «cero medios», es «no se ha servido».
    #[test]
    fn la_carcasa_de_error_no_es_una_lista_vacia() {
        let carcasa = "<html><script>{\"__dr\":\"Barcelona404ErrorRoot.entrypoint\"}</script></html>";
        assert!(leer_html(carcasa, &Destino::Perfil { usuario: "nasa".into() }).is_none());
        assert!(leer_html("<html></html>", &Destino::Perfil { usuario: "nasa".into() }).is_none());
        // Un perfil que existe pero no ha publicado nada con medios sí se sirve.
        let solo_cuenta = "<html><script type=\"application/json\">{\"user\":{\"username\":\"nasa\"}}</script></html>";
        let l = leer_html(solo_cuenta, &Destino::Perfil { usuario: "nasa".into() }).unwrap();
        assert_eq!((l.items.len(), l.publicaciones), (0, 0));
    }

    /// El muro de inicio de sesión: ni publicaciones ni cuenta en la página.
    #[test]
    fn reconoce_el_muro_de_inicio_de_sesion() {
        let muro = "<html><script>{\"__dr\":\"BarcelonaLoginRoot.entrypoint\"}</script></html>";
        assert!(pide_sesion(muro));
        assert!(leer_html(muro, &Destino::Perfil { usuario: "nasawebb".into() }).is_none());
        assert!(!pide_sesion(&pagina("")));
    }

    #[test]
    fn avisa_de_que_hay_mas_publicaciones() {
        let l = leer_html(
            &pagina("<script>{\"page_info\":{\"end_cursor\":\"QVF\",\"has_next_page\":true}}</script>"),
            &Destino::Perfil { usuario: "nasa".into() },
        )
        .unwrap();
        assert!(l.hay_mas);
    }

    /// Un enlace caducado se renueva con la MISMA ruta en una lectura nueva.
    #[test]
    fn la_url_fresca_es_la_del_mismo_archivo() {
        let l = leer_html(&pagina(""), &Destino::Perfil { usuario: "nasa".into() }).unwrap();
        let actual = &l.items[2];
        let vieja = format!("{}?oh=viejo&oe=0", sin_query(&actual.url));
        assert_eq!(url_fresca(&l, &vieja, "Decc755lYhC", 3).map(|(u, _)| u).as_deref(), Some(actual.url.as_str()));
        // Por posición, si la ruta no aparece.
        assert_eq!(
            url_fresca(&l, "https://scontent.cdninstagram.com/otra.jpg", "Decc755lYhC", 3).map(|(u, _)| u).as_deref(),
            Some(actual.url.as_str())
        );
        assert_eq!(url_fresca(&l, "https://scontent.cdninstagram.com/otra.jpg", "Decc755lYhC", 0), None);
    }

    /// El manifiesto DASH real de un vídeo de Threads (recortado a 240p, 720p,
    /// 1080p y el audio): la mejor pista es la de 1080p, el audio va aparte, y
    /// las URL salen con `&` y no con `&amp;`.
    #[test]
    fn el_manifiesto_da_la_mejor_pista_y_su_audio() {
        let d = mejor_dash(include_str!("testdata/threads_mpd.xml")).expect("hay pistas");
        assert_eq!((d.ancho, d.alto), (1920, 1080));
        assert!(d.video.contains("/m367/AQNpDjkZ5"), "{}", d.video);
        assert!(d.audio.contains("/m78/AQMYnjl3"), "{}", d.audio);
        assert!(!d.video.contains("&amp;") && d.video.contains('&'), "desescapada: {}", d.video);
        assert_eq!(d.bytes, 4_527_727 + 241_563, "FBContentLength de las dos pistas");
        // Sin pistas, nada; y una sin BaseURL no cuenta.
        assert_eq!(mejor_dash("<MPD></MPD>"), None);
        assert_eq!(
            mejor_dash("<MPD><AdaptationSet contentType=\"video\"><Representation width=\"1920\" height=\"1080\"/></AdaptationSet></MPD>"),
            None
        );
    }

    /// Un post de vídeo con manifiesto: la ficha es la pista de 1080p, con su
    /// audio aparte, su resolución y su peso. Sin manifiesto, o con uno que no
    /// supera al MP4 progresivo, se queda el progresivo, que ya lleva audio.
    #[test]
    fn un_video_con_manifiesto_baja_la_maxima_calidad() {
        let mpd = include_str!("testdata/threads_mpd.xml");
        let con_mpd = |m: &str| {
            let mut posts: Vec<Value> = serde_json::from_str(include_str!("testdata/threads_posts.json")).unwrap();
            for p in &mut posts {
                if p["code"] == "De1nYC8Eggo" {
                    p["video_dash_manifest"] = Value::String(m.to_string());
                }
            }
            let html = format!(
                "<script type=\"application/json\">{{\"x\":{}}}</script>",
                serde_json::to_string(&posts).unwrap()
            );
            let d = Destino::Post { usuario: "netflix".into(), codigo: "De1nYC8Eggo".into() };
            leer_html(&html, &d).unwrap().items.remove(0)
        };

        let v = con_mpd(mpd);
        assert!(v.is_video && v.ext == "mp4");
        assert!(v.url.contains("/m367/AQNpDjkZ5"), "la pista de 1080p: {}", v.url);
        assert!(v.audio_url.contains("/m78/"), "con su audio: {}", v.audio_url);
        assert_eq!((v.width, v.height), (1920, 1080));
        assert_eq!(v.filesize, 4_527_727 + 241_563);

        // Un manifiesto que solo llega a 1280×720 no mejora al progresivo.
        let solo_720 = mpd.replace("width=\"1920\" height=\"1080\"", "width=\"1280\" height=\"720\"");
        let p = con_mpd(&solo_720);
        assert!(p.url.contains("/o1/v/t16/"), "el progresivo: {}", p.url);
        assert_eq!(p.audio_url, "");
        assert_eq!(p.filesize, 0);
    }

    #[test]
    fn fechas_y_extensiones() {
        assert_eq!(fecha_de_unix(0), "1970-01-01");
        assert_eq!(fecha_de_unix(1_792_760_420), "2026-10-23");
        assert_eq!(fecha_de_unix(951_782_400), "2000-02-29");
        assert_eq!(extension_imagen("https://x.cdninstagram.com/v/t51/1_n.webp?stp=dst-webp"), "webp");
        assert_eq!(extension_imagen("https://x.cdninstagram.com/v/t51/AQabc?stp=1"), "jpg");
    }
}
