//! Descubridor de artistas: del nombre de un personaje a los perfiles que lo
//! dibujan.
//!
//! LA IDEA, EN UNA FRASE: el booru no es el destino, **es el índice que ya
//! existe**. Cada post publica el campo `source` con el enlace al post
//! ORIGINAL del artista, así que una etiqueta de personaje es, de hecho, una
//! tabla de referencias cruzadas hacia X, Pixiv, Patreon y Fanbox mantenida por
//! miles de personas que ya hacen ese trabajo.
//!
//! Medido sobre 300 posts de `yukinoshita_yukino` en yande.re antes de escribir
//! una línea:
//!
//! ```text
//!  19 posts   x.com/ponkan_8      ← el ilustrador de la obra
//!   3 posts   x.com/emuzu100
//!   2 posts   x.com/inanakisiki
//! ```
//!
//! POR QUÉ ORDENAR POR NÚMERO DE POSTS y no por popularidad global: quien ha
//! dibujado a ese personaje veinte veces interesa más que una cuenta enorme que
//! lo dibujó una. Y de regalo, ese orden filtra solo: las cuentas oficiales y
//! las tiendas —`AMNIBUS_STORE`, `anime_oregairu`— se quedan abajo con uno o
//! dos, sin necesidad de una lista negra que mantener.

use serde_json::Value;

/// Dónde publica un artista.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sitio {
    X,
    Pixiv,
    Patreon,
    Fanbox,
    Bluesky,
}

impl Sitio {
    /// ¿Se puede bajar sin pagarle a ESE creador en concreto?
    ///
    /// X, Bluesky y Pixiv son abiertos: basta con la sesión que ya tengas.
    /// Patreon y Fanbox cobran **por creador**, así que un perfil suyo puede
    /// ser inútil aunque estés suscrito a otros diez. Por eso, dentro de un
    /// mismo artista, los abiertos se enseñan primero: son los que puedes usar
    /// ahora mismo.
    pub fn abierto(self) -> bool {
        matches!(self, Sitio::X | Sitio::Bluesky | Sitio::Pixiv)
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Sitio::X => "X",
            Sitio::Pixiv => "Pixiv",
            Sitio::Patreon => "Patreon",
            Sitio::Fanbox => "Fanbox",
            Sitio::Bluesky => "Bluesky",
        }
    }
}

/// El perfil de un artista, ya normalizado a su portada.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Perfil {
    pub sitio: Sitio,
    /// Identificador dentro del sitio: `ponkan_8`, `real_haku89`, `@ateoyh`…
    pub id: String,
    /// URL lista para pegar en la pestaña Perfil.
    pub url: String,
}

/// Un artista con lo que sabemos de él tras cosechar una etiqueta.
#[derive(Clone, Debug)]
pub struct Artista {
    /// Todos los sitios donde se le ha visto publicar, **abiertos primero**.
    ///
    /// Un artista suele tener varias casas: la misma persona sube a X y cobra
    /// en Fanbox. Agruparlas es justo el trabajo manual que esta pestaña
    /// existe para evitar — y sobre todo, es lo que hace útil a un artista
    /// cuyo Fanbox no puedes abrir porque no le pagas a él.
    ///
    /// INVARIANTE: nunca está vacío. No es una promesa escrita, es que el
    /// único constructor —`Artista::nuevo`— exige un perfil, así que el estado
    /// inválido no se puede ni construir. Antes esto era un `Vec` que se
    /// llenaba después y `principal()` indexaba el `[0]`: correcto por
    /// casualidad, y a un refactor de distancia de un pánico.
    perfiles: Vec<Perfil>,
    /// Cuántos posts del personaje buscado son suyos, sumando sus sitios.
    pub posts: u32,
    /// Hasta cuatro miniaturas del booru, para reconocerle de un vistazo.
    pub muestras: Vec<String>,
}

impl Artista {
    /// Crea un artista a partir de su primer perfil. Único constructor: es lo
    /// que garantiza que `perfiles` nunca esté vacío.
    fn nuevo(perfil: Perfil) -> Self {
        Self { perfiles: vec![perfil], posts: 0, muestras: Vec::new() }
    }

    /// El perfil que se ofrece por defecto: el primero abierto que haya.
    ///
    /// No puede fallar: el constructor exige un perfil y nada los quita.
    pub fn principal(&self) -> &Perfil {
        &self.perfiles[0]
    }

    /// Todas sus casas, abiertas primero.
    pub fn perfiles(&self) -> &[Perfil] {
        &self.perfiles
    }
}

/// Un post del booru, reducido a lo que hace falta aquí.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostBooru {
    pub source: String,
    pub preview: String,
}

/// Host de una URL, en minúsculas y sin `www.`.
///
/// Se compara el host ENTERO y nunca por subcadena. `x.com` está dentro de
/// `netflix.com` y de `vox.com`, y ese descuido ya costó un fallo en el
/// enrutado de galerías de la v1.7.0.
fn host_de(url: &str) -> Option<String> {
    let resto = url.split_once("://")?.1;
    let host = resto.split(['/', '?', '#']).next()?;
    // Descartar credenciales y puerto: `usuario@host:443`
    let host = host.rsplit('@').next()?.split(':').next()?;
    let host = host.trim_start_matches("www.").to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn host_es(host: &str, sufijo: &str) -> bool {
    host == sufijo || host.ends_with(&format!(".{sufijo}"))
}

/// Segmentos de la ruta, sin los vacíos.
fn segmentos(url: &str) -> Vec<&str> {
    let sin_esquema = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let ruta = sin_esquema.split(['?', '#']).next().unwrap_or("");
    ruta.split('/').skip(1).filter(|s| !s.is_empty()).collect()
}

/// Un identificador de usuario plausible.
///
/// Evita tomar por artista a palabras de la ruta como `status`, `posts` o
/// `artworks`, y descarta cualquier cosa con caracteres que ningún sitio de
/// estos admite en un nombre.
fn id_plausible(s: &str) -> bool {
    const RESERVADAS: &[&str] = &[
        "status", "statuses", "posts", "post", "artworks", "artwork", "profile", "i", "intent",
        "home", "search", "c", "en", "ja", "member", "users", "user",
    ];
    !s.is_empty()
        && s.len() <= 64
        && !RESERVADAS.contains(&s.to_ascii_lowercase().as_str())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// De la URL de UN post de un artista, deduce su PERFIL.
///
/// Formas medidas contra datos reales de yande.re, no supuestas:
///
/// | Fuente | Perfil |
/// |:--|:--|
/// | `x.com/ponkan_8/status/2075…` | `https://x.com/ponkan_8` |
/// | `patreon.com/real_haku89/posts/166…` | `https://www.patreon.com/real_haku89` |
/// | `patreon.com/c/MenikaEloise` | `https://www.patreon.com/c/MenikaEloise` |
/// | `fanbox.cc/@ateoyh/posts/123…` | `https://www.fanbox.cc/@ateoyh` |
/// | `agm94786.fanbox.cc/posts/120…` | `https://www.fanbox.cc/@agm94786` |
/// | `bsky.app/profile/X/post/N` | `https://bsky.app/profile/X` |
/// | `pixiv.net/artworks/148…` | **`None`** |
///
/// PIXIV DEVUELVE `None` A PROPÓSITO, y es el caso más frecuente: la URL de una
/// obra **no contiene al autor**, así que resolverlo exigiría una petición por
/// obra. Mentir aquí sería peor que reconocerlo: la interfaz puede ofrecer el
/// enlace al post y dejar que el usuario decida, en vez de inventarse un perfil
/// que no está en el dato.
pub fn perfil_de_fuente(source: &str) -> Option<Perfil> {
    let url = source.trim();
    if !url.starts_with("http") {
        return None;
    }
    let host = host_de(url)?;
    let seg = segmentos(url);

    // --- X / Twitter: x.com/<usuario>/status/<id> ---
    if host_es(&host, "x.com") || host_es(&host, "twitter.com") {
        let id = seg.first()?;
        if !id_plausible(id) {
            return None;
        }
        return Some(Perfil {
            sitio: Sitio::X,
            id: (*id).to_string(),
            url: format!("https://x.com/{id}"),
        });
    }

    // --- Patreon: /<usuario>/posts/<id> y la forma nueva /c/<usuario> ---
    if host_es(&host, "patreon.com") {
        let (id, url) = match seg.as_slice() {
            ["c", u, ..] if id_plausible(u) => {
                ((*u).to_string(), format!("https://www.patreon.com/c/{u}"))
            }
            [u, ..] if id_plausible(u) => {
                ((*u).to_string(), format!("https://www.patreon.com/{u}"))
            }
            _ => return None,
        };
        return Some(Perfil { sitio: Sitio::Patreon, id, url });
    }

    // --- Fanbox: dos formas, y las dos aparecen en los datos ---
    if host_es(&host, "fanbox.cc") {
        // `<usuario>.fanbox.cc/posts/N`.
        //
        // SE CANONICALIZA A LA FORMA CON @, y no es cosmético: los dos formatos
        // conviven en los datos reales, así que sin unificarlos el MISMO
        // artista aparecía dos veces en la lista, con sus posts repartidos
        // entre las dos entradas. Se veía en `inanakisiki`, que salía como
        // `www.fanbox.cc/@inanakisiki` y como `inanakisiki.fanbox.cc`.
        if host != "fanbox.cc" {
            let id = host.trim_end_matches(".fanbox.cc").to_string();
            if id_plausible(&id) {
                return Some(Perfil {
                    sitio: Sitio::Fanbox,
                    url: format!("https://www.fanbox.cc/@{id}"),
                    id,
                });
            }
            return None;
        }
        // `fanbox.cc/@usuario/posts/N`
        let primero = seg.first()?;
        let id = primero.strip_prefix('@')?;
        if !id_plausible(id) {
            return None;
        }
        return Some(Perfil {
            sitio: Sitio::Fanbox,
            id: id.to_string(),
            url: format!("https://www.fanbox.cc/@{id}"),
        });
    }

    // --- Bluesky: bsky.app/profile/<handle>/post/<id> ---
    if host_es(&host, "bsky.app") {
        if let ["profile", h, ..] = seg.as_slice() {
            if id_plausible(h) {
                return Some(Perfil {
                    sitio: Sitio::Bluesky,
                    id: (*h).to_string(),
                    url: format!("https://bsky.app/profile/{h}"),
                });
            }
        }
        return None;
    }

    // --- Pixiv ---
    //
    // Su URL de OBRA (`/artworks/N`) no lleva al autor y por eso no se
    // resuelve. Pero algunos posts citan directamente el PERFIL, y esos sí:
    // `/users/12345` es la forma moderna y `member.php?id=12345` la antigua,
    // que sigue apareciendo en fuentes viejas de los boorus.
    if host_es(&host, "pixiv.net") {
        // Pixiv antepone el idioma en la ruta: `/en/users/123`. Se salta, o
        // el patrón de abajo no casaría con media web del sitio.
        let seg: &[&str] = match seg.as_slice() {
            [l, resto @ ..] if l.len() == 2 && l.chars().all(|c| c.is_ascii_alphabetic()) => resto,
            todo => todo,
        };
        if let ["users", id, ..] = seg {
            if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
                return Some(Perfil {
                    sitio: Sitio::Pixiv,
                    id: (*id).to_string(),
                    url: format!("https://www.pixiv.net/users/{id}"),
                });
            }
        }
        if let Some(q) = url.split_once('?').map(|(_, q)| q) {
            if let Some(id) = q
                .split('&')
                .filter_map(|p| p.split_once('='))
                .find(|(k, _)| *k == "id")
                .map(|(_, v)| v)
            {
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                    return Some(Perfil {
                        sitio: Sitio::Pixiv,
                        id: id.to_string(),
                        url: format!("https://www.pixiv.net/users/{id}"),
                    });
                }
            }
        }
        return None;
    }

    // Todo lo demás: sin autor en la URL. Ver la nota de la cabecera.
    None
}

/// URL de listado de una etiqueta en yande.re.
///
/// yande.re y no Gelbooru ni Danbooru: Gelbooru cerró su API a los anónimos y
/// devuelve vacío, y Danbooru está tras Cloudflare y responde 403 con
/// frecuencia. yande.re sirve el JSON sin credenciales y con `source` en cada
/// post, que es exactamente lo que hace falta.
pub fn url_cosecha(tag: &str, pagina: u32) -> String {
    let t: String = tag
        .trim()
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-' | '.' => c.to_string(),
            _ => {
                let mut b = [0u8; 4];
                c.encode_utf8(&mut b)
                    .as_bytes()
                    .iter()
                    .map(|x| format!("%{x:02X}"))
                    .collect()
            }
        })
        .collect();
    format!(
        "https://yande.re/post.json?tags={t}&limit=100&page={}",
        pagina.max(1)
    )
}

/// Extrae lo aprovechable de la respuesta del booru.
///
/// Tolerante a propósito: un post sin `source` no es un error, es un post que
/// nadie atribuyó. Devolver `Err` por eso dejaría la búsqueda entera en nada.
pub fn parse_posts(json: &str) -> Vec<PostBooru> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .map(|p| PostBooru {
            source: p.get("source").and_then(Value::as_str).unwrap_or("").to_string(),
            preview: p
                .get("preview_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
        .collect()
}

/// Cuántas miniaturas de muestra se guardan por artista.
const MUESTRAS: usize = 4;

/// Agrupa los posts por perfil de artista y los ordena por relevancia.
///
/// El orden es: primero por número de posts del personaje, y a igualdad por el
/// identificador. **Lo segundo importa**: sin un desempate estable, dos
/// búsquedas idénticas devolverían la lista en distinto orden y parecería que
/// la aplicación va a su aire.
pub fn agrupar(posts: &[PostBooru]) -> Vec<Artista> {
    let mut por_artista: std::collections::HashMap<String, Artista> =
        std::collections::HashMap::new();

    for p in posts {
        let Some(perfil) = perfil_de_fuente(&p.source) else {
            continue;
        };
        // La clave es el IDENTIFICADOR, no la URL. Los artistas reutilizan el
        // mismo nombre entre sitios —`siino13` en Fanbox y `Siino_13` en X— y
        // agrupar por URL los partía en dos entradas, cada una con la mitad de
        // sus posts. Se normaliza el separador porque unos usan `_` y otros no.
        let clave = clave_de_artista(&perfil.id);
        let e = por_artista
            .entry(clave)
            .or_insert_with(|| Artista::nuevo(perfil.clone()));
        e.posts += 1;
        if !e.perfiles.iter().any(|x| x.url == perfil.url) {
            e.perfiles.push(perfil);
        }
        if e.muestras.len() < MUESTRAS && !p.preview.is_empty() {
            e.muestras.push(p.preview.clone());
        }
    }

    let mut v: Vec<Artista> = por_artista.into_values().collect();
    for a in &mut v {
        // Dentro de un artista, primero lo que se puede abrir hoy.
        a.perfiles
            .sort_by(|x, y| y.sitio.abierto().cmp(&x.sitio.abierto()).then_with(|| x.url.cmp(&y.url)));
    }
    v.sort_by(|a, b| {
        b.posts
            .cmp(&a.posts)
            // Desempate estable: sin él, `HashMap` daría un orden distinto en
            // cada búsqueda y parecería que la aplicación va a su aire.
            .then_with(|| a.principal().id.cmp(&b.principal().id))
    });
    v
}

/// Clave con la que se decide que dos perfiles son la misma persona.
///
/// Solo minúsculas y sin separadores: `Siino_13` y `siino13` son el mismo
/// artista en dos sitios. Es una heurística y puede equivocarse si dos
/// personas distintas eligen el mismo nombre, pero el coste de acertar —ver el
/// X de alguien cuyo Fanbox no puedes abrir— compensa de largo al de fallar.
fn clave_de_artista(id: &str) -> String {
    id.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Las siete formas medidas contra datos reales de yande.re.
    #[test]
    fn reconoce_las_formas_reales_de_cada_sitio() {
        let casos: &[(&str, Sitio, &str, &str)] = &[
            (
                "https://x.com/ponkan_8/status/2075875105201414245",
                Sitio::X,
                "ponkan_8",
                "https://x.com/ponkan_8",
            ),
            (
                "https://twitter.com/simasima0033/status/123",
                Sitio::X,
                "simasima0033",
                "https://x.com/simasima0033",
            ),
            (
                "https://www.patreon.com/real_haku89/posts/166030600",
                Sitio::Patreon,
                "real_haku89",
                "https://www.patreon.com/real_haku89",
            ),
            (
                "https://www.patreon.com/c/MenikaEloise",
                Sitio::Patreon,
                "MenikaEloise",
                "https://www.patreon.com/c/MenikaEloise",
            ),
            (
                "https://www.fanbox.cc/@ateoyh/posts/12367376",
                Sitio::Fanbox,
                "ateoyh",
                "https://www.fanbox.cc/@ateoyh",
            ),
            (
                "https://agm94786.fanbox.cc/posts/12020694",
                Sitio::Fanbox,
                "agm94786",
                "https://www.fanbox.cc/@agm94786",
            ),
            (
                "https://bsky.app/profile/alguien.bsky.social/post/3k",
                Sitio::Bluesky,
                "alguien.bsky.social",
                "https://bsky.app/profile/alguien.bsky.social",
            ),
        ];
        for (fuente, sitio, id, url) in casos {
            let p = perfil_de_fuente(fuente).unwrap_or_else(|| panic!("no reconocida: {fuente}"));
            assert_eq!(p.sitio, *sitio, "{fuente}");
            assert_eq!(p.id, *id, "{fuente}");
            assert_eq!(p.url, *url, "{fuente}");
        }
    }

    /// Las dos formas de Fanbox conviven en los datos reales. Sin unificarlas,
    /// el MISMO artista salía dos veces en la lista con sus posts repartidos —
    /// se veía en `inanakisiki`.
    #[test]
    fn las_dos_formas_de_fanbox_son_un_solo_perfil() {
        let a = perfil_de_fuente("https://www.fanbox.cc/@inanakisiki/posts/1").unwrap();
        let b = perfil_de_fuente("https://inanakisiki.fanbox.cc/posts/2").unwrap();
        assert_eq!(a.url, b.url);
        assert_eq!(a.id, b.id);

        // Y por tanto se agrupan en una sola entrada, no en dos.
        let posts = vec![
            PostBooru { source: "https://www.fanbox.cc/@inanakisiki/posts/1".into(), preview: String::new() },
            PostBooru { source: "https://inanakisiki.fanbox.cc/posts/2".into(), preview: String::new() },
        ];
        let g = agrupar(&posts);
        assert_eq!(g.len(), 1, "un artista, no dos");
        assert_eq!(g[0].posts, 2);
    }

    /// Pixiv es el destino MÁS frecuente y la URL de una OBRA no lleva al
    /// autor. Inventárselo sería peor que reconocer que no está en el dato.
    /// Pero si la fuente cita el PERFIL, ahí sí está y se aprovecha.
    #[test]
    fn pixiv_solo_resuelve_cuando_el_autor_esta_en_la_url() {
        // Obra: el autor no aparece por ningún lado.
        assert!(perfil_de_fuente("https://www.pixiv.net/artworks/148283917").is_none());
        assert!(perfil_de_fuente("https://i.pximg.net/img-original/img/2026/07/23/1.png").is_none());

        // Perfil moderno y perfil antiguo: los dos dan el mismo resultado.
        for u in [
            "https://www.pixiv.net/users/110912955",
            "https://www.pixiv.net/en/users/110912955",
            "https://www.pixiv.net/member.php?id=110912955",
        ] {
            let p = perfil_de_fuente(u).unwrap_or_else(|| panic!("debería resolver: {u}"));
            assert_eq!(p.sitio, Sitio::Pixiv);
            assert_eq!(p.url, "https://www.pixiv.net/users/110912955", "{u}");
        }
        // Y un id que no es un número no cuela.
        assert!(perfil_de_fuente("https://www.pixiv.net/users/abc").is_none());
    }

    /// El host se compara entero. `x.com` está dentro de `netflix.com`, y esa
    /// confusión ya costó un fallo en el enrutado de la v1.7.0.
    #[test]
    fn rechaza_dominios_impostores() {
        for u in [
            "https://x.com.atacante.example/ponkan_8/status/1",
            "https://notx.com/alguien/status/1",
            "https://patreon.com.evil.net/usuario/posts/1",
            "https://malo.example/profile/x/post/1",
        ] {
            assert!(perfil_de_fuente(u).is_none(), "no debería colar: {u}");
        }
    }

    #[test]
    fn descarta_lo_que_no_es_un_perfil() {
        for u in [
            "",
            "no es una url",
            "ftp://x.com/alguien/status/1",
            "https://x.com/",
            "https://x.com/i/status/1",         // `i` es ruta interna de X
            "https://www.fanbox.cc/posts/123",  // sin @usuario
            "https://bsky.app/search?q=x",
        ] {
            assert!(perfil_de_fuente(u).is_none(), "no debería colar: {u:?}");
        }
    }

    #[test]
    fn tolera_puerto_credenciales_y_query() {
        let p = perfil_de_fuente("https://user@x.com:443/ponkan_8/status/1?s=20&t=abc").unwrap();
        assert_eq!(p.url, "https://x.com/ponkan_8");
    }

    /// Forma real de la respuesta de yande.re.
    #[test]
    fn lee_la_respuesta_del_booru_sin_exigir_todos_los_campos() {
        let json = r#"[
          {"id":1,"source":"https://x.com/ponkan_8/status/1","preview_url":"https://a/1.jpg"},
          {"id":2,"source":"","preview_url":"https://a/2.jpg"},
          {"id":3,"preview_url":"https://a/3.jpg"}
        ]"#;
        let v = parse_posts(json);
        assert_eq!(v.len(), 3, "un post sin fuente no es un error, es un post sin atribuir");
        assert_eq!(v[0].source, "https://x.com/ponkan_8/status/1");
        assert!(v[1].source.is_empty());
        assert!(v[2].source.is_empty());

        // Y una respuesta rota no revienta ni pierde la búsqueda entera.
        assert!(parse_posts("no es json").is_empty());
        assert!(parse_posts("{}").is_empty());
    }

    #[test]
    fn agrupa_y_ordena_por_posts_del_personaje() {
        let p = |s: &str, t: &str| PostBooru {
            source: s.into(),
            preview: t.into(),
        };
        let posts = vec![
            p("https://x.com/ponkan_8/status/1", "t1"),
            p("https://x.com/ponkan_8/status/2", "t2"),
            p("https://x.com/ponkan_8/status/3", "t3"),
            p("https://x.com/emuzu100/status/9", "t9"),
            p("https://www.pixiv.net/artworks/1", "tp"), // sin autor: se ignora
            p("", "tz"),                                 // sin fuente: se ignora
        ];
        let a = agrupar(&posts);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].principal().id, "ponkan_8");
        assert_eq!(a[0].posts, 3);
        assert_eq!(a[0].muestras, vec!["t1", "t2", "t3"]);
        assert_eq!(a[1].principal().id, "emuzu100");
        assert_eq!(a[1].posts, 1);
    }

    /// LO QUE DE VERDAD RESUELVE ESTA PESTAÑA. Los artistas reutilizan su
    /// nombre entre sitios, así que `siino13` de Fanbox y `Siino_13` de X son
    /// la misma persona. Agrupados, un Fanbox que no puedes abrir porque no le
    /// pagas a ESE creador te enseña al lado su X, que sí puedes.
    #[test]
    fn un_artista_con_varias_casas_es_una_sola_entrada() {
        let posts = vec![
            PostBooru { source: "https://siino13.fanbox.cc/posts/1".into(), preview: "a".into() },
            PostBooru { source: "https://siino13.fanbox.cc/posts/2".into(), preview: "b".into() },
            PostBooru { source: "https://x.com/Siino_13/status/9".into(), preview: "c".into() },
        ];
        let a = agrupar(&posts);
        assert_eq!(a.len(), 1, "una persona, una entrada");
        assert_eq!(a[0].posts, 3, "sus posts se suman, no se reparten");
        assert_eq!(a[0].perfiles().len(), 2, "sus dos casas");
        // El abierto va primero: es el que puedes usar ahora mismo.
        assert_eq!(a[0].principal().sitio, Sitio::X);
        assert_eq!(a[0].principal().url, "https://x.com/Siino_13");
        assert!(a[0].perfiles().iter().any(|p| p.sitio == Sitio::Fanbox));
    }

    #[test]
    fn los_sitios_de_pago_por_creador_se_distinguen() {
        assert!(Sitio::X.abierto());
        assert!(Sitio::Bluesky.abierto());
        assert!(Sitio::Pixiv.abierto());
        // Estos cobran POR CREADOR: estar suscrito a otros diez no sirve.
        assert!(!Sitio::Patreon.abierto());
        assert!(!Sitio::Fanbox.abierto());
    }

    /// Sin desempate estable, dos búsquedas idénticas devolverían la lista en
    /// distinto orden —`HashMap` no lo garantiza— y parecería que la
    /// aplicación va a su aire.
    #[test]
    fn el_orden_es_estable_a_igualdad_de_posts() {
        let posts: Vec<PostBooru> = ["zeta", "alfa", "mu"]
            .iter()
            .map(|u| PostBooru {
                source: format!("https://x.com/{u}/status/1"),
                preview: String::new(),
            })
            .collect();
        let ids: Vec<String> = agrupar(&posts).iter().map(|a| a.principal().id.clone()).collect();
        assert_eq!(ids, vec!["alfa", "mu", "zeta"]);
        // Y repetirlo da lo mismo.
        for _ in 0..5 {
            let otra: Vec<String> =
                agrupar(&posts).iter().map(|a| a.principal().id.clone()).collect();
            assert_eq!(otra, ids);
        }
    }

    #[test]
    fn no_guarda_mas_de_cuatro_muestras() {
        let posts: Vec<PostBooru> = (0..10)
            .map(|i| PostBooru {
                source: "https://x.com/uno/status/1".into(),
                preview: format!("t{i}"),
            })
            .collect();
        let a = agrupar(&posts);
        assert_eq!(a[0].posts, 10, "se cuentan todos");
        assert_eq!(a[0].muestras.len(), MUESTRAS, "pero solo se guardan cuatro");
    }

    #[test]
    fn la_url_de_cosecha_escapa_la_etiqueta() {
        assert_eq!(
            url_cosecha("yukinoshita_yukino", 1),
            "https://yande.re/post.json?tags=yukinoshita_yukino&limit=100&page=1"
        );
        // Los paréntesis de las etiquetas de obra deben ir escapados.
        assert!(url_cosecha("artoria_pendragon_(fate)", 2).contains("%28fate%29"));
        assert!(url_cosecha("x", 0).ends_with("page=1"), "la página nunca es 0");
    }
}
