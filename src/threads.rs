//! Threads (threads.com) sin navegador: un post suelto, entero, y un perfil
//! entero, página a página.
//!
//! LO QUE SE MIDIÓ ANTES DE ESCRIBIR ESTO (24 de octubre de 2026). Ni gallery-dl
//! (1.33.0-dev) ni yt-dlp (2026.10.21) tienen extractor de Threads. Pero la
//! página de un post, pedida como la pide un navegador, ya trae en el HTML las
//! publicaciones con sus medios: los mismos `image_versions2`, `video_versions`
//! y `carousel_media` que lee el userscript, a resolución original (3072×4096
//! en la muestra) y con las URL firmadas del CDN.
//!
//! LA TRAMPA: sin las cuatro cabeceras `Sec-Fetch-*` de una navegación,
//! threads.com responde `200` con la carcasa de su página de error
//! (`Barcelona404ErrorRoot`) y ni un solo medio. Ninguna de las cuatro basta
//! por separado. Con ellas, lo mismo con curl que con reqwest/rustls.
//!
//! UN PERFIL ENTERO. Su página trae los cuatro primeros hilos y, con ellos, la
//! consulta con la que los pidió (`BarcelonaProfileThreadsTabDirectQuery`: su
//! `queryID`, sus variables y el `end_cursor`), más las fichas que el JS de la
//! web mete en cada petición (`LSD`, el token CSRF, `SiteData`…). Para seguir,
//! la web repite esa consulta con `after`, y aquí se repite igual, campo por
//! campo (ver `Paginador::peticion`). A un visitante se la sirve si lleva la
//! cabecera que la web pone en ese caso, `X-LOGGED-OUT-THREADS-MIGRATED-REQUEST`.
//! Medido sin sesión: @zuck entero —171 hilos, hasta julio de 2023— en 12
//! páginas de 15, con pausas de ~3 s y sin un solo bloqueo. La primera página
//! vuelve a traer los hilos del HTML: de ahí `Lectura::vistas`.
//!
//! CON SESIÓN. Hay perfiles que Threads no enseña a visitantes (piden iniciar
//! sesión: @nasawebb, @nasahubble) y cuentas privadas que solo ve quien las
//! sigue. Para esos se usa la sesión de threads.com del usuario —las cookies de
//! su navegador o su cookies.txt—, con la misma petición que hace la web con
//! sesión: su `fb_dtsg`, su id (`av`, `__user`) y sin la cabecera de
//! visitante. Sin sesión, esa forma de la petición contesta «User not found»:
//! el servidor la toma por una con sesión. Solo se usa cuando sin ella no se
//! ve nada, porque una sesión de Meta usada fuera del navegador es justo lo que
//! Meta vigila, y la cuenta es del usuario.
//!
//! LO QUE NO SE HA PODIDO MEDIR: la petición con una sesión de verdad, porque
//! donde se escribió esto no hay cuenta de Threads. Está hecha a imagen del JS
//! de la web (`BarcelonaRelayEnvironment`, `createRelayFBNetworkFetch`,
//! `getAsyncParams`), y `threads_sesion_en_vivo` la prueba con un cookies.txt.

use crate::gallery::GalleryItem;
use serde_json::{Map, Value};
use std::collections::HashSet;

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
    /// Con qué pedir las siguientes: solo en un perfil, y si la página trae
    /// su consulta.
    pub paginador: Option<Paginador>,
    /// Las publicaciones ya leídas, por su código. Las páginas siguientes no
    /// deben repetirlas, y la primera vuelve a traer las de la página.
    pub vistas: HashSet<String>,
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

    let mut lectura = Lectura::default();
    anadir(&elegidas, &mut lectura.vistas, &mut lectura.items, &mut lectura.publicaciones);
    if let Destino::Perfil { usuario } = destino {
        lectura.paginador = paginador(html, usuario);
    }
    lectura.hay_mas = match &lectura.paginador {
        Some(p) => p.hay_mas(),
        None => html.contains("\"has_next_page\":true"),
    };
    Some(lectura)
}

/// Pasa a fichas las publicaciones que no estén ya en `vistas`.
fn anadir(publicaciones: &[&Value], vistas: &mut HashSet<String>, items: &mut Vec<GalleryItem>, cuenta: &mut usize) {
    let mut vistas_url = HashSet::new();
    for p in publicaciones {
        // Por el código, que es lo que lleva cada ficha en `post_id`: al
        // seguir tras «Detener», la aplicación siembra con él lo que ya está
        // en la rejilla.
        let codigo = texto(Some(p), "code");
        let clave = if codigo.is_empty() { texto(Some(p), "pk") } else { codigo };
        if !vistas.insert(clave) {
            continue;
        }
        *cuenta += 1;
        for item in items_de_publicacion(p) {
            if vistas_url.insert(sin_query(&item.url).to_string()) {
                items.push(item);
            }
        }
    }
}

// ------------------- Las páginas siguientes de un perfil -------------------

/// Adonde pide la web de Threads las publicaciones de un perfil.
///
/// Es una consulta «distillery» (`is_distillery: true` en su JS), y
/// `createRelayIGNetwork` manda esas a `/graphql/query`; las demás, a
/// `/api/graphql`. Medido: las dos direcciones la sirven, y se imita a la web.
pub const URL_CONSULTA: &str = "https://www.threads.com/graphql/query";

/// Hilos por página. Pedir más no sirve: con `first: 20` llegan 15 igual.
const POR_PAGINA: u64 = 15;

/// `X-IG-App-ID` de la web de Threads, por si la página no lo trae.
const APP_ID: &str = "238260118697367";

/// `X-ASBD-ID`: una constante del JS de Meta (`BDHeaderConfig`).
const ASBD_ID: &str = "359341";

/// Lo necesario para pedir las publicaciones siguientes de un perfil: la
/// consulta con la que su propia página trajo las primeras y los datos de esa
/// página que el JS de la web mete en cada petición.
#[derive(Clone, PartialEq)]
pub struct Paginador {
    pub usuario: String,
    doc_id: String,
    consulta: String,
    variables: Map<String, Value>,
    /// Por dónde sigue el perfil. `None` = no hay más; vacío = desde el
    /// principio (`after: null`), si la página dice que hay más sin decir
    /// desde dónde.
    cursor: Option<String>,
    lsd: String,
    csrf: String,
    /// `fb_dtsg`: solo lo trae una página leída con sesión.
    dtsg: String,
    /// Quién mira: `0` sin sesión.
    espectador: String,
    app_id: String,
    bloks: String,
    hs: String,
    rev: String,
    hsi: String,
    comet: String,
    dpr: String,
    ccg: String,
    /// `__s` y `X-Web-Session-ID`: la web lo inventa al cargar.
    sesion_web: String,
    /// `__req`: cuántas peticiones lleva, en base 36.
    peticiones: u32,
}

/// A mano, para que un `{:?}` no saque nunca las fichas de una sesión.
impl std::fmt::Debug for Paginador {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Paginador")
            .field("usuario", &self.usuario)
            .field("consulta", &self.consulta)
            .field("doc_id", &self.doc_id)
            .field("hay_mas", &self.hay_mas())
            .field("con_sesion", &self.con_sesion())
            .finish_non_exhaustive()
    }
}

/// Una petición lista para mandar: cabeceras y cuerpo (form-urlencoded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peticion {
    pub cabeceras: Vec<(&'static str, String)>,
    pub cuerpo: Vec<(&'static str, String)>,
}

/// Las publicaciones nuevas de una página.
#[derive(Debug, Default)]
pub struct Pagina {
    pub items: Vec<GalleryItem>,
    /// Publicaciones nuevas de la cuenta, con medios o sin ellos.
    pub publicaciones: usize,
}

/// Por qué una página no ha traído publicaciones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FalloPagina {
    /// No es JSON: Threads ha contestado con una página (la de iniciar sesión,
    /// una comprobación de seguridad).
    NoEsJson,
    /// JSON sin publicaciones, con lo que diga Meta (`User not found`,
    /// `login_required`…).
    SinDatos(String),
}

impl Paginador {
    pub fn hay_mas(&self) -> bool {
        self.cursor.is_some()
    }

    /// ¿Sale de una página leída con sesión? Sin `fb_dtsg` y sin id de quien
    /// mira, la petición va como la de un visitante.
    pub fn con_sesion(&self) -> bool {
        !self.dtsg.is_empty() && !self.espectador.is_empty() && self.espectador != "0"
    }

    /// Sigue desde donde iba otro: al pasar a la sesión a mitad de un perfil.
    pub fn seguir_desde(&mut self, otro: &Paginador) {
        self.cursor = otro.cursor.clone();
    }

    /// La petición de la página siguiente, como la hace la web: cabeceras de
    /// `BarcelonaRelayEnvironment` y `getAsyncHeaders`, cuerpo de
    /// `getAsyncParams` y `createRelayFBNetworkFetch`, en ese orden. `cookie`
    /// es la cabecera que irá con ella: de ahí sale el token CSRF si la página
    /// no lo traía. `None` si ya no hay más.
    pub fn peticion(&mut self, cookie: &str) -> Option<Peticion> {
        let cursor = self.cursor.as_deref()?;
        let mut variables = self.variables.clone();
        let after = if cursor.is_empty() { Value::Null } else { Value::String(cursor.to_string()) };
        variables.insert("after".into(), after);
        variables.insert("first".into(), Value::from(POR_PAGINA));
        self.peticiones += 1;
        let sesion = self.con_sesion();
        // `jazoest` es `DTSGUtils.getNumericValue`: un 2 (la versión de
        // `SprinkleConfig`) y la suma de los códigos del token, que es el
        // `fb_dtsg` con sesión y el `lsd` sin ella.
        let ficha = if sesion { &self.dtsg } else { &self.lsd };
        let mut cuerpo = vec![
            ("av", self.espectador.clone()),
            ("__user", self.espectador.clone()),
            ("__a", "1".to_string()),
            ("__req", base36(self.peticiones)),
            ("__hs", self.hs.clone()),
            ("dpr", self.dpr.clone()),
            ("__ccg", self.ccg.clone()),
            ("__rev", self.rev.clone()),
            ("__s", self.sesion_web.clone()),
            ("__hsi", self.hsi.clone()),
            ("__comet_req", self.comet.clone()),
        ];
        if sesion {
            cuerpo.push(("fb_dtsg", self.dtsg.clone()));
        }
        cuerpo.extend([
            ("jazoest", jazoest(ficha)),
            ("lsd", self.lsd.clone()),
            ("fb_api_caller_class", "RelayModern".to_string()),
            ("fb_api_req_friendly_name", self.consulta.clone()),
            ("server_timestamps", "true".to_string()),
            ("variables", Value::Object(variables).to_string()),
            ("doc_id", self.doc_id.clone()),
        ]);
        // Un dato que la página no trajo no se manda vacío: se omite.
        cuerpo.retain(|(_, v)| !v.is_empty());

        let csrf = if self.csrf.is_empty() { valor_cookie(cookie, "csrftoken") } else { self.csrf.clone() };
        let mut cabeceras = vec![
            ("Accept", "*/*".to_string()),
            ("Accept-Language", "en-US,en;q=0.9".to_string()),
            ("Origin", "https://www.threads.com".to_string()),
            ("Referer", format!("https://www.threads.com/@{}", self.usuario)),
            ("Sec-Fetch-Dest", "empty".to_string()),
            ("Sec-Fetch-Mode", "cors".to_string()),
            ("Sec-Fetch-Site", "same-origin".to_string()),
            ("X-ASBD-ID", ASBD_ID.to_string()),
            ("X-BLOKS-VERSION-ID", self.bloks.clone()),
            ("X-CSRFToken", csrf),
            ("X-FB-Friendly-Name", self.consulta.clone()),
            ("X-FB-LSD", self.lsd.clone()),
            ("X-IG-App-ID", self.app_id.clone()),
            ("X-Web-Session-ID", self.sesion_web.clone()),
        ];
        if !sesion {
            // La que pone la web a un visitante. Sin ella el servidor toma la
            // petición por una con sesión, y sin sesión: «User not found».
            cabeceras.push(("X-LOGGED-OUT-THREADS-MIGRATED-REQUEST", "true".to_string()));
        }
        cabeceras.retain(|(_, v)| !v.is_empty());
        Some(Peticion { cabeceras, cuerpo })
    }

    /// Lee la respuesta a `peticion` y avanza el cursor. Lo que no es de la
    /// cuenta, y lo ya leído (`vistas`), se descarta.
    ///
    /// Un error suelto junto a los datos no tumba la página: medido, Threads
    /// manda a veces `field_exception` en un campo y los hilos completos, y su
    /// propia web solo se para con los de gravedad `CRITICAL`.
    pub fn leer_pagina(&mut self, cuerpo: &str, vistas: &mut HashSet<String>) -> Result<Pagina, FalloPagina> {
        let v = json_de_meta(cuerpo).ok_or(FalloPagina::NoEsJson)?;
        let Some(datos) = v.get("data").and_then(|d| d.get("mediaData")).filter(|m| m.is_object()) else {
            return Err(FalloPagina::SinDatos(mensaje_de(&v)));
        };
        let mut halladas = Vec::new();
        cosechar(datos, &mut halladas, 0);
        let elegidas: Vec<&Value> = halladas
            .into_iter()
            .filter(|p| texto(p.get("user"), "username").eq_ignore_ascii_case(&self.usuario))
            .collect();
        let mut pagina = Pagina::default();
        anadir(&elegidas, vistas, &mut pagina.items, &mut pagina.publicaciones);

        // Un cursor que no avanza, o una página vacía que dice que hay más,
        // acabarían en un bucle: el perfil se da por acabado.
        let hilos = datos.get("edges").and_then(Value::as_array).map_or(0, Vec::len);
        let nuevo = datos.get("page_info").and_then(cursor_de).filter(|c| !c.is_empty());
        self.cursor = match nuevo {
            Some(c) if hilos > 0 && self.cursor.as_deref() != Some(c.as_str()) => Some(c),
            _ => None,
        };
        Ok(pagina)
    }
}

/// Saca de la página de un perfil lo necesario para pedir sus publicaciones
/// siguientes. `None` si no trae la consulta o sus fichas: Threads ha cambiado
/// su web, y quien llama se queda con lo que ya tiene.
pub fn paginador(html: &str, usuario: &str) -> Option<Paginador> {
    let lsd = cadena(&modulo(html, "LSD")?, "token")?;
    let (doc_id, consulta, variables) = bloques_json(html)
        .filter(|b| b.contains("adp_BarcelonaProfileThreadsTab"))
        .filter_map(|b| serde_json::from_str::<Value>(b).ok())
        .find_map(|v| buscar_consulta(&v, 0))?;
    let cursor = bloques_json(html)
        .filter(|b| b.contains("\"page_info\""))
        .filter_map(|b| serde_json::from_str::<Value>(b).ok())
        .find_map(|v| buscar_cursor(&v, 0))
        .flatten();
    let de = |nombre: &str, clave: &str| modulo(html, nombre).and_then(|m| cadena(&m, clave));
    let cifras = |s: &String| s.bytes().all(|b| b.is_ascii_digit());
    Some(Paginador {
        usuario: usuario.to_string(),
        doc_id,
        consulta,
        variables,
        cursor,
        lsd,
        csrf: de("InstagramSecurityConfig", "csrf_token").unwrap_or_default(),
        dtsg: de("DTSGInitialData", "token").unwrap_or_default(),
        espectador: de("CurrentUserInitialData", "USER_ID").filter(cifras).unwrap_or_else(|| "0".into()),
        app_id: de("CurrentUserInitialData", "APP_ID").filter(cifras).unwrap_or_else(|| APP_ID.into()),
        bloks: de("WebBloksVersioningID", "versioningID").unwrap_or_default(),
        hs: de("SiteData", "haste_session").unwrap_or_default(),
        rev: de("SiteData", "client_revision").unwrap_or_default(),
        hsi: de("SiteData", "hsi").unwrap_or_default(),
        comet: de("SiteData", "comet_env").unwrap_or_default(),
        dpr: de("SiteData", "pr").unwrap_or_else(|| "1".into()),
        ccg: de("WebConnectionClassServerGuess", "connectionClass").unwrap_or_else(|| "EXCELLENT".into()),
        sesion_web: sesion_web_nueva(),
        peticiones: 0,
    })
}

/// Lo que dice Meta en una respuesta de error, en una línea. Sirve para el
/// cuerpo de un 4xx, que también suele ser JSON.
pub fn mensaje_de_error(cuerpo: &str) -> String {
    json_de_meta(cuerpo).map(|v| mensaje_de(&v)).unwrap_or_default()
}

/// Una respuesta JSON de Meta, sin el `for (;;);` que antepone a algunas.
fn json_de_meta(cuerpo: &str) -> Option<Value> {
    let cuerpo = cuerpo.trim_start();
    serde_json::from_str(cuerpo.strip_prefix("for (;;);").unwrap_or(cuerpo)).ok()
}

/// En las de GraphQL, `errors[0]`: su `description`, que es la legible
/// («User not found»), y si no su `message`. En las de la API de siempre
/// (`{"message":"checkpoint_required","status":"fail"}`), `message`.
fn mensaje_de(v: &Value) -> String {
    let e = v.get("errors").and_then(|e| e.get(0));
    let m = [
        e.and_then(|e| e.get("description")),
        e.and_then(|e| e.get("message")),
        e.and_then(|e| e.get("summary")),
        v.get("message"),
    ]
    .into_iter()
    .flatten()
    .find_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
    .unwrap_or("");
    m.chars().filter(|c| !c.is_control()).take(160).collect()
}

/// La configuración de un módulo de la página: `["LSD",[],{"token":"…"},323]`
/// → `{"token":"…"}`. Las páginas de Meta los declaran todos así; el objeto
/// se lee con el analizador de JSON, que se para al cerrarlo.
fn modulo(html: &str, nombre: &str) -> Option<Map<String, Value>> {
    let clave = format!("[\"{nombre}\",[],");
    let ini = html.find(&clave)? + clave.len();
    match serde_json::Deserializer::from_str(&html[ini..]).into_iter::<Value>().next()? {
        Ok(Value::Object(m)) => Some(m),
        _ => None,
    }
}

/// Un valor corto de texto o número, tal cual. Lo que no lo es —o es largo,
/// o trae controles— no sale de aquí hacia una cabecera.
fn cadena(m: &Map<String, Value>, clave: &str) -> Option<String> {
    let s = match m.get(clave)? {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)).then_some(s)
}

/// La consulta de la pestaña de hilos entre los «preloaders» de la página:
/// su `queryID`, su nombre y sus variables (`userID` y los
/// `__relay_internal__pv__…`, sin los cuales Threads contesta
/// `missing_required_variable_value`).
fn buscar_consulta(v: &Value, prof: u32) -> Option<(String, String, Map<String, Value>)> {
    if prof > 40 {
        return None;
    }
    match v {
        Value::Array(a) => a.iter().find_map(|x| buscar_consulta(x, prof + 1)),
        Value::Object(o) => {
            let id = o.get("preloaderID").and_then(Value::as_str).unwrap_or("");
            if let Some(resto) = id.strip_prefix("adp_BarcelonaProfileThreadsTab") {
                let doc = o
                    .get("queryID")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
                let variables = o.get("variables").and_then(Value::as_object).filter(|m| m.contains_key("userID"));
                if let (Some(doc), Some(variables)) = (doc, variables) {
                    let nombre = o.get("queryName").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| {
                        format!("BarcelonaProfileThreadsTab{}", resto.split("RelayPreloader").next().unwrap_or(""))
                    });
                    return Some((doc.to_string(), nombre, variables.clone()));
                }
            }
            o.values().find_map(|x| buscar_consulta(x, prof + 1))
        }
        _ => None,
    }
}

/// El cursor de los hilos del perfil, de su `mediaData.page_info`:
/// `Some(None)` = está, y no hay más.
fn buscar_cursor(v: &Value, prof: u32) -> Option<Option<String>> {
    if prof > 40 {
        return None;
    }
    match v {
        Value::Array(a) => a.iter().find_map(|x| buscar_cursor(x, prof + 1)),
        Value::Object(o) => match o.get("mediaData").and_then(|m| m.get("page_info")) {
            Some(pi) => Some(cursor_de(pi)),
            None => o.values().find_map(|x| buscar_cursor(x, prof + 1)),
        },
        _ => None,
    }
}

/// Por dónde sigue un `page_info`: `None` si no hay más.
fn cursor_de(pi: &Value) -> Option<String> {
    (pi.get("has_next_page").and_then(Value::as_bool) == Some(true))
        .then(|| pi.get("end_cursor").and_then(Value::as_str).unwrap_or("").to_string())
}

/// El valor de una cookie en una cabecera `Cookie`.
fn valor_cookie(cookie: &str, nombre: &str) -> String {
    cookie
        .split(';')
        .filter_map(|par| par.trim().split_once('='))
        .find(|(n, _)| *n == nombre)
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default()
}

/// `jazoest`: `2` y la suma de los códigos UTF-16 del token.
fn jazoest(token: &str) -> String {
    format!("2{}", token.encode_utf16().map(u32::from).sum::<u32>())
}

fn base36(mut n: u32) -> String {
    const CIFRAS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut s = Vec::new();
    loop {
        s.push(CIFRAS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// El `WebSession.getId()` de la web: tres grupos de seis caracteres en base
/// 36 (`k3x9q1:0f7zrm:p2a8cd`) que se inventa al cargar la página. No es un
/// secreto ni hace falta que sea impredecible: identifica la pestaña.
fn sesion_web_nueva() -> String {
    let semilla = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64);
    let mut x = semilla | 1;
    let mut grupo = || -> String {
        (0..6)
            .map(|_| {
                // xorshift64
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                char::from_digit((x % 36) as u32, 36).unwrap_or('0')
            })
            .collect()
    };
    let (a, b, c) = (grupo(), grupo(), grupo());
    format!("{a}:{b}:{c}")
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

    // ------------------------ Páginas siguientes ------------------------

    /// La página real de @nasa sin sesión (24/10/2026), recortada a lo que
    /// se lee: los módulos de configuración, los dos «preloaders» y el
    /// resultado de la consulta con su `page_info`. Misma estructura.
    const PERFIL: &str = include_str!("testdata/threads_perfil.html");
    /// La primera respuesta real de la consulta, recortada a cuatro hilos:
    /// un carrusel con su respuesta, una foto, un vídeo y uno de texto.
    const PAGINA: &str = include_str!("testdata/threads_pagina.json");

    fn nasa() -> Destino {
        Destino::Perfil { usuario: "nasa".into() }
    }

    /// Lo que sale de una petición, por nombre.
    fn campo<'a>(lista: &'a [(&'static str, String)], nombre: &str) -> Option<&'a str> {
        lista.iter().find(|(n, _)| *n == nombre).map(|(_, v)| v.as_str())
    }

    /// La misma página como la vería alguien con sesión: con su `fb_dtsg`,
    /// su id y, para probar el otro camino, sin el token CSRF en la página.
    fn perfil_con_sesion() -> String {
        PERFIL
            .replace("[\"DTSGInitialData\",[],{}", "[\"DTSGInitialData\",[],{\"token\":\"NAcMQ1oUbxYZ3zUPZ0ZvJxQ8Jp8yoKqfYv3Wq9F8KQ7x2b1bZ6JQ:17:1700000000\"}")
            .replace("\"USER_ID\":\"0\"", "\"USER_ID\":\"63000000001\"")
            .replace("{\"csrf_token\":\"lRYHa7pHB_mZgMd14MNyrP\"}", "{}")
    }

    #[test]
    fn el_perfil_trae_su_consulta_y_sus_fichas() {
        let l = leer_html(PERFIL, &nasa()).unwrap();
        assert_eq!(l.publicaciones, 5, "cuatro hilos, uno con su respuesta");
        assert_eq!(l.items.len(), 6, "el carrusel de 4 y dos fotos");
        assert_eq!(l.vistas.len(), 5);
        assert!(l.hay_mas);
        let p = l.paginador.expect("la página trae su consulta");
        assert!(p.hay_mas() && !p.con_sesion());
        assert_eq!(p.doc_id, "28424781820556358");
        assert_eq!(p.consulta, "BarcelonaProfileThreadsTabDirectQuery", "la de los hilos, no la de la cabecera");
        assert_eq!(p.variables["userID"], "28829152388");
        assert!(p.variables.keys().any(|k| k.starts_with("__relay_internal__pv__")));
        assert!(p.cursor.as_deref().is_some_and(|c| c.starts_with("QVFD")), "{:?}", p.cursor);
        assert_eq!(p.lsd, "AdTPbcKXYmvP_c_XyiAaC-b7Ecs");
        assert_eq!(p.csrf, "lRYHa7pHB_mZgMd14MNyrP");
        assert_eq!((p.espectador.as_str(), p.dtsg.as_str()), ("0", ""));
        assert_eq!(p.app_id, "238260118697367");
        assert_eq!((p.rev.as_str(), p.comet.as_str(), p.dpr.as_str()), ("1051607682", "122", "1"));
        assert!(p.hs.contains("barcelona_logged_out_pkg") && !p.hsi.is_empty() && !p.bloks.is_empty());
        assert_eq!(p.ccg, "EXCELLENT");

        // Un post no lleva paginador, y una página sin la consulta tampoco.
        let post = Destino::Post { usuario: "nasa".into(), codigo: "Decc755lYhC".into() };
        assert!(leer_html(PERFIL, &post).unwrap().paginador.is_none());
        assert!(leer_html(&pagina(""), &nasa()).unwrap().paginador.is_none());
    }

    /// Sin sesión, la petición es la de la web a un visitante, campo a campo.
    #[test]
    fn la_peticion_sin_sesion_es_la_de_la_web() {
        let mut p = paginador(PERFIL, "nasa").unwrap();
        let cursor = p.cursor.clone().unwrap();
        let pet = p.peticion("").unwrap();
        let cab = |k| campo(&pet.cabeceras, k);
        let cuerpo = |k| campo(&pet.cuerpo, k);
        assert_eq!(cab("X-LOGGED-OUT-THREADS-MIGRATED-REQUEST"), Some("true"));
        assert_eq!(cab("X-FB-LSD"), Some("AdTPbcKXYmvP_c_XyiAaC-b7Ecs"));
        assert_eq!(cab("X-CSRFToken"), Some("lRYHa7pHB_mZgMd14MNyrP"));
        assert_eq!(cab("X-IG-App-ID"), Some("238260118697367"));
        assert_eq!(cab("X-ASBD-ID"), Some("359341"));
        assert_eq!(cab("X-FB-Friendly-Name"), Some("BarcelonaProfileThreadsTabDirectQuery"));
        assert_eq!(cab("Sec-Fetch-Mode"), Some("cors"));
        assert_eq!(cab("Referer"), Some("https://www.threads.com/@nasa"));
        assert!(cab("X-BLOKS-VERSION-ID").is_some_and(|v| v.starts_with("f6e7112f")));
        assert_eq!((cuerpo("av"), cuerpo("__user"), cuerpo("__a")), (Some("0"), Some("0"), Some("1")));
        assert_eq!(cuerpo("fb_dtsg"), None, "un visitante no tiene");
        assert_eq!(cuerpo("jazoest"), Some("22398"), "la suma del lsd");
        assert_eq!(cuerpo("lsd"), Some("AdTPbcKXYmvP_c_XyiAaC-b7Ecs"));
        assert_eq!(cuerpo("doc_id"), Some("28424781820556358"));
        assert_eq!(cuerpo("fb_api_caller_class"), Some("RelayModern"));
        assert_eq!(cuerpo("server_timestamps"), Some("true"));
        assert_eq!(cuerpo("__comet_req"), Some("122"));
        assert_eq!(cuerpo("__req"), Some("1"));
        let s = cuerpo("__s").unwrap();
        assert_eq!(cab("X-Web-Session-ID"), Some(s));
        assert!(s.len() == 20 && s.split(':').all(|g| g.len() == 6 && g.bytes().all(|b| b.is_ascii_alphanumeric())), "{s}");
        let v: Value = serde_json::from_str(cuerpo("variables").unwrap()).unwrap();
        assert_eq!(v["after"], Value::String(cursor));
        assert_eq!(v["first"], 15);
        assert_eq!(v["userID"], "28829152388");
        assert_eq!(v["__relay_internal__pv__BarcelonaIsLoggedInrelayprovider"], false);
        // Cada petición lleva su número, como en la web.
        assert_eq!(campo(&p.peticion("").unwrap().cuerpo, "__req"), Some("2"));
    }

    /// Con sesión: su `fb_dtsg`, su id, el CSRF de su cookie si la página no
    /// lo trae, y sin la cabecera de visitante. Y nada de eso en un `{:?}`.
    #[test]
    fn con_sesion_lleva_el_dtsg_y_quien_mira() {
        let mut p = paginador(&perfil_con_sesion(), "nasa").unwrap();
        assert!(p.con_sesion());
        let pet = p.peticion("sessionid=s3cr3t; csrftoken=CSRF123; ds_user_id=63000000001").unwrap();
        let cab = |k| campo(&pet.cabeceras, k);
        let cuerpo = |k| campo(&pet.cuerpo, k);
        assert_eq!(cab("X-LOGGED-OUT-THREADS-MIGRATED-REQUEST"), None);
        assert_eq!(cab("X-CSRFToken"), Some("CSRF123"), "el de la cookie");
        assert_eq!((cuerpo("av"), cuerpo("__user")), (Some("63000000001"), Some("63000000001")));
        assert_eq!(cuerpo("fb_dtsg"), Some("NAcMQ1oUbxYZ3zUPZ0ZvJxQ8Jp8yoKqfYv3Wq9F8KQ7x2b1bZ6JQ:17:1700000000"));
        assert_eq!(cuerpo("jazoest"), Some("25095"), "la suma del fb_dtsg, no la del lsd");
        let depurado = format!("{p:?}");
        assert!(!depurado.contains("NAcMQ") && !depurado.contains("AdTPbc"), "{depurado}");
        assert!(depurado.contains("con_sesion: true"), "{depurado}");
    }

    /// Una página de la API: sus publicaciones y el cursor siguiente.
    #[test]
    fn una_pagina_da_sus_publicaciones_y_el_cursor() {
        let mut p = paginador(PERFIL, "nasa").unwrap();
        let mut vistas = HashSet::new();
        let pag = p.leer_pagina(PAGINA, &mut vistas).unwrap();
        assert_eq!(pag.publicaciones, 5);
        assert_eq!(pag.items.len(), 6, "4 del carrusel, la foto y el vídeo");
        let v = pag.items.iter().find(|i| i.is_video).expect("el vídeo");
        assert_eq!((v.post_id.as_str(), v.ext.as_str()), ("DeMqtuYgkaL", "mp4"));
        assert!(pag.items.iter().all(|i| i.author == "nasa"));
        assert!(p.cursor.as_deref().is_some_and(|c| c.starts_with("AQHT1DxH")), "{:?}", p.cursor);
        // La misma página otra vez no trae nada nuevo.
        let otra = p.leer_pagina(PAGINA, &mut vistas).unwrap();
        assert_eq!((otra.publicaciones, otra.items.len()), (0, 0));
    }

    /// MEDIDO: la primera página repite los hilos que ya traía el HTML. Lo
    /// leído en la página del perfil no vuelve a la rejilla.
    #[test]
    fn la_primera_pagina_no_repite_lo_del_html() {
        let mut l = leer_html(PERFIL, &nasa()).unwrap();
        let mut p = l.paginador.take().unwrap();
        let pag = p.leer_pagina(PAGINA, &mut l.vistas).unwrap();
        assert_eq!(pag.publicaciones, 1, "solo el vídeo es nuevo");
        assert_eq!(pag.items.len(), 1);
        assert!(pag.items[0].is_video);
    }

    /// Lo que no es de la cuenta no entra; y el final del perfil, o un cursor
    /// que no avanza, lo dan por acabado en vez de pedir lo mismo sin fin.
    #[test]
    fn el_final_del_perfil_y_los_bucles() {
        let mut v: Value = serde_json::from_str(PAGINA).unwrap();
        v["data"]["mediaData"]["edges"][1]["node"]["thread_items"][0]["post"]["user"]["username"] = "otra.cuenta".into();
        let mut p = paginador(PERFIL, "nasa").unwrap();
        let pag = p.leer_pagina(&v.to_string(), &mut HashSet::new()).unwrap();
        assert_eq!(pag.publicaciones, 4, "la foto de otra cuenta, fuera");

        let fin = PAGINA.replace("\"has_next_page\":true", "\"has_next_page\":false");
        let mut p = paginador(PERFIL, "nasa").unwrap();
        p.leer_pagina(&fin, &mut HashSet::new()).unwrap();
        assert!(!p.hay_mas());
        assert!(p.peticion("").is_none(), "ya no hay nada que pedir");

        // El mismo cursor dos veces: se acaba.
        let mut p = paginador(PERFIL, "nasa").unwrap();
        p.leer_pagina(PAGINA, &mut HashSet::new()).unwrap();
        assert!(p.hay_mas());
        p.leer_pagina(PAGINA, &mut HashSet::new()).unwrap();
        assert!(!p.hay_mas());

        // Una página sin hilos que dice que hay más, también.
        let mut v: Value = serde_json::from_str(PAGINA).unwrap();
        v["data"]["mediaData"]["edges"] = Value::Array(Vec::new());
        let mut p = paginador(PERFIL, "nasa").unwrap();
        p.leer_pagina(&v.to_string(), &mut HashSet::new()).unwrap();
        assert!(!p.hay_mas());
    }

    /// Lo que contesta Meta cuando no hay datos, tal cual llega.
    #[test]
    fn los_errores_de_meta() {
        let mut p = paginador(PERFIL, "nasa").unwrap();
        let mut vistas = HashSet::new();
        // La respuesta real de `/graphql/query` como «con sesión» sin tenerla.
        let sin_usuario = r#"{"errors":[{"message":"execution error","code":4630001,"summary":"Bad Request","description":"User not found","path":["mediaData"],"severity":"CRITICAL"}],"data":null,"extensions":{"is_final":true},"status":"ok"}"#;
        assert_eq!(p.leer_pagina(sin_usuario, &mut vistas).unwrap_err(), FalloPagina::SinDatos("User not found".into()));
        assert_eq!(
            p.leer_pagina("<!DOCTYPE html><html><body>Log in</body></html>", &mut vistas).unwrap_err(),
            FalloPagina::NoEsJson
        );
        assert_eq!(mensaje_de_error(r#"{"message":"checkpoint_required","status":"fail"}"#), "checkpoint_required");
        assert_eq!(mensaje_de_error("no es json"), "");
        // Un error suelto con los datos buenos: la página vale (medido).
        let mut v: Value = serde_json::from_str(PAGINA).unwrap();
        v["errors"] = serde_json::json!([{"message": "A server error field_exception occured.", "severity": "WARNING"}]);
        assert_eq!(p.leer_pagina(&format!("for (;;);{v}"), &mut vistas).unwrap().publicaciones, 5);
        assert!(p.hay_mas());
    }

    #[test]
    fn jazoest_base36_y_cookies() {
        assert_eq!(jazoest("AdTPbcKXYmvP_c_XyiAaC-b7Ecs"), "22398");
        assert_eq!([1, 35, 36, 1295].map(base36), ["1", "z", "10", "zz"]);
        assert_eq!(valor_cookie("a=1; csrftoken=XY z; b=2", "csrftoken"), "XY z");
        assert_eq!(valor_cookie("xcsrftoken=1", "csrftoken"), "");
        assert_ne!(sesion_web_nueva(), "", "y con el formato de la web: ver la prueba de la petición");
    }
}
