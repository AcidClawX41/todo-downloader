//! Buscador de boorus (Danbooru, Gelbooru, e621…) — By Eric V. Gramunt
//!
//! No reimplementa las APIs: delega en **gallery-dl** en modo `-j` (volcado de
//! metadatos SIN descargar). Ese modo devuelve el JSON completo del post, del
//! que se extraen la URL original, la miniatura, las dimensiones y las etiquetas.
//!
//! Motivo del enfoque: Danbooru, Gelbooru y Moebooru tienen APIs **distintas
//! entre sí** y cambian con el tiempo. gallery-dl ya mantiene un extractor por
//! sitio y se actualiza solo; duplicar ese trabajo en Rust sería mantenimiento
//! perpetuo para no ganar nada. Las descargas sí las hace el motor HTTP nativo,
//! que da reanudación y calidad original.
//!
//! El parseo es **tolerante a propósito**: cada booru nombra los campos a su
//! manera (`image_width` vs `width`, `preview_file_url` vs `preview_url`, y e621
//! los anida bajo `file`/`preview`). Se prueban todas las variantes conocidas.

use serde_json::Value;

/// Un sitio soportado
pub struct Site {
    /// Nombre mostrado
    pub name: &'static str,
    /// Clave del extractor en gallery-dl (para pasarle credenciales con -o)
    pub key: &'static str,
    /// Plantilla de búsqueda; {tags} se sustituye por las etiquetas
    pub search: &'static str,
    /// Si la API exige credenciales sí o sí
    pub needs_auth: bool,
}

pub const SITES: &[Site] = &[
    Site {
        name: "Danbooru",
        key: "danbooru",
        search: "https://danbooru.donmai.us/posts?tags={tags}",
        needs_auth: false,
    },
    Site {
        name: "Safebooru",
        key: "safebooru",
        search: "https://safebooru.org/index.php?page=post&s=list&tags={tags}",
        needs_auth: false,
    },
    Site {
        name: "AIBooru",
        key: "aibooru",
        search: "https://aibooru.online/posts?tags={tags}",
        needs_auth: false,
    },
    Site {
        name: "yande.re",
        key: "yandere",
        search: "https://yande.re/post?tags={tags}",
        needs_auth: false,
    },
    Site {
        name: "Konachan",
        key: "konachan",
        search: "https://konachan.com/post?tags={tags}",
        needs_auth: false,
    },
    Site {
        name: "e621",
        key: "e621",
        search: "https://e621.net/posts?tags={tags}",
        needs_auth: false,
    },
    // Gelbooru cerró su API a los anónimos: sin api-key + user-id responde
    // {"error":"AuthRequired"}. Se marca para avisar antes de buscar.
    Site {
        name: "Gelbooru",
        key: "gelbooru",
        search: "https://gelbooru.com/index.php?page=post&s=list&tags={tags}",
        needs_auth: true,
    },
];

/// Etiquetas de ejemplo para el desplegable de la interfaz.
///
/// Sirven de doble propósito: dar algo con lo que empezar, y **enseñar la
/// convención de nombres** de los boorus, que no es evidente — minúsculas,
/// guion bajo por espacio, y la obra entre paréntesis cuando el nombre se
/// repite entre series (`toki_(blue_archive)`).
///
/// Todas verificadas contra Danbooru: devuelven resultados.
pub const SAMPLE_TAGS: &[(&str, &str)] = &[
    // Todas COMPROBADAS contra la API de Gelbooru antes de entrar aquí, con su
    // número de posts. Un ejemplo que no devuelve nada es peor que no ponerlo:
    // la primera búsqueda falla y parece que la función no sirve.
    ("Toki — Blue Archive", "toki_(blue_archive)"),
    ("Artoria Pendragon — Fate", "artoria_pendragon_(fate)"),
    ("Rin Tohsaka", "tohsaka_rin"),
    ("Yukino Yukinoshita", "yukinoshita_yukino"),
    ("Hatsune Miku", "hatsune_miku"),
    ("Marin Kitagawa", "kitagawa_marin"),
    ("Yor Forger — Spy x Family", "yor_briar"),
    ("Makima — Chainsaw Man", "makima_(chainsaw_man)"),
    ("Power — Chainsaw Man", "power_(chainsaw_man)"),
    ("Nezuko — Demon Slayer", "kamado_nezuko"),
    ("Frieren", "frieren"),
    ("Rem — Re:Zero", "rem_(re:zero)"),
    ("Zero Two — Darling in the Franxx", "zero_two_(darling_in_the_franxx)"),
    ("Mikasa Ackerman", "mikasa_ackerman"),
    // Las dos Asukas: la del anime original y la del Rebuild. Son etiquetas
    // DISTINTAS y con muy distinto volumen (28.097 contra 132), así que
    // ponerlas por separado es lo honesto.
    ("Asuka Langley Soryu — Evangelion", "souryuu_asuka_langley"),
    ("Asuka Langley Shikinami — Rebuild", "shikinami_asuka_langley"),
    ("Akiha Tohno — Tsukihime", "tohno_akiha"),
    // Las dos Bismarcks. Mismo barco de origen, dos juegos y dos etiquetas sin
    // relación: buscar una NO devuelve nada de la otra.
    ("Bismarck — KanColle", "bismarck_(kancolle)"),
    ("Bismarck — Azur Lane", "bismarck_(azur_lane)"),
    ("Nami — One Piece", "nami_(one_piece)"),
    ("Ganyu — Genshin", "ganyu_(genshin_impact)"),
    ("Raiden Shogun — Genshin", "raiden_shogun"),
    ("Tifa Lockhart — FF VII", "tifa_lockhart"),
    ("Aerith Gainsborough — FF VII", "aerith_gainsborough"),
    ("2B — NieR: Automata", "2b_(nier:automata)"),
    ("Samus Aran — Metroid", "samus_aran"),
    ("Princess Zelda", "princess_zelda"),
    ("Chun-Li — Street Fighter", "chun-li"),
    ("D.Va — Overwatch", "d.va_(overwatch)"),
    ("Mercy — Overwatch", "mercy_(overwatch)"),
    ("Harley Quinn", "harley_quinn"),
    ("Lara Croft — Tomb Raider", "lara_croft"),
    ("Ciri — The Witcher", "ciri"),
    // Personajes masculinos
    ("Monkey D. Luffy", "monkey_d._luffy"),
    ("Roronoa Zoro", "roronoa_zoro"),
    ("Denji — Chainsaw Man", "denji_(chainsaw_man)"),
    ("Gojo Satoru — Jujutsu Kaisen", "gojo_satoru"),
    ("Levi Ackerman", "levi_(shingeki_no_kyojin)"),
    ("Eren Yeager", "eren_yeager"),
    ("Killua Zoldyck — Hunter x Hunter", "killua_zoldyck"),
    ("Edward Elric — FMA", "edward_elric"),
    ("Jotaro Kujo — JoJo", "kujo_jotaro"),
    ("Joseph Joestar — JoJo", "joseph_joestar"),
    ("Dio Brando — JoJo", "dio_brando"),
    ("Spike Spiegel — Cowboy Bebop", "spike_spiegel"),
    ("Guts — Berserk", "guts_(berserk)"),
    ("Keroro", "keroro"),
    // Obras enteras: devuelven arte de todo el reparto
    ("Pokémon", "pokemon"),
    ("Star Wars", "star_wars"),
    ("Marvel", "marvel"),
    ("DC Comics", "dc_comics"),
    ("Undertale", "undertale"),
    ("Breaking Bad", "breaking_bad"),
    ("The Simpsons", "the_simpsons"),
    ("Family Guy", "family_guy"),
];

/// Un post ya normalizado, venga del booru que venga
#[derive(Clone)]
pub struct Post {
    pub id: u64,
    /// Original a máxima calidad (lo que se descarga)
    pub file_url: String,
    /// Miniatura para la rejilla
    pub preview_url: String,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub ext: String,
    /// g/s/q/e (general, sensible, questionable, explicit) según el sitio
    pub rating: String,
    pub artist: String,
    /// Marcado por el usuario en la rejilla
    pub selected: bool,
}

impl Post {
    /// ¿Es una imagen? (los boorus también alojan webm/mp4)
    pub fn is_image(&self) -> bool {
        matches!(self.ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif" | "avif")
    }
}

/// Construye la URL de búsqueda. Las etiquetas van separadas por espacios y
/// se codifican como `+`, que es lo que esperan todos estos sitios.
pub fn search_url(site: &Site, tags: &str) -> String {
    let clean: String = tags
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("+");
    site.search.replace("{tags}", &clean)
}

/// Contenido del archivo de configuración temporal con las credenciales.
///
/// **Endurecimiento de seguridad:** antes se pasaban con `-o clave=valor`, y eso
/// las dejaba visibles en la línea de comandos del proceso — cualquier programa
/// del mismo usuario podía leerlas con `wmic process get commandline`, la
/// columna «Línea de comandos» del Administrador de tareas o `ps aux`.
///
/// Ahora van en un archivo que se pasa con `-c`, que gallery-dl trata como
/// configuración **adicional** (no reemplaza la del usuario, se fusiona).
/// El archivo se crea justo antes de buscar y se borra al terminar, así que
/// ni aparece en los argumentos ni queda en disco de forma permanente.
///
/// Devuelve `None` si no hay credenciales que escribir.
/// Configuración de credenciales para gallery-dl, si el sitio las exige.
///
/// SOLO PARA LOS SITIOS QUE LAS EXIGEN, y esto no es una precaución teórica.
/// Los ajustes guardan UN par usuario/clave, no uno por sitio. Antes se le
/// enchufaba a cualquier booru que se buscara, de modo que el `user-id`
/// numérico y la `api-key` de **Gelbooru** —el único que obliga— acababan
/// mandados a Danbooru como `username` y `api-key`.
///
/// El efecto era desconcertante: Danbooru, AIBooru, e621 y Konachan leen esos
/// campos y se encontraban con credenciales que no son suyas, mientras que
/// Safebooru y yande.re ni los miran y por eso nunca fallaron. Cuadra con el
/// síntoma exacto —cuatro sitios agotando el plazo y dos funcionando— y explica
/// por qué descargar la galería completa del MISMO sitio sí funcionaba: ese
/// camino nunca ha pasado un `-c`.
///
/// Lo correcto de verdad sería un par de credenciales POR SITIO. Mientras no
/// exista, no mandarlas donde no constan necesarias es la misma política que
/// ya rige para las cookies desde que enviarlas de más rompió YouTube.
pub fn auth_config(site: &Site, user: &str, key: &str) -> Option<String> {
    if !site.needs_auth {
        return None;
    }
    let (user, key) = (user.trim(), key.trim());
    if user.is_empty() || key.is_empty() {
        return None;
    }
    // Gelbooru usa user-id/api-key; el resto username/api-key
    let u_field = if site.key == "gelbooru" { "user-id" } else { "username" };

    // Se construye con serde_json para que las comillas y los caracteres
    // especiales de la clave se escapen solos.
    let cfg = serde_json::json!({
        "extractor": {
            site.key: {
                u_field: user,
                "api-key": key,
            }
        }
    });
    Some(cfg.to_string())
}

// ---------------- Parseo tolerante ----------------

fn s(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(x) = v.get(*k).and_then(|x| x.as_str()) {
            if !x.is_empty() {
                return x.to_string();
            }
        }
    }
    String::new()
}

/// Número tolerante: algunos boorus (Safebooru y compañía) devuelven los
/// enteros **como cadenas** (`"1494"`), así que no basta con `as_u64()`.
fn n(v: &Value, keys: &[&str]) -> u64 {
    for k in keys {
        match v.get(*k) {
            Some(Value::Number(num)) => {
                if let Some(x) = num.as_u64() {
                    return x;
                }
            }
            Some(Value::String(s)) => {
                if let Ok(x) = s.trim().parse::<u64>() {
                    return x;
                }
            }
            _ => {}
        }
    }
    0
}

/// Extrae los posts del volcado JSON de `gallery-dl -j`.
///
/// El formato es un array de entradas `[tipo, …, metadatos]`; nos quedamos con
/// las que traen un objeto con `file_url` (o `file.url` en e621).
pub fn parse(json: &str) -> Result<Vec<Post>, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let arr = root.as_array().ok_or(if crate::i18n::lang() == crate::i18n::Lang::Es {
        "respuesta inesperada"
    } else {
        "unexpected reply"
    })?;
    let mut out = Vec::new();

    for entry in arr {
        let Some(items) = entry.as_array() else { continue };
        // gallery-dl emite dos entradas por post: tipo 2 (directorio, solo
        // metadatos) y tipo 3 (archivo). Sin filtrar salían DUPLICADOS.
        let kind = items.first().and_then(|k| k.as_u64()).unwrap_or(0);
        let is_error = items
            .last()
            .and_then(|m| m.as_object())
            .map(|m| m.contains_key("error"))
            .unwrap_or(false);
        if kind != 3 && !is_error {
            continue;
        }
        let Some(meta) = items.last().and_then(|m| m.as_object()) else { continue };
        let meta = Value::Object(meta.clone());

        // Error explícito del extractor (p. ej. Gelbooru sin credenciales)
        if let Some(err) = meta.get("error").and_then(|e| e.as_str()) {
            let msg = meta
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(err)
                .to_string();
            return Err(msg);
        }

        // e621 anida bajo file/preview; el resto va plano
        let file_obj = meta.get("file");
        let file_url = if let Some(f) = file_obj.and_then(|f| f.get("url")).and_then(|u| u.as_str()) {
            f.to_string()
        } else {
            s(&meta, &["file_url", "large_file_url"])
        };
        if file_url.is_empty() {
            continue;
        }

        let preview_url = if let Some(p) =
            meta.get("preview").and_then(|p| p.get("url")).and_then(|u| u.as_str())
        {
            p.to_string()
        } else {
            let direct = s(&meta, &["preview_file_url", "preview_url", "sample_url", "large_file_url"]);
            if direct.is_empty() { file_url.clone() } else { direct }
        };

        let (mut w, mut h) = (
            n(&meta, &["image_width", "width"]) as u32,
            n(&meta, &["image_height", "height"]) as u32,
        );
        let mut size = n(&meta, &["file_size", "size"]);
        if let Some(f) = file_obj {
            if w == 0 {
                w = n(f, &["width"]) as u32;
            }
            if h == 0 {
                h = n(f, &["height"]) as u32;
            }
            if size == 0 {
                size = n(f, &["size"]);
            }
        }

        let ext = {
            let e = s(&meta, &["extension", "file_ext"]);
            if !e.is_empty() {
                e
            } else {
                file_url
                    .split(['?', '#'])
                    .next()
                    .and_then(|p| p.rsplit('.').next())
                    .unwrap_or("jpg")
                    .to_string()
            }
        };

        out.push(Post {
            id: n(&meta, &["id"]),
            file_url,
            preview_url,
            width: w,
            height: h,
            file_size: size,
            ext: ext.to_ascii_lowercase(),
            rating: s(&meta, &["rating"]),
            artist: s(&meta, &["tag_string_artist", "artist", "author"]),
            selected: false,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un par de credenciales para TODOS los boorus era el fallo: el user-id
    /// numérico de Gelbooru llegaba a Danbooru como nombre de usuario y dejaba
    /// la búsqueda colgada hasta agotar el plazo. Cuatro sitios caídos y dos
    /// funcionando, según leyeran esos campos o no.
    #[test]
    fn las_credenciales_solo_van_al_sitio_que_las_exige() {
        let gel = SITES.iter().find(|s| s.key == "gelbooru").unwrap();
        let dan = SITES.iter().find(|s| s.key == "danbooru").unwrap();
        let e6 = SITES.iter().find(|s| s.key == "e621").unwrap();

        assert!(auth_config(gel, "12345", "clave").is_some(), "Gelbooru sí las exige");
        assert!(auth_config(dan, "12345", "clave").is_none(), "Danbooru no debe recibirlas");
        assert!(auth_config(e6, "12345", "clave").is_none(), "e621 no debe recibirlas");

        // Sin credenciales no se escribe nada, ni siquiera para Gelbooru.
        assert!(auth_config(gel, "", "").is_none());
        assert!(auth_config(gel, "12345", "  ").is_none());
    }
}
