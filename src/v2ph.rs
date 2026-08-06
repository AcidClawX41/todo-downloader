//! Extractor nativo de V2PH.
//!
//! POR QUÉ NATIVO Y NO UN MOTOR EXTERNO: el sitio entrega HTML renderizado en
//! el servidor. Sin JavaScript, sin desafío anti-bot y sin sesión. Las URLs de
//! las imágenes vienen tal cual en el marcado y APUNTAN AL ORIGINAL — la misma
//! que obtienes con «guardar imagen como» en el navegador. No hay un endpoint
//! de «tamaño completo» que descubrir ni una miniatura que remontar.
//!
//! Eso hace innecesario todo lo que arrastraría un motor externo (Python,
//! Selenium y un Chrome instalado) y deja el trabajo en lo que la aplicación ya
//! sabe hacer: pedir una página, sacar enlaces y encolarlos en el motor HTTP
//! nativo, que ya trae reanudación, pausa y carpetas por autor.
//!
//! QUÉ SE PARSEA Y QUÉ NO: solo se leen cosas ancladas a un patrón de URL —
//! el CDN de fotos, los enlaces de álbum, el número de página. Los rótulos de
//! metadatos («Fotos», «Modelo», «Agencia») están traducidos a diez idiomas
//! según el parámetro `hl`, así que leerlos por texto sería un parser roto en
//! cuanto alguien tenga otro idioma configurado. El título sale de la etiqueta
//! `og:title`, que no se traduce de sitio.

use regex::Regex;
use std::sync::OnceLock;

/// Hosts admitidos. Comparación exacta: `v2ph.com.atacante.net` NO vale.
const HOSTS: &[&str] = &["v2ph.com", "www.v2ph.com"];

/// Fotos reales de un álbum. Las portadas de las tarjetas viven en
/// `/album/`, una ruta distinta, y por eso las «Galerías relacionadas» del pie
/// no se cuelan como si fueran fotos de este álbum.
fn re_fotos() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"https://cdn\.v2ph\.com/photos/[A-Za-z0-9_\-]+\.(?:jpg|jpeg|png|webp)").unwrap()
    })
}

/// Portada de una tarjeta de álbum en una página de listado.
fn re_portadas() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"https://cdn\.v2ph\.com/album/[A-Za-z0-9_\-]+\.(?:jpg|jpeg|png|webp)").unwrap()
    })
}

/// Enlace a un álbum. Existen DOS formas y las dos están en producción:
/// `/album/YTY-12258` (con prefijo de agencia) y `/album/z6m47xma.html`
/// (identificador opaco). Un extractor que solo cubra una se deja la mitad.
fn re_album_href() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"href="(?:https://www\.v2ph\.com)?/album/([A-Za-z0-9_\-]+(?:\.html)?)""#).unwrap())
}

/// Cualquier `?page=N` o `&page=N` del marcado.
fn re_page() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[?&]page=(\d+)").unwrap())
}

fn re_og_title() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"<meta[^>]+property="og:title"[^>]+content="([^"]*)""#).unwrap()
    })
}

/// Enlace a la ficha de una modelo, para nombrar la carpeta de destino.
fn re_actor_href() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"href="(?:https://www\.v2ph\.com)?/actor/([A-Za-z0-9_\-]+(?:\.html)?)""#).unwrap())
}

/// Qué clase de página de V2PH es una URL.
#[derive(Debug, Clone, PartialEq)]
pub enum V2phUrl {
    /// Un álbum concreto. `page` es la página DENTRO del álbum (10 fotos cada una).
    Album { id: String, page: u32 },
    /// Listado de álbumes: modelo, agencia, categoría o país. Todos comparten
    /// exactamente la misma estructura de tarjeta y de paginación.
    Listing { kind: String, slug: String, page: u32 },
}

/// Extrae el host de una URL sin depender de un parser completo.
fn host_of(url: &str) -> Option<String> {
    let resto = url.split_once("://")?.1;
    let autoridad = resto.split(['/', '?', '#']).next()?;
    // Descartar credenciales embebidas: `usuario:clave@host`
    let host = autoridad.rsplit('@').next()?;
    // Descartar puerto y el punto final de un FQDN absoluto
    let host = host.split(':').next()?.trim_end_matches('.');
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn es_v2ph(host: &str) -> bool {
    HOSTS.contains(&host)
}

/// ¿Esta URL la sabe resolver este módulo?
pub fn is_v2ph(url: &str) -> bool {
    classify(url).is_some()
}

/// Clasifica una URL de V2PH. Devuelve `None` si no lo es o si es una sección
/// que no sabemos enumerar (login, búsqueda, portada…).
pub fn classify(url: &str) -> Option<V2phUrl> {
    let host = host_of(url)?;
    if !es_v2ph(&host) {
        return None;
    }

    let page = re_page()
        .captures(url)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(1)
        .max(1);

    let ruta = url.split_once("://")?.1;
    let ruta = ruta.find('/').map(|i| &ruta[i..])?;
    let ruta = ruta.split(['?', '#']).next()?;
    let mut trozos = ruta.trim_matches('/').split('/');
    let primero = trozos.next()?;
    let segundo = trozos.next().unwrap_or("");

    if segundo.is_empty() {
        return None;
    }

    match primero {
        "album" => Some(V2phUrl::Album { id: segundo.to_string(), page }),
        "actor" | "company" | "category" | "country" => Some(V2phUrl::Listing {
            kind: primero.to_string(),
            slug: segundo.to_string(),
            page,
        }),
        _ => None,
    }
}

/// Reconstruye la URL de una página concreta de un álbum.
pub fn album_url(id: &str, page: u32) -> String {
    if page <= 1 {
        format!("https://www.v2ph.com/album/{id}")
    } else {
        format!("https://www.v2ph.com/album/{id}?page={page}")
    }
}

/// Reconstruye la URL de una página concreta de un listado.
pub fn listing_url(kind: &str, slug: &str, page: u32) -> String {
    if page <= 1 {
        format!("https://www.v2ph.com/{kind}/{slug}")
    } else {
        format!("https://www.v2ph.com/{kind}/{slug}?page={page}")
    }
}

/// Última página según los enlaces de paginación.
///
/// Se toma el MÁXIMO de todos los `page=N` del documento en vez de buscar el
/// rótulo «Último», que está traducido. Si no hay paginación, es 1.
pub fn last_page(html: &str) -> u32 {
    re_page()
        .captures_iter(html)
        .filter_map(|c| c.get(1)?.as_str().parse::<u32>().ok())
        .max()
        .unwrap_or(1)
        .max(1)
}

/// Título del documento, sin traducir.
pub fn title(html: &str) -> String {
    re_og_title()
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| decode_entities(m.as_str()))
        .unwrap_or_default()
}

/// Identificador de la modelo a la que pertenece la página, si aparece.
/// Sirve para nombrar la carpeta de destino.
pub fn actor_slug(html: &str) -> Option<String> {
    re_actor_href()
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim_end_matches(".html").to_string())
}

/// Fotos de una página de álbum, en orden y sin repetir.
///
/// El mismo `src` puede aparecer dos veces (por ejemplo en un `srcset`), y
/// duplicar una foto en la cola significa descargarla dos veces.
pub fn album_photos(html: &str) -> Vec<String> {
    let mut vistas = std::collections::HashSet::new();
    re_fotos()
        .find_iter(html)
        .map(|m| m.as_str().to_string())
        .filter(|u| vistas.insert(u.clone()))
        .collect()
}

/// Una tarjeta de álbum dentro de una página de listado.
#[derive(Debug, Clone, PartialEq)]
pub struct AlbumCard {
    /// Identificador tal cual va en `/album/{id}`
    pub id: String,
    pub url: String,
    /// Portada, para la rejilla. Puede faltar.
    pub cover: String,
}

/// Álbumes de una página de listado, en el orden en que aparecen.
///
/// Los enlaces de álbum y las portadas se emparejan POR POSICIÓN: en el
/// marcado cada tarjeta es una portada seguida de su enlace. Si esa
/// correspondencia se rompe alguna vez, se pierde la miniatura pero NO el
/// enlace, que es lo que de verdad importa para descargar.
pub fn listing_albums(html: &str) -> Vec<AlbumCard> {
    let mut vistos = std::collections::HashSet::new();
    let ids: Vec<String> = re_album_href()
        .captures_iter(html)
        .filter_map(|c| Some(c.get(1)?.as_str().to_string()))
        .filter(|id| vistos.insert(id.clone()))
        .collect();

    let covers: Vec<String> = re_portadas().find_iter(html).map(|m| m.as_str().to_string()).collect();

    ids.into_iter()
        .enumerate()
        .map(|(i, id)| AlbumCard {
            url: album_url(&id, 1),
            cover: covers.get(i).cloned().unwrap_or_default(),
            id,
        })
        .collect()
}

/// Entidades HTML que aparecen de verdad en títulos de este sitio.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// ¿Esta página es el muro de acceso en vez del contenido?
///
/// V2PH deja ver la PRIMERA página de un álbum a cualquiera, pero a partir de
/// la segunda responde con un formulario de acceso y el mensaje «You need to
/// log in to view more content of this album». Sin detectarlo, un álbum de 38
/// fotos parecía tener 10 y nadie sabía por qué.
///
/// El texto del mensaje está traducido a diez idiomas, así que NO se busca por
/// texto. El enlace de recuperación de contraseña solo aparece dentro de ese
/// formulario, y su ruta es la misma en todos los idiomas.
pub fn requiere_sesion(html: &str) -> bool {
    html.contains("/site/recovery-password") && album_photos(html).is_empty()
}

/// Cookies de V2PH a partir de un archivo en formato Netscape (cookies.txt).
///
/// Es el mismo archivo que ya se usa para Instagram y Weibo, así que el usuario
/// no tiene que aprender nada nuevo ni escribir una contraseña en ningún sitio:
/// la sesión sale de su propio navegador y la aplicación no guarda credenciales.
///
/// Formato: siete campos separados por tabulador —
/// `dominio  incluir_subdominios  ruta  seguro  caducidad  nombre  valor`.
/// Las líneas que empiezan por `#` son comentarios, salvo `#HttpOnly_`, que
/// Chrome antepone al dominio y hay que quitar en vez de descartar la línea.
pub fn cookie_header(cookies_txt: &str) -> Option<String> {
    let mut pares: Vec<String> = Vec::new();
    for linea in cookies_txt.lines() {
        let linea = linea.strip_prefix("#HttpOnly_").unwrap_or(linea);
        if linea.starts_with('#') || linea.trim().is_empty() {
            continue;
        }
        let campos: Vec<&str> = linea.split('\t').collect();
        if campos.len() < 7 {
            continue;
        }
        let dominio = campos[0].trim_start_matches('.').to_ascii_lowercase();
        // Solo cookies de V2PH: no se filtra la sesión de otros sitios
        if dominio != "v2ph.com" && !dominio.ends_with(".v2ph.com") {
            continue;
        }
        let (nombre, valor) = (campos[5].trim(), campos[6].trim());
        if nombre.is_empty() {
            continue;
        }
        pares.push(format!("{nombre}={valor}"));
    }
    if pares.is_empty() {
        None
    } else {
        Some(pares.join("; "))
    }
}

// ============================ Formulario de acceso ============================
//
// NO SE FIJAN NOMBRES DE CAMPO. El formulario se lee del HTML en cada intento:
// se recogen TODOS los `<input>` tal cual están —incluidos los ocultos, que es
// donde vive el testigo anti-CSRF— y solo se identifican por su `type` cuál es
// el usuario, cuál la contraseña y cuál la casilla de recordar.
//
// La alternativa era codificar «_csrf» y «LoginForm[username]» a mano. Eso
// funciona hasta que el sitio renombra un campo, y entonces falla con un error
// que no dice nada. Leerlo del propio formulario cuesta lo mismo y aguanta.

/// Formulario de acceso ya interpretado.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginForm {
    /// Destino del envío, ya absoluto
    pub action: String,
    /// Campos ocultos y sus valores, tal cual venían (testigo CSRF incluido)
    pub ocultos: Vec<(String, String)>,
    pub campo_usuario: String,
    pub campo_clave: String,
    /// Casilla «recordarme», si existe. Se marca siempre: sin ella el sitio
    /// entrega una cookie de sesión que muere al cerrar, y toda esta gestión
    /// perdería el sentido a la primera.
    pub campo_recordar: Option<String>,
}

fn re_form() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<form\b([^>]*)>(.*?)</form>").unwrap())
}

fn re_input() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<input\b([^>]*)>").unwrap())
}

/// Valor de un atributo dentro de una etiqueta, con comillas simples o dobles.
fn atributo(etiqueta: &str, nombre: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?is)\b{}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#,
        regex::escape(nombre)
    ))
    .ok()?;
    let c = re.captures(etiqueta)?;
    let v = c
        .get(1)
        .or_else(|| c.get(2))
        .or_else(|| c.get(3))?
        .as_str();
    Some(decode_entities(v))
}

/// Interpreta el formulario de acceso de una página.
///
/// Se elige el formulario que contenga un campo de contraseña: una página de
/// login suele traer también el buscador de cabecera, y enviar las credenciales
/// ahí sería, además de inútil, mandarlas a donde no toca.
pub fn parse_login_form(html: &str, base: &str) -> Option<LoginForm> {
    for c in re_form().captures_iter(html) {
        let attrs = c.get(1)?.as_str();
        let cuerpo = c.get(2)?.as_str();

        let mut ocultos: Vec<(String, String)> = Vec::new();
        let mut campo_usuario: Option<String> = None;
        let mut campo_clave: Option<String> = None;
        let mut campo_recordar: Option<String> = None;

        for i in re_input().captures_iter(cuerpo) {
            let etiqueta = i.get(1)?.as_str();
            let Some(nombre) = atributo(etiqueta, "name") else { continue };
            if nombre.is_empty() {
                continue;
            }
            let tipo = atributo(etiqueta, "type").unwrap_or_default().to_ascii_lowercase();
            let valor = atributo(etiqueta, "value").unwrap_or_default();

            match tipo.as_str() {
                "password" => campo_clave = Some(nombre),
                "hidden" => ocultos.push((nombre, valor)),
                "checkbox" => {
                    // La de recordar es la única casilla que interesa marcar
                    if campo_recordar.is_none() {
                        campo_recordar = Some(nombre);
                    }
                }
                "submit" | "button" | "reset" | "image" => {}
                // text, email o sin tipo: el primero es el usuario
                _ => {
                    if campo_usuario.is_none() {
                        campo_usuario = Some(nombre);
                    }
                }
            }
        }

        let (Some(campo_usuario), Some(campo_clave)) = (campo_usuario, campo_clave) else {
            continue; // no es el formulario de acceso
        };

        let action = atributo(attrs, "action").unwrap_or_default();
        let action = absoluta(&action, base);

        return Some(LoginForm { action, ocultos, campo_usuario, campo_clave, campo_recordar });
    }
    None
}

/// Convierte un `action` relativo en absoluto contra la URL de la página.
fn absoluta(action: &str, base: &str) -> String {
    let a = action.trim();
    if a.is_empty() {
        return base.to_string();
    }
    if a.starts_with("http://") || a.starts_with("https://") {
        return a.to_string();
    }
    if let Some(resto) = a.strip_prefix('/') {
        // Raíz del sitio: esquema + host de `base`
        let sin_esquema = base.split_once("://").map(|(e, r)| (e, r));
        if let Some((esquema, resto_base)) = sin_esquema {
            let host = resto_base.split('/').next().unwrap_or("");
            return format!("{esquema}://{host}/{resto}");
        }
    }
    // Relativa al directorio actual
    let corte = base.rfind('/').map(|i| i + 1).unwrap_or(base.len());
    format!("{}{a}", &base[..corte])
}

/// Funde cabeceras `Set-Cookie` sobre un estado previo de cookies.
///
/// Solo interesa `nombre=valor`: los atributos (`Path`, `HttpOnly`, `Expires`…)
/// gobiernan al navegador, no a nosotros. Una cookie repetida gana la última,
/// que es lo que hace cualquier cliente.
pub fn merge_cookies(previas: &str, set_cookie: &[String]) -> String {
    let mut orden: Vec<String> = Vec::new();
    let mut valores: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let mut anotar = |par: &str| {
        let par = par.split(';').next().unwrap_or("").trim();
        let Some((n, v)) = par.split_once('=') else { return };
        let n = n.trim().to_string();
        if n.is_empty() {
            return;
        }
        if !valores.contains_key(&n) {
            orden.push(n.clone());
        }
        valores.insert(n, v.trim().to_string());
    };

    for par in previas.split(';') {
        anotar(par);
    }
    for sc in set_cookie {
        anotar(sc);
    }

    orden
        .into_iter()
        .filter_map(|n| valores.get(&n).map(|v| format!("{n}={v}")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// ¿Esto es el desafío anti-bot de Cloudflare y no la página pedida?
///
/// V2PH protege `/login` con la verificación de Cloudflare, aunque las páginas
/// de álbum pasen sin ella. La respuesta es un 403 con el título «Just a
/// moment...» y un script que hay que ejecutar para superarla.
///
/// Detectarlo importa porque cambia por completo el consejo: no es una
/// contraseña mal escrita ni un ajuste, es que el acceso automatizado a esa
/// ruta concreta está cerrado por diseño.
pub fn es_desafio_cloudflare(html: &str) -> bool {
    html.contains("Just a moment")
        || html.contains("cf-browser-verification")
        || html.contains("__cf_chl")
        || html.contains("challenges.cloudflare.com")
}

/// ¿La página muestra que hay sesión iniciada?
///
/// Señal independiente del idioma: el enlace de cierre de sesión solo existe
/// para quien ha entrado, y el de registro solo para quien no. Comprobar el
/// texto («Mi cuenta», «我的账户»…) sería atarse a las diez traducciones.
pub fn sesion_iniciada(html: &str) -> bool {
    (html.contains("/logout") || html.contains("/site/logout")) && !html.contains("/site/recovery-password")
}


#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN: &str = r#"
      <form action="/site/search" method="get">
        <input type="text" name="q" value="">
      </form>
      <form id="login-form" action="/login" method="post">
        <input type="hidden" name="_csrf-frontend" value="AbC123==">
        <input type="text" name="LoginForm[username]" value="">
        <input type="password" name="LoginForm[password]">
        <input type="checkbox" name="LoginForm[rememberMe]" value="1">
        <button type="submit">Login</button>
      </form>"#;

    #[test]
    fn el_formulario_se_lee_del_html_sin_fijar_nombres() {
        let f = parse_login_form(LOGIN, "https://www.v2ph.com/login").unwrap();
        assert_eq!(f.action, "https://www.v2ph.com/login");
        assert_eq!(f.campo_usuario, "LoginForm[username]");
        assert_eq!(f.campo_clave, "LoginForm[password]");
        assert_eq!(f.campo_recordar.as_deref(), Some("LoginForm[rememberMe]"));
        // El testigo anti-CSRF viaja tal cual, sin conocer su nombre
        assert_eq!(f.ocultos, vec![("_csrf-frontend".to_string(), "AbC123==".to_string())]);
    }

    #[test]
    fn el_buscador_de_cabecera_no_se_confunde_con_el_acceso() {
        // Sin campo de contraseña no es el formulario de acceso
        let solo_buscador = r#"<form action="/s"><input type="text" name="q"></form>"#;
        assert_eq!(parse_login_form(solo_buscador, "https://www.v2ph.com/login"), None);
    }

    #[test]
    fn las_acciones_relativas_se_resuelven() {
        assert_eq!(absoluta("/login", "https://www.v2ph.com/site/x"), "https://www.v2ph.com/login");
        assert_eq!(absoluta("", "https://www.v2ph.com/login"), "https://www.v2ph.com/login");
        assert_eq!(
            absoluta("https://otro.com/x", "https://www.v2ph.com/login"),
            "https://otro.com/x"
        );
        assert_eq!(absoluta("entrar", "https://www.v2ph.com/site/login"), "https://www.v2ph.com/site/entrar");
    }

    #[test]
    fn las_cookies_se_funden_y_la_ultima_gana() {
        let r = merge_cookies(
            "PHPSESSID=viejo; idioma=es",
            &[
                "PHPSESSID=nuevo; Path=/; HttpOnly".into(),
                "_identity=abc; Expires=Thu, 01 Jan 2030 00:00:00 GMT".into(),
            ],
        );
        assert!(r.contains("PHPSESSID=nuevo"));
        assert!(!r.contains("viejo"));
        assert!(r.contains("_identity=abc"));
        assert!(r.contains("idioma=es"));
        // Los atributos no viajan
        assert!(!r.contains("HttpOnly"));
        assert!(!r.contains("Path"));
    }

    #[test]
    fn se_detecta_la_sesion_por_estructura_no_por_texto() {
        assert!(sesion_iniciada(r#"<a href="/logout">Cerrar sesión</a>"#));
        assert!(!sesion_iniciada(
            r#"<a href="/signup">Alta</a><a href="/site/recovery-password">?</a>"#
        ));
        assert!(!sesion_iniciada("<html></html>"));
    }

    #[test]
    fn detecta_el_muro_de_acceso() {
        let muro = r#"<h1>Page 2</h1>
          <p>You need to log in to view more content of this album.</p>
          <a href="https://www.v2ph.com/site/recovery-password">Forgot Password?</a>
          <img src="https://cdn.v2ph.com/album/frxV3GyD_X1oED5F.jpg">"#;
        assert!(requiere_sesion(muro));
        // La portada del pie NO cuenta como foto del álbum
        assert!(album_photos(muro).is_empty());
        // Una página con fotos no es un muro, aunque lleve enlaces de cuenta
        assert!(!requiere_sesion(ALBUM));
    }

    #[test]
    fn lee_cookies_de_v2ph_y_solo_de_v2ph() {
        let txt = "# Netscape HTTP Cookie File\n\
            .v2ph.com\tTRUE\t/\tFALSE\t1799999999\t_identity\tabc123\n\
            #HttpOnly_.v2ph.com\tTRUE\t/\tTRUE\t1799999999\tPHPSESSID\txyz\n\
            .instagram.com\tTRUE\t/\tFALSE\t1799999999\tsessionid\tNOPE\n\
            linea basura sin tabuladores\n";
        let h = cookie_header(txt).unwrap();
        assert!(h.contains("_identity=abc123"));
        // El prefijo #HttpOnly_ de Chrome no debe descartar la cookie
        assert!(h.contains("PHPSESSID=xyz"));
        // Jamás se envía la sesión de otro sitio
        assert!(!h.contains("NOPE"));
        assert!(!h.contains("instagram"));
    }

    #[test]
    fn sin_cookies_de_v2ph_no_se_manda_cabecera() {
        assert_eq!(cookie_header(""), None);
        assert_eq!(
            cookie_header(".instagram.com\tTRUE\t/\tFALSE\t1\tsessionid\tx"),
            None
        );
    }

    #[test]
    fn clasifica_las_dos_formas_de_url_de_album() {
        assert_eq!(
            classify("https://www.v2ph.com/album/YTY-12258"),
            Some(V2phUrl::Album { id: "YTY-12258".into(), page: 1 })
        );
        // La forma con identificador opaco también existe en producción
        assert_eq!(
            classify("https://www.v2ph.com/album/z6m47xma.html"),
            Some(V2phUrl::Album { id: "z6m47xma.html".into(), page: 1 })
        );
        assert_eq!(
            classify("https://www.v2ph.com/album/YTY-12258?page=3"),
            Some(V2phUrl::Album { id: "YTY-12258".into(), page: 3 })
        );
    }

    #[test]
    fn clasifica_los_cuatro_tipos_de_listado() {
        for (kind, slug, url) in [
            ("actor", "n6oxon8m.html", "https://www.v2ph.com/actor/n6oxon8m.html"),
            ("company", "XIUREN", "https://www.v2ph.com/company/XIUREN"),
            ("category", "cosplay", "https://www.v2ph.com/category/cosplay"),
            ("country", "japan", "https://www.v2ph.com/country/japan"),
        ] {
            assert_eq!(
                classify(url),
                Some(V2phUrl::Listing { kind: kind.into(), slug: slug.into(), page: 1 }),
                "fallo con {url}"
            );
        }
    }

    #[test]
    fn rechaza_dominios_impostores_y_secciones_no_enumerables() {
        assert_eq!(classify("https://v2ph.com.atacante.net/album/X"), None);
        assert_eq!(classify("https://notv2ph.com/album/X"), None);
        assert_eq!(classify("https://v2ph.company/album/X"), None);
        // Secciones que no sabemos enumerar
        assert_eq!(classify("https://www.v2ph.com/login"), None);
        assert_eq!(classify("https://www.v2ph.com/"), None);
        assert_eq!(classify("https://www.v2ph.com/album/"), None);
        // Otros sitios
        assert_eq!(classify("https://www.instagram.com/alguien/"), None);
    }

    #[test]
    fn el_host_se_compara_entero() {
        assert_eq!(host_of("https://www.v2ph.com/album/X").as_deref(), Some("www.v2ph.com"));
        assert_eq!(host_of("https://usuario:clave@v2ph.com/x").as_deref(), Some("v2ph.com"));
        assert_eq!(host_of("https://v2ph.com.:443/x").as_deref(), Some("v2ph.com"));
        assert_eq!(host_of("v2ph.com/album/X"), None); // sin esquema
    }

    const ALBUM: &str = r#"
      <meta property="og:title" content="[Yituyu] Zombie &amp; friends">
      <img src="https://cdn.v2ph.com/photos/ijtOmAjwkL7G88SW.jpg">
      <img src="https://cdn.v2ph.com/photos/jxxaG7B4uF5tPSod.jpg">
      <img src="https://cdn.v2ph.com/photos/ijtOmAjwkL7G88SW.jpg">
      <a href="/actor/n6oxon8m.html">Xia</a>
      <a href="https://www.v2ph.com/album/YTY-12258?page=2">2</a>
      <a href="https://www.v2ph.com/album/YTY-12258?page=4">Last</a>
      <h3>Related</h3>
      <a href="/album/XiuRen-2148"><img src="https://cdn.v2ph.com/album/5ryzuhe5mf1yKf6d.jpg"></a>
    "#;

    #[test]
    fn las_portadas_de_relacionados_no_se_cuelan_como_fotos() {
        let fotos = album_photos(ALBUM);
        // Tres etiquetas <img> de /photos/, pero una repetida
        assert_eq!(fotos.len(), 2);
        assert!(fotos.iter().all(|u| u.contains("/photos/")));
        // La portada de la galería relacionada vive en /album/ y queda fuera
        assert!(!fotos.iter().any(|u| u.contains("5ryzuhe5mf1yKf6d")));
    }

    #[test]
    fn la_ultima_pagina_sale_del_maximo_no_del_rotulo() {
        assert_eq!(last_page(ALBUM), 4);
        assert_eq!(last_page("<html>sin paginacion</html>"), 1);
    }

    #[test]
    fn titulo_y_modelo() {
        assert_eq!(title(ALBUM), "[Yituyu] Zombie & friends");
        assert_eq!(actor_slug(ALBUM).as_deref(), Some("n6oxon8m"));
    }

    #[test]
    fn un_listado_empareja_portada_con_album() {
        let html = r#"
          <a href="/album/A-1"><img src="https://cdn.v2ph.com/album/aaa.jpg"></a>
          <a href="/album/A-2"><img src="https://cdn.v2ph.com/album/bbb.jpg"></a>
          <a href="https://www.v2ph.com/company/X?page=703">Last</a>
        "#;
        let v = listing_albums(html);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "A-1");
        assert_eq!(v[0].url, "https://www.v2ph.com/album/A-1");
        assert_eq!(v[0].cover, "https://cdn.v2ph.com/album/aaa.jpg");
        assert_eq!(v[1].id, "A-2");
        assert_eq!(last_page(html), 703);
    }

    #[test]
    fn las_urls_se_reconstruyen_sin_page_en_la_primera() {
        assert_eq!(album_url("YTY-1", 1), "https://www.v2ph.com/album/YTY-1");
        assert_eq!(album_url("YTY-1", 2), "https://www.v2ph.com/album/YTY-1?page=2");
        assert_eq!(listing_url("actor", "abc.html", 1), "https://www.v2ph.com/actor/abc.html");
        assert_eq!(
            listing_url("company", "XIUREN", 5),
            "https://www.v2ph.com/company/XIUREN?page=5"
        );
    }
}
