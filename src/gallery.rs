//! Explorador de galerías: listar antes de descargar.
//!
//! Mismo camino que el navegador de boorus: `gallery-dl -j --no-download`
//! vuelca los metadatos de cada archivo SIN bajar nada, se muestran en una
//! rejilla con miniaturas, y el usuario elige qué quiere.
//!
//! La diferencia con Booru es el modelo de datos. En un booru cada post es un
//! archivo; en Instagram o Weibo una publicación puede tener 10 imágenes y un
//! vídeo, así que hace falta saber a qué publicación pertenece cada archivo y
//! qué posición ocupa dentro de ella.
//!
//! POR QUÉ EL PARSEO ES TAN TOLERANTE: cada extractor de gallery-dl nombra los
//! campos a su manera, y algunos devuelven los números como cadena. Instagram
//! usa `post_shortcode`/`num`/`count`, Weibo usa `pid` y anida el texto en
//! `status`. Exigir un esquema concreto haría que el explorador se quedara en
//! blanco en cuanto un extractor cambie una clave.

use serde_json::Value;

/// Un archivo concreto dentro de una publicación.
#[derive(Clone, Debug, Default)]
pub struct GalleryItem {
    /// URL directa al archivo original (lo que se descarga)
    pub url: String,
    /// Nombre sugerido por el extractor, si lo hay
    pub filename: String,
    pub ext: String,
    pub width: u32,
    pub height: u32,
    /// Tamaño en bytes si el extractor lo conoce (a menudo 0)
    pub filesize: u64,
    pub is_video: bool,
    /// Identificador de la publicación: agrupa los archivos de un carrusel
    pub post_id: String,
    /// Posición dentro de la publicación (1-based) y total de archivos en ella
    pub index_in_post: u32,
    pub count_in_post: u32,
    pub author: String,
    pub description: String,
    pub date: String,
    /// URL de la publicación, para reintentar si el enlace de CDN caduca
    pub post_url: String,
    /// Imagen para la previsualización. En un vídeo NO puede ser el propio
    /// archivo (no se decodifica como imagen), así que si el extractor no da
    /// una portada, se queda vacía y la rejilla muestra un marcador.
    pub thumb_url: String,
    /// Marcado en la rejilla
    pub selected: bool,
}

impl GalleryItem {
    /// ¿Pertenece a un carrusel de varios archivos?
    pub fn is_carousel(&self) -> bool {
        self.count_in_post > 1
    }

    /// Resolución legible, o «—» si el extractor no la aporta.
    pub fn resolution(&self) -> String {
        if self.width > 0 && self.height > 0 {
            format!("{}×{}", self.width, self.height)
        } else {
            "—".into()
        }
    }

    /// Etiqueta de posición dentro de la publicación: «3/10».
    pub fn position(&self) -> String {
        if self.count_in_post > 1 {
            format!("{}/{}", self.index_in_post.max(1), self.count_in_post)
        } else {
            String::new()
        }
    }

    /// Resumen de una línea para la lista.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        if self.is_video {
            s.push_str(if crate::i18n::lang() == crate::i18n::Lang::Es { "VÍDEO  " } else { "VIDEO  " });
        }
        // La resolución solo si se conoce. En un listado de archivos —los
        // pesos de un modelo, por ejemplo— no existe, y un «—» al principio de
        // cada ficha es ruido que empuja lo que sí importa hacia el final.
        if self.width > 0 && self.height > 0 {
            s.push_str(&self.resolution());
        }
        if self.filesize > 0 {
            if !s.is_empty() {
                s.push_str("  ·  ");
            }
            // «3783.0 MB» hay que traducirlo mentalmente; «3.7 GB» no.
            let mb = self.filesize as f64 / 1_048_576.0;
            if mb >= 1024.0 {
                s.push_str(&format!("{:.1} GB", mb / 1024.0));
            } else {
                s.push_str(&format!("{mb:.1} MB"));
            }
        }
        if !self.ext.is_empty() {
            if !s.is_empty() {
                s.push_str("  ·  ");
            }
            s.push_str(&self.ext.to_uppercase());
        }
        let pos = self.position();
        if !pos.is_empty() {
            s.push_str(&format!("  ·  {pos} del post"));
        }
        if !self.date.is_empty() {
            // Solo la parte de fecha: la hora no ayuda a decidir qué bajar
            let d = self.date.split(['T', ' ']).next().unwrap_or(&self.date);
            s.push_str(&format!("  ·  {d}"));
        }
        s
    }
}

// ------------------------- Lectura tolerante de campos -------------------------

/// Entero que puede venir como número o como cadena. Varios extractores de
/// booru y de Weibo devuelven `"1080"` en vez de `1080`.
fn num(meta: &Value, keys: &[&str]) -> u64 {
    for k in keys {
        match meta.get(*k) {
            Some(Value::Number(n)) => {
                if let Some(v) = n.as_u64() {
                    return v;
                }
                if let Some(v) = n.as_f64() {
                    if v >= 0.0 {
                        return v as u64;
                    }
                }
            }
            Some(Value::String(s)) => {
                if let Ok(v) = s.trim().parse::<u64>() {
                    return v;
                }
            }
            _ => {}
        }
    }
    0
}

/// Primera cadena no vacía de entre varias claves posibles.
fn text(meta: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = meta.get(*k).and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                return s.trim().to_string();
            }
        }
        // Weibo anida parte de los metadatos dentro de `status`
        if let Some(inner) = meta.get("status").and_then(|s| s.get(*k)).and_then(|v| v.as_str()) {
            if !inner.trim().is_empty() {
                return inner.trim().to_string();
            }
        }
    }
    String::new()
}

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "webm", "mkv", "m4v", "avi"];

/// Comando de listado: metadatos, sin descargar, paginado.
///
/// `--no-download` es la garantía de que explorar no consume ancho de banda ni
/// escribe nada; `--range` es lo que permite traer de 30 en 30 en vez de
/// esperar a que Instagram entregue un perfil de 2000 publicaciones.
pub fn list_args(url: &str, first: u32, last: u32) -> Vec<String> {
    vec![
        "-j".into(),
        "--no-download".into(),
        "--range".into(),
        format!("{first}-{last}"),
        "--".into(),
        url.to_string(),
    ]
}

/// Resultado de listar: archivos y, en su caso, URLs que hay que seguir.
#[derive(Debug, Default)]
pub struct Listing {
    pub items: Vec<GalleryItem>,
    /// Entradas de tipo 6 («queue»): gallery-dl dice que esa URL se expande en
    /// otro extractor y que hay que volver a preguntarle por ella.
    pub queued: Vec<String>,
}

/// Parsea la salida de `gallery-dl -j`.
///
/// Devuelve `Err` con el mensaje del extractor cuando el propio gallery-dl
/// informa de un error (por ejemplo Instagram sin sesión válida).
pub fn parse_listing(json: &str) -> Result<Listing, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let arr = root.as_array().ok_or(if crate::i18n::lang() == crate::i18n::Lang::Es {
        "respuesta inesperada de gallery-dl"
    } else {
        "unexpected reply from gallery-dl"
    })?;
    let mut out: Vec<GalleryItem> = Vec::new();
    let mut queued: Vec<String> = Vec::new();

    for entry in arr {
        let Some(fields) = entry.as_array() else { continue };
        let kind = fields.first().and_then(|k| k.as_u64()).unwrap_or(0);
        let Some(meta) = fields.last() else { continue };

        // Error explícito del extractor: se propaga tal cual para que el
        // usuario vea «necesitas cookies» en vez de una rejilla vacía.
        if let Some(err) = meta.get("error").and_then(|e| e.as_str()) {
            let msg = meta.get("message").and_then(|m| m.as_str()).unwrap_or(err);
            return Err(msg.to_string());
        }

        // Tipo 6 = «queue»: no es un archivo, es una URL que gallery-dl delega
        // en otro extractor. Instagram lo usa para los perfiles: el extractor
        // `user` devuelve un puntero a `/posts/` y ahí es donde están las fotos.
        // Sin seguir esta pista, un perfil parece vacío aunque tenga 300 posts.
        if kind == 6 {
            if let Some(u) = fields.get(1).and_then(|u| u.as_str()) {
                if u.starts_with("http") {
                    queued.push(u.to_string());
                }
            }
            continue;
        }

        // Tipo 3 = archivo. El tipo 2 es la entrada de directorio y solo trae
        // metadatos del post; contarla produciría duplicados.
        if kind != 3 {
            continue;
        }
        let Some(url) = fields.get(1).and_then(|u| u.as_str()) else { continue };
        if url.is_empty() {
            continue;
        }

        // PORTADA DE UN VÍDEO DE X, no un archivo suelto.
        //
        // Con `extractor.twitter.previews=true`, X emite el póster de cada
        // vídeo como una ENTRADA APARTE, justo detrás del vídeo. Añadirla como
        // un archivo más llenaría la rejilla de JPEG duplicados y «Marcar
        // todo» se los bajaría. Lo que se quiere es lo contrario: que sea la
        // miniatura del vídeo que la precede, que sin ella salía con un
        // triángulo y sin imagen.
        if text(meta, &["type"]) == "preview" {
            if let Some(anterior) = out.last_mut() {
                if anterior.is_video && anterior.thumb_url.is_empty() {
                    anterior.thumb_url = url.to_string();
                }
            }
            continue;
        }

        let ext = text(meta, &["extension"]).to_ascii_lowercase();
        let ext = if ext.is_empty() {
            url.split(['?', '#'])
                .next()
                .and_then(|p| p.rsplit('.').next())
                .filter(|e| (2..=5).contains(&e.len()))
                .unwrap_or("")
                .to_ascii_lowercase()
        } else {
            ext
        };

        let typename = text(meta, &["typename", "type", "media_type"]).to_ascii_lowercase();
        let is_video = VIDEO_EXTS.contains(&ext.as_str())
            || typename.contains("video")
            || meta.get("video_url").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());

        let count = num(meta, &["count", "num_total", "total"]) as u32;
        let index = num(meta, &["num", "index"]) as u32;

        out.push(GalleryItem {
            url: url.to_string(),
            filename: text(meta, &["filename", "name"]),
            ext,
            width: num(meta, &["width", "image_width"]) as u32,
            height: num(meta, &["height", "image_height"]) as u32,
            filesize: num(meta, &["filesize", "size", "file_size"]),
            is_video,
            post_id: text(
                meta,
                &["post_shortcode", "shortcode", "post_id", "pid", "id", "status_id"],
            ),
            index_in_post: index,
            count_in_post: count.max(1),
            author: text(meta, &["username", "owner", "screen_name", "nick", "user"]),
            description: {
                let d = text(meta, &["description", "text", "content", "caption", "title"]);
                d.chars().take(160).collect()
            },
            date: text(meta, &["date", "created_at", "post_date"]),
            post_url: post_url_de(meta),
            thumb_url: {
                // Se prefiere una portada explícita: bajar el original a tamaño
                // completo solo para previsualizar 30 elementos es tirar ancho
                // de banda y velocidad.
                let t = text(
                    meta,
                    &["display_url", "thumbnail", "preview_url", "thumb", "cover", "image"],
                );
                if !t.is_empty() {
                    t
                } else if !is_video {
                    // Para imágenes el propio archivo sirve de vista previa
                    url.to_string()
                } else {
                    String::new()
                }
            },
            selected: false,
        });
    }

    Ok(Listing { items: out, queued })
}

/// URL de la PUBLICACIÓN, que es la red de seguridad cuando el enlace directo
/// del CDN caduca: `download_task` reintenta desde aquí con gallery-dl.
///
/// EL CASO DE FACEBOOK: su extractor no publica `post_url` ni `permalink`, y
/// su clave `url` es el propio enlace de `fbcdn.net`. Con la búsqueda genérica,
/// `post_url` acababa siendo el enlace del CDN, así que el respaldo reintentaba
/// exactamente la URL que acababa de caducar: existía en el código y no servía
/// de nada. Sí publica el `id` de la foto, y la página es
/// `facebook.com/photo/?fbid=<id>`, así que se reconstruye.
///
/// Sin esto, explorar Facebook sería una trampa: la rejilla se llenaría y la
/// mitad de las descargas fallaría media hora después sin salida.
fn post_url_de(meta: &Value) -> String {
    let directa = text(meta, &["post_url", "permalink"]);
    if !directa.is_empty() {
        return directa;
    }
    if text(meta, &["category"]) == "facebook" {
        let id = text(meta, &["id"]);
        if !id.is_empty() {
            return format!("https://www.facebook.com/photo/?fbid={id}");
        }
    }
    // El resto de extractores sí usan `url` como enlace de la publicación.
    text(meta, &["url"])
}

/// ¿Este sitio se puede explorar antes de descargar?
///
/// Se limita a los extractores donde el listado por metadatos está probado y
/// donde el enlace directo se puede bajar después por HTTP con el Referer
/// correcto. No es una lista de «sitios soportados por gallery-dl»: es una
/// lista de sitios donde este flujo concreto funciona.
pub fn is_browsable(host: &str) -> bool {
    const SITES: &[&str] = &[
        "instagram.com",
        "weibo.com",
        "weibo.cn",
        // X: el listado trae `width`, `height` y la URL de `pbs.twimg.com`
        // con `name=orig`, que es el original. Su CDN de medios es PÚBLICO:
        // el enlace se baja después sin sesión, que es justo lo que este
        // flujo necesita. La sesión hace falta para LISTAR, no para bajar.
        "x.com",
        "twitter.com",
        // Bluesky: `cdn.bsky.app` también es público y sin firmar.
        "bsky.app",
        // Facebook es el caso delicado: sus enlaces de `fbcdn.net` van
        // firmados y caducan. Se explora igualmente porque el respaldo por
        // URL de publicación SÍ funciona —ver `post_url_de`—, así que una
        // foto cuyo enlace haya muerto se vuelve a resolver desde su página
        // en vez de quedarse en un error sin salida.
        "facebook.com",
        // Fanbox. Su extractor publica `width` y `height`, y lo único que
        // pide es la cookie `FANBOXSESSID` del navegador —que la aplicación ya
        // sabe entregar—, así que entra en la rejilla como Patreon.
        //
        // PIXIV NO ESTÁ AQUÍ, Y NO ES UN OLVIDO. Su extractor no se apaña con
        // cookies: exige un `refresh-token` de OAuth que se obtiene por un
        // procedimiento aparte y que esta aplicación no gestiona. Sin él
        // responde «'refresh-token' required» y punto. Ofrecer una rejilla que
        // siempre sale vacía sería peor que mandarlo a Descargas, que es lo que
        // se hace hoy.
        "fanbox.cc",
        // Patreon. Se midió antes de decidir, y las tres condiciones se
        // cumplen:
        //
        //  - El listado trae `width` y `height` reales (3584×4800 en la
        //    muestra), así que la rejilla no tiene que medir la miniatura.
        //  - Trae la URL de la publicación en `url`
        //    (`…/Kei_Artworks/posts/sorry-i-was-gone-166592250`), que es lo
        //    que `post_url_de` necesita para resolver de nuevo un enlace
        //    caducado. Es justo lo que a Facebook le faltaba.
        //  - Sus enlaces van firmados, pero `token-time` daba **catorce
        //    días** de margen en la muestra. Explorar con calma y elegir
        //    después cabe de sobra; una firma de una hora lo habría
        //    descartado.
        //
        // El archivo que se lista es `download_url`, el original, no la
        // variante `{"w":620}` del visor. Eso importa: las dimensiones que
        // se enseñan corresponden a lo que se va a bajar y no a una copia
        // reducida.
        "patreon.com",
    ];
    SITES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_exploran_los_sitios_con_cdn_publico() {
        assert!(is_browsable("www.instagram.com"));
        assert!(is_browsable("x.com"));
        assert!(is_browsable("twitter.com"));
        assert!(is_browsable("bsky.app"));
        assert!(is_browsable("www.facebook.com"));
        // Y el impostor de siempre
        assert!(!is_browsable("x.com.atacante.net"));
    }

    /// Sin esto, el respaldo de Facebook reintentaría el enlace de CDN que
    /// acaba de caducar, porque su extractor no publica la URL del post.
    #[test]
    fn facebook_reconstruye_la_url_de_la_publicacion() {
        let meta: Value = serde_json::from_str(
            r#"{"category":"facebook","id":"123456","url":"https://scontent.fbcdn.net/v/foto.jpg?oh=x&oe=y"}"#,
        )
        .unwrap();
        assert_eq!(post_url_de(&meta), "https://www.facebook.com/photo/?fbid=123456");
    }

    #[test]
    fn el_resto_de_sitios_conserva_su_url_de_publicacion() {
        let meta: Value = serde_json::from_str(
            r#"{"category":"instagram","post_url":"https://www.instagram.com/p/ABC/"}"#,
        )
        .unwrap();
        assert_eq!(post_url_de(&meta), "https://www.instagram.com/p/ABC/");
    }

    #[test]
    fn list_args_no_descarga_y_pagina() {
        let a = list_args("https://www.instagram.com/alguien/", 1, 30);
        assert!(a.contains(&"--no-download".to_string()), "explorar no debe descargar");
        assert!(a.contains(&"-j".to_string()));
        assert!(a.contains(&"1-30".to_string()));
        // La URL siempre tras `--`, como el resto de motores
        let sep = a.iter().position(|s| s == "--").unwrap();
        assert_eq!(a.last().unwrap(), "https://www.instagram.com/alguien/");
        assert!(sep < a.len() - 1);
    }

    #[test]
    fn ignora_entradas_de_directorio_y_no_duplica() {
        // gallery-dl emite tipo 2 (directorio) y tipo 3 (archivo) por post
        let j = r#"[
            [2, {"post_id":"A"}],
            [3, "https://cdn/1.jpg", {"post_shortcode":"A","num":1,"count":2,"width":1080,"height":1350}],
            [3, "https://cdn/2.jpg", {"post_shortcode":"A","num":2,"count":2,"width":1080,"height":1350}]
        ]"#;
        let v = parse_listing(j).unwrap().items;
        assert_eq!(v.len(), 2, "solo los archivos, sin la entrada de directorio");
        assert_eq!(v[0].post_id, "A");
        assert!(v[0].is_carousel());
        assert_eq!(v[1].position(), "2/2");
    }

    #[test]
    fn extrae_resolucion_y_detecta_video() {
        let j = r#"[
            [3, "https://cdn/a.jpg", {"width":1440,"height":1800,"extension":"jpg"}],
            [3, "https://cdn/b.mp4", {"width":720,"height":1280,"extension":"mp4"}],
            [3, "https://cdn/c", {"typename":"GraphVideo"}]
        ]"#;
        let v = parse_listing(j).unwrap().items;
        assert_eq!(v[0].resolution(), "1440×1800");
        assert!(!v[0].is_video);
        assert!(v[1].is_video, "por extensión");
        assert!(v[2].is_video, "por typename");
        assert_eq!(v[2].resolution(), "—", "sin datos no se inventa resolución");
    }

    #[test]
    fn la_miniatura_nunca_apunta_a_un_video() {
        let j = r#"[
            [3, "https://cdn/a.jpg", {"extension":"jpg"}],
            [3, "https://cdn/b.mp4", {"extension":"mp4"}],
            [3, "https://cdn/c.mp4", {"extension":"mp4","display_url":"https://cdn/c.jpg"}]
        ]"#;
        let v = parse_listing(j).unwrap().items;
        // Imagen: vale el propio archivo
        assert_eq!(v[0].thumb_url, "https://cdn/a.jpg");
        // Vídeo sin portada: vacío, NO el .mp4 (no se decodifica como imagen)
        assert!(v[1].thumb_url.is_empty(), "no puede previsualizar un mp4");
        // Vídeo con portada: se usa la portada
        assert_eq!(v[2].thumb_url, "https://cdn/c.jpg");
    }

    #[test]
    fn tolera_numeros_como_cadena() {
        // Varios extractores devuelven "1080" en vez de 1080
        let j = r#"[[3,"https://cdn/x.jpg",{"width":"1080","height":"1920","filesize":"2048"}]]"#;
        let v = parse_listing(j).unwrap().items;
        assert_eq!(v[0].width, 1080);
        assert_eq!(v[0].height, 1920);
        assert_eq!(v[0].filesize, 2048);
    }

    #[test]
    fn lee_campos_anidados_de_weibo() {
        // Weibo mete parte de los metadatos dentro de `status`
        let j = r#"[[3,"https://wx.sinaimg/x.jpg",{"pid":"999","status":{"text":"hola mundo"}}]]"#;
        let v = parse_listing(j).unwrap().items;
        assert_eq!(v[0].post_id, "999");
        assert_eq!(v[0].description, "hola mundo");
    }

    /// REGRESIÓN: el perfil de Instagram que parecía vacío.
    ///
    /// El extractor `user` no devuelve archivos: devuelve una entrada de tipo 6
    /// apuntando a `/posts/`. Ignorarla hacía que un perfil con cientos de
    /// publicaciones se listara como cero elementos, sin error ni aviso.
    #[test]
    fn una_entrada_de_cola_se_recoge_para_seguirla() {
        let j = r#"[[6,"https://www.instagram.com/vega_teu/posts/",
                     {"category":"instagram","subcategory":"user"}]]"#;
        let l = parse_listing(j).unwrap();
        assert!(l.items.is_empty(), "una cola no es un archivo");
        assert_eq!(l.queued, vec!["https://www.instagram.com/vega_teu/posts/"]);
    }

    #[test]
    fn una_cola_sin_url_valida_se_descarta() {
        let j = r#"[[6,"no-es-url",{}],[6,null,{}]]"#;
        assert!(parse_listing(j).unwrap().queued.is_empty());
    }

    #[test]
    fn propaga_el_error_del_extractor() {
        let j = r#"[[3,"",{"error":"AuthRequired","message":"Instagram necesita sesión"}]]"#;
        assert_eq!(parse_listing(j).unwrap_err(), "Instagram necesita sesión");
    }

    #[test]
    fn respuesta_vacia_o_rota_no_revienta() {
        assert_eq!(parse_listing("[]").unwrap().items.len(), 0);
        assert!(parse_listing("no es json").is_err());
        assert!(parse_listing("{}").is_err());
        // Entradas incompletas se saltan en vez de tumbar el listado
        assert_eq!(parse_listing(r#"[[3],[3,""],[9,"x",{}]]"#).unwrap().items.len(), 0);
    }

    /// Patreon entró en la rejilla DESPUÉS de medir, no antes: sus enlaces
    /// van firmados y explorar solo tiene sentido si sobreviven a la espera.
    /// La muestra dio catorce días de margen y una URL de publicación con la
    /// que rescatar un enlace caducado — que es lo que a Facebook le faltaba.
    #[test]
    fn patreon_es_explorable_y_conserva_la_url_del_post() {
        assert!(is_browsable("patreon.com"));
        assert!(is_browsable("www.patreon.com"));
        assert!(!is_browsable("patreon.com.atacante.example"));

        // El campo `url` del post es el permalink, y es el que rescata un
        // enlace de CDN muerto.
        let meta: Value = serde_json::from_str(
            r#"{"id":166592250,"url":"https://www.patreon.com/Kei_Artworks/posts/sorry-i-was-gone-166592250"}"#,
        )
        .unwrap();
        assert_eq!(
            post_url_de(&meta),
            "https://www.patreon.com/Kei_Artworks/posts/sorry-i-was-gone-166592250"
        );
    }
    /// X emite el póster de un vídeo como una entrada APARTE, detrás del
    /// vídeo. Debe convertirse en su miniatura, no en un archivo más: si no,
    /// la rejilla se llena de JPEG duplicados y «Marcar todo» los baja.
    #[test]
    fn la_portada_de_un_video_de_x_no_es_un_archivo_mas() {
        let j = r#"[
          [3,"https://video.twimg.com/a.mp4",
            {"extension":"mp4","type":"video","width":1280,"height":720,"num":1}],
          [3,"https://pbs.twimg.com/media/ABC?format=jpg&name=orig",
            {"extension":"jpg","type":"preview","width":1280,"height":720,"num":2}]
        ]"#;
        let l = parse_listing(j).unwrap();
        assert_eq!(l.items.len(), 1, "el póster no es un elemento propio");
        assert!(l.items[0].is_video);
        assert_eq!(l.items[0].url, "https://video.twimg.com/a.mp4");
        assert!(
            l.items[0].thumb_url.contains("pbs.twimg.com"),
            "el póster pasa a ser su miniatura: {}",
            l.items[0].thumb_url
        );
    }
    #[test]
    fn solo_los_sitios_probados_son_explorables() {
        assert!(is_browsable("instagram.com"));
        assert!(is_browsable("www.instagram.com"));
        assert!(is_browsable("weibo.com"));
        assert!(is_browsable("m.weibo.cn"));
        // Ni boorus (tienen su propia pestaña) ni dominios impostores
        assert!(!is_browsable("danbooru.donmai.us"));
        assert!(!is_browsable("instagram.com.atacante.example"));
    }


    /// El resumen de una ficha no puede inventarse lo que no sabe.
    ///
    /// En un listado de archivos —los pesos de un modelo de Hugging Face— no
    /// hay resolución, y un «—» al principio de cada una de treinta y dos
    /// fichas es ruido puro. Y «3783.0 MB» obliga a dividir mentalmente por
    /// 1024 para saber si eso cabe en el disco.
    #[test]
    fn el_resumen_no_inventa_resolucion_y_se_lee_de_un_vistazo() {
        let peso = GalleryItem {
            filename: "model-00006-of-00018.safetensors".into(),
            ext: "safetensors".into(),
            filesize: 3_957_109_648,
            ..Default::default()
        };
        let s = peso.summary();
        assert!(!s.contains('—'), "no debería haber resolución: {s}");
        assert!(s.starts_with("3.7 GB"), "{s}");
        assert!(s.contains("SAFETENSORS"), "{s}");

        // Por debajo de un giga se sigue leyendo en megas.
        let chico = GalleryItem { filesize: 3_355_443, ext: "txt".into(), ..Default::default() };
        assert!(chico.summary().starts_with("3.2 MB"), "{}", chico.summary());

        // Y donde SÍ se conoce la resolución, sigue saliendo la primera.
        let foto = GalleryItem {
            width: 1440,
            height: 1800,
            filesize: 2_097_152,
            ext: "jpg".into(),
            ..Default::default()
        };
        assert_eq!(foto.summary(), "1440×1800  ·  2.0 MB  ·  JPG");
    }

    #[test]
    fn el_resumen_no_miente_cuando_faltan_datos() {
        let vacio = GalleryItem::default();
        let s = vacio.summary();
        // Este test pedía antes un «—» donde no hubiera resolución. Se quitó al
        // empezar a listar archivos de un modelo: ahí la resolución no es que
        // se desconozca, es que NO EXISTE, y un guion en cada una de treinta y
        // dos fichas es ruido que empuja hacia el final lo único que importa.
        //
        // El principio se mantiene igual: omitir un dato que no se tiene no es
        // mentir; inventárselo sí.
        assert!(s.is_empty(), "sin nada que decir, no se dice nada: {s}");
        assert!(!s.contains("MB") && !s.contains("GB"), "sin tamaño no se inventa: {s}");
        // `resolution()` sí sigue devolviendo «—»: ahí se pregunta
        // explícitamente por la resolución, y callar sería peor que un guion.
        assert_eq!(vacio.resolution(), "—");
        assert_eq!(vacio.position(), "", "sin carrusel no hay posición");
    }
}
