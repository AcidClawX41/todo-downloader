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
    /// Página donde el usuario genera su clave de API, si el sitio la ofrece.
    ///
    /// Es lo que distingue a los sitios con los que se puede hablar de frente
    /// de los que no. Danbooru pide por escrito que los clientes
    /// se identifiquen y **no imiten navegadores**, y a cambio da una clave
    /// que levanta los topes del anónimo. AIBooru no trae ese aviso en su
    /// fork, pero sí la clave, y sus lecturas por API no tienen límite.
    ///
    /// Guardar la URL aquí y no en la interfaz es lo que permite que el aviso
    /// de «te falta la clave» lleve al sitio correcto sin una tabla aparte
    /// que se quede vieja — el mismo motivo por el que `dominio()` sale de
    /// `search`.
    pub api_key_url: Option<&'static str>,
    /// ¿Este sitio PIDE que los clientes se identifiquen en vez de imitar un
    /// navegador?
    ///
    /// Cierto para la familia Danbooru y la de e621.
    ///
    /// ESTE CAMPO HA CAMBIADO DOS VECES. Merece contarse, porque las dos
    /// veces se decidió por una deducción y no por una medida:
    ///
    /// 1. Se puso a `true` para los tres porque Danbooru lo pide por escrito.
    /// 2. Se bajó a `false` en AIBooru al ver que se colgaba, deduciendo que
    ///    su fork —que no trae ese aviso— prefería un User-Agent de navegador.
    /// 3. Se vuelve a `true`: la versión de Linux funciona **sin cookies y sin
    ///    cuenta**, y lo que la de Windows hacía de más era justamente mandar
    ///    cookies. El User-Agent nunca fue el problema; el paso 2 confundió
    ///    dos cosas que van juntas en `sesion_para_booru`.
    ///
    /// Lo que sí está medido: sin cookies funciona. Y la ayuda de la API de
    /// Danbooru dice que los User-Agent de navegador están bloqueados en su
    /// API, así que identificarse es además lo correcto.
    pub ua_propio: bool,
}

impl Site {
    /// El dominio del sitio, sacado de su propia plantilla de búsqueda.
    ///
    /// HACE FALTA porque el aviso de Cloudflare decía a quién visitar para
    /// conseguir la `cf_clearance`… y lo decía FIJO: «danbooru.donmai.us».
    /// Buscando en AIBooru, el mensaje mandaba al usuario al sitio que NO
    /// estaba fallando, a por una cookie que no le sirve — la `cf_clearance`
    /// va atada a un dominio concreto. Un consejo equivocado gasta más tiempo
    /// que no dar ninguno.
    pub fn dominio(&self) -> &str {
        self.search
            .split_once("://")
            .map(|(_, r)| r)
            .unwrap_or(self.search)
            .split('/')
            .next()
            .unwrap_or(self.search)
    }

    /// ¿Hay que presentarse ante este sitio en vez de imitar un navegador?
    ///
    /// Cierto solo donde el sitio lo pide por escrito. Para ellos, mandar un
    /// User-Agent de Chrome sobre la huella TLS de Python no es un disfraz
    /// convincente: es una contradicción, y puntúa peor que un cliente que
    /// dice su nombre.
    ///
    /// Fuente: `danbooru.donmai.us/wiki_pages/help:api`.
    pub fn se_identifica(&self) -> bool {
        self.ua_propio
    }

    /// ¿Admite clave de API?
    ///
    /// Independiente de lo anterior: AIBooru la admite y aun así quiere que se
    /// le hable como a un navegador.
    pub fn admite_clave(&self) -> bool {
        self.api_key_url.is_some()
    }
}

pub const SITES: &[Site] = &[
    Site {
        name: "Danbooru",
        key: "danbooru",
        search: "https://danbooru.donmai.us/posts?tags={tags}",
        api_key_url: Some("https://danbooru.donmai.us/profile"),
        ua_propio: true,
        needs_auth: false,
    },
    Site {
        name: "Safebooru",
        key: "safebooru",
        search: "https://safebooru.org/index.php?page=post&s=list&tags={tags}",
        api_key_url: None,
        ua_propio: false,
        needs_auth: false,
    },
    Site {
        name: "AIBooru",
        key: "aibooru",
        search: "https://aibooru.online/posts?tags={tags}",
        api_key_url: Some("https://aibooru.online/profile"),
        ua_propio: true,
        needs_auth: false,
    },
    Site {
        name: "yande.re",
        key: "yandere",
        search: "https://yande.re/post?tags={tags}",
        api_key_url: None,
        ua_propio: false,
        needs_auth: false,
    },
    Site {
        name: "Konachan",
        key: "konachan",
        search: "https://konachan.com/post?tags={tags}",
        api_key_url: None,
        ua_propio: false,
        needs_auth: false,
    },
    Site {
        name: "e621",
        key: "e621",
        search: "https://e621.net/posts?tags={tags}",
        api_key_url: Some("https://e621.net/users/home"),
        ua_propio: true,
        needs_auth: false,
    },
    // Gelbooru cerró su API a los anónimos: sin api-key + user-id responde
    // {"error":"AuthRequired"}. Se marca para avisar antes de buscar.
    Site {
        name: "Gelbooru",
        key: "gelbooru",
        search: "https://gelbooru.com/index.php?page=post&s=list&tags={tags}",
        api_key_url: None,
        ua_propio: false,
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
    // Un ejemplo que no devuelve nada es peor que no ponerlo: la primera
    // búsqueda falla y parece que la función no sirve.
    //
    // Las de hasta «Ciri» se comprobaron contra la API de Gelbooru, con su
    // número de posts. Las de Kaguya-sama, Shingeki no Kyojin y Haruhi son
    // POSTERIORES y NO se han podido comprobar: Danbooru dejó de responder a
    // las consultas del wiki mientras se añadían. Siguen la convención de
    // nombres del sitio —apellido primero, sin honoríficos— pero hasta que
    // alguien las busque, son una apuesta razonada y no un dato.
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
    // Kanna: la etiqueta canónica de Danbooru es `kanna_(blue_archive)`.
    // `ogata_kanna_(blue_archive)` existe, pero es un ALIAS que apunta a
    // aquella. Poner el alias funcionaría hoy y dejaría de funcionar el día
    // que lo retiren, así que va la canónica.
    ("Kanna — Blue Archive", "kanna_(blue_archive)"),
    // Carrera: comprobada, con pocos posts en Danbooru en agosto de 2026 —del
    // orden de nueve— porque su anime es reciente. La cuenta va a SUBIR, no a
    // bajar: es un ejemplo joven, no uno que se apague. Se anota la fecha para
    // que quien lea esto dentro de un año sepa que el número está viejo y no
    // se le ocurra quitarla por escasa.
    ("Carrera — Tensura", "carrera_(tensei_shitara_slime_datta_ken)"),

    // Akame: el signo de admiración forma parte de la etiqueta, porque forma
    // parte del título de la obra («Akame ga Kill!»). Danbooru los conserva.
    ("Akame — Akame ga Kill!", "akame_(akame_ga_kill!)"),

    // --- Roshidere ---
    //
    // DOS ÓRDENES DISTINTOS, Y NO ES UN CAPRICHO DE DANBOORU.
    //
    // Los nombres JAPONESES van apellido primero: `suou_yuki`, `hayasaka_ai`,
    // `suzumiya_haruhi`. Los OCCIDENTALES van al revés, como se escriben:
    // `mikasa_ackerman`, `historia_reiss`, `alisa_mikhailovna_kujou`.
    //
    // Alya lleva las dos cosas —nombre ruso y apellido japonés— y Danbooru la
    // trata como occidental. Aquí se puso `kujou_alisa_mikhailovna`, por
    // costumbre, y devolvía CERO resultados en los tres boorus.
    ("Alya — Roshidere", "alisa_mikhailovna_kujou"),
    // Comprobada: 40 resultados en Danbooru y en Gelbooru.
    ("Yuki Suou — Roshidere", "suou_yuki"),

    // --- Kaguya-sama ---
    ("Kaguya Shinomiya", "shinomiya_kaguya"),
    ("Ai Hayasaka", "hayasaka_ai"),
    ("Chika Fujiwara", "fujiwara_chika"),
    // --- Shingeki no Kyojin --- (Mikasa ya está más arriba)
    ("Historia Reiss", "historia_reiss"),
    ("Annie Leonhart", "annie_leonhart"),
    ("Sasha Blouse", "sasha_blouse"),
    ("Hange Zoe", "hange_zoe"),
    // --- Haruhi Suzumiya ---
    ("Haruhi Suzumiya", "suzumiya_haruhi"),
    ("Yuki Nagato", "nagato_yuki"),
    ("Mikuru Asahina", "asahina_mikuru"),

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
/// Configuración de gallery-dl con las credenciales de UN sitio.
///
/// VA A UN ARCHIVO, NUNCA A LA LÍNEA DE COMANDOS. Los argumentos de un proceso
/// los ve cualquiera con el administrador de tareas abierto, y los dos sitios
/// avisan de lo mismo sobre la clave: «trátala como una contraseña».
///
/// Sirve para dos casos distintos:
///
/// - `needs_auth`: sin credenciales el sitio no contesta (Gelbooru).
/// - `api_key_url`: el sitio funciona sin ellas, pero con ellas funciona
///   MEJOR — se levantan los topes de paginación y de etiquetas del anónimo, y
///   se deja de depender de una cookie que caduca en media hora.
///
/// Fuente: `danbooru.donmai.us/wiki_pages/help:api`.
pub fn auth_config(site: &Site, user: &str, key: &str) -> Option<String> {
    if !site.needs_auth && !site.admite_clave() {
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

    /// El aviso de Cloudflare nombraba «danbooru.donmai.us» pasara lo que
    /// pasara. Buscando en AIBooru mandaba a por una cookie de otro dominio,
    /// que no vale: la `cf_clearance` va atada al sitio que la emitió.
    #[test]
    fn cada_sitio_sabe_su_dominio() {
        let d: Vec<&str> = SITES.iter().map(|s| s.dominio()).collect();
        assert!(d.contains(&"danbooru.donmai.us"), "{d:?}");
        assert!(d.contains(&"aibooru.online"), "{d:?}");
        // Ninguno se queda con la ruta ni con el esquema pegados.
        for s in SITES {
            let dom = s.dominio();
            assert!(!dom.contains('/'), "{}: {dom}", s.name);
            assert!(!dom.contains(':'), "{}: {dom}", s.name);
            assert!(!dom.is_empty(), "{}", s.name);
        }
    }

    /// Un par de credenciales para TODOS los boorus era el fallo: el user-id
    /// numérico de Gelbooru llegaba a Danbooru como nombre de usuario y dejaba
    /// la búsqueda colgada hasta agotar el plazo. Cuatro sitios caídos y dos
    /// funcionando, según leyeran esos campos o no.
    ///
    /// ESTE TEST CAMBIÓ DE MECANISMO, NO DE INTENCIÓN.
    ///
    /// Antes lo garantizaba negando: `auth_config` devolvía `None` para todo
    /// el que no fuera Gelbooru, así que las credenciales del uno no podían
    /// llegar al otro porque no llegaban a nadie más. Eso dejó de valer cuando
    /// Danbooru, AIBooru y e621 pasaron a tener cada uno su clave de API: hoy
    /// negárselas rompería justo lo que vienen a arreglar.
    ///
    /// Ahora se garantiza en la raíz: hay un par POR SITIO, indexado por su
    /// clave de extractor, y cada configuración solo puede nombrar al suyo.
    /// Lo que se comprueba aquí es eso — que ninguna credencial aparece bajo
    /// un extractor que no sea el de su dueño.
    #[test]
    fn las_credenciales_de_un_sitio_nunca_llegan_a_otro() {
        let de = |k: &str| SITES.iter().find(|s| s.key == k).unwrap();
        let claves: Vec<&str> = SITES.iter().map(|s| s.key).collect();

        for k in ["gelbooru", "danbooru", "aibooru", "e621"] {
            let cfg = auth_config(de(k), "12345", "clave")
                .unwrap_or_else(|| panic!("{k} admite credenciales"));
            let v: serde_json::Value = serde_json::from_str(&cfg).unwrap();
            let ext = v["extractor"].as_object().expect("extractor");
            // Nombra a su dueño y a nadie más. Un solo extractor, siempre.
            assert_eq!(ext.len(), 1, "{k}: {cfg}");
            assert!(ext.contains_key(k), "{k}: {cfg}");
            // Y ningún otro sitio aparece por ningún lado del texto: si un día
            // alguien anida algo mal, esto lo caza aunque el objeto tenga una
            // sola clave arriba.
            for otro in claves.iter().filter(|o| **o != k) {
                assert!(!cfg.contains(&format!("\"{otro}\"")), "{k} nombra a {otro}: {cfg}");
            }
        }

        // Gelbooru sigue usando un campo distinto del resto. Confundirlos fue
        // la mitad del fallo original.
        let g: serde_json::Value =
            serde_json::from_str(&auth_config(de("gelbooru"), "12345", "k").unwrap()).unwrap();
        assert_eq!(g["extractor"]["gelbooru"]["user-id"], "12345");
        assert!(g["extractor"]["gelbooru"]["username"].is_null());
        let d: serde_json::Value =
            serde_json::from_str(&auth_config(de("danbooru"), "eric", "k").unwrap()).unwrap();
        assert_eq!(d["extractor"]["danbooru"]["username"], "eric");
        assert!(d["extractor"]["danbooru"]["user-id"].is_null());

        // NOTA: que esto acabe en un ARCHIVO y no en la línea de comandos lo
        // decide `write_booru_auth`, no esta función. Aquí se comprueba el
        // contenido; el continente se comprueba leyendo aquella.

        // Quien no ofrece clave no recibe nada, tenga el usuario lo que tenga
        // puesto. Es lo que impide que un par suelto se reparta por ahí.
        for k in ["yandere", "safebooru", "konachan"] {
            assert!(auth_config(de(k), "12345", "clave").is_none(), "{k} no admite credenciales");
        }

        // Sin credenciales no se escribe nada, ni siquiera para Gelbooru.
        assert!(auth_config(de("gelbooru"), "", "").is_none());
        assert!(auth_config(de("gelbooru"), "12345", "  ").is_none());
        assert!(auth_config(de("danbooru"), " ", "clave").is_none());
    }

    /// Cada sitio tiene su contrato y confundirlos costó el 403.
    ///
    /// Fuente: `danbooru.donmai.us/wiki_pages/help:api`.
    #[test]
    fn cada_sitio_declara_como_quiere_que_se_le_hable() {
        let de = |n: &str| SITES.iter().find(|s| s.name == n).expect(n);

        // Ofrecen clave de API.
        for n in ["Danbooru", "AIBooru", "e621"] {
            let s = de(n);
            assert!(s.admite_clave(), "{n} debería admitir clave");
            let u = s.api_key_url.expect(n);
            assert!(u.starts_with("https://"), "{n}: {u}");
            // La página de la clave tiene que ser del PROPIO sitio, o el aviso
            // mandaría al usuario a generar una clave que no le sirve — el
            // mismo fallo que ya tuvo el aviso de la `cf_clearance`.
            assert!(u.contains(s.dominio()), "{n}: {u} no es de {}", s.dominio());
        }
        for n in ["Safebooru", "yande.re", "Konachan", "Gelbooru"] {
            assert!(!de(n).admite_clave(), "{n} no ofrece clave de API");
        }

        // A LA FAMILIA DANBOORU NO SE LE MANDAN COOKIES. Está MEDIDO: la
        // versión de Linux de la v1.8.5 lista los tres sin cookies, sin cuenta
        // y sin clave, y la de Windows —que mandaba un `cookies.txt` con una
        // `cf_clearance` vieja— se colgaba. Una cookie caducada no equivale a
        // ninguna: presenta un permiso inválido y se gana el rechazo.
        //
        // Este campo ya se cambió una vez por deducción y hubo que deshacerlo.
        // Si alguien vuelve a bajarlo, que sea con una medida delante.
        for n in ["Danbooru", "AIBooru", "e621"] {
            assert!(
                de(n).se_identifica(),
                "{n} va sin cookies y con User-Agent propio: lo dice la medida de Linux"
            );
        }
        for n in ["Safebooru", "yande.re", "Konachan", "Gelbooru"] {
            assert!(!de(n).se_identifica(), "{n} sigue con la sesión del navegador");
        }
        // Gelbooru es el único que NO funciona sin credenciales.
        assert!(de("Gelbooru").needs_auth);
        assert!(!de("Danbooru").needs_auth, "Danbooru funciona sin clave, solo que peor");
    }

}
