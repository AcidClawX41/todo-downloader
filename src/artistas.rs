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
    /// Redes chinas. No salen en el campo `source` de yande.re —medido sobre
    /// 135 fuentes de `genshin_impact`, `honkai:_star_rail` y `arknights`:
    /// cero— pero sí en la base de artistas de Danbooru, que registra TODAS
    /// las direcciones de cada autor. Pidiendo la página 50 con 200 por página
    /// seguía devolviendo resultados: hay más de diez mil artistas con Weibo
    /// apuntado ahí, y muchos son gente que solo publica en chino.
    Weibo,
    Lofter,
    Bilibili,
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
        matches!(
            self,
            Sitio::X
                | Sitio::Bluesky
                | Sitio::Pixiv
                | Sitio::Weibo
                | Sitio::Lofter
                | Sitio::Bilibili
        )
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Sitio::X => "X",
            Sitio::Pixiv => "Pixiv",
            Sitio::Patreon => "Patreon",
            Sitio::Fanbox => "Fanbox",
            Sitio::Bluesky => "Bluesky",
            Sitio::Weibo => "Weibo",
            Sitio::Lofter => "Lofter",
            Sitio::Bilibili => "Bilibili",
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

    /// Añade casas que no se conocían, sin repetir.
    ///
    /// Es lo que hace la ficha de artista de Danbooru: el campo `source` de un
    /// post da UNA dirección, la de esa imagen; la ficha las da todas, y ahí
    /// es donde aparecen Weibo y Lofter.
    pub fn fusionar(&mut self, nuevos: Vec<Perfil>) {
        for p in nuevos {
            if !self.perfiles.iter().any(|x| x.sitio == p.sitio && x.id == p.id) {
                self.perfiles.push(p);
            }
        }
        ordenar_perfiles(&mut self.perfiles);
    }

    /// Crea un artista a partir de las casas que devolvió la ficha.
    ///
    /// `None` si no hay ninguna: un artista sin una sola dirección que ofrecer
    /// no es una fila, es un hueco. Y así el invariante de `perfiles` sigue
    /// garantizado por construcción.
    pub fn desde_perfiles(mut perfiles: Vec<Perfil>, posts: u32, muestras: Vec<String>) -> Option<Self> {
        ordenar_perfiles(&mut perfiles);
        let primero = perfiles.first()?.clone();
        let mut a = Artista::nuevo(primero);
        a.perfiles = perfiles;
        a.posts = posts;
        a.muestras = muestras;
        Some(a)
    }
}

/// Dentro de un artista, primero lo que se puede abrir hoy.
fn ordenar_perfiles(v: &mut [Perfil]) {
    v.sort_by(|x, y| {
        y.sitio
            .abierto()
            .cmp(&x.sitio.abierto())
            .then_with(|| x.url.cmp(&y.url))
    });
}

/// Un post del booru, reducido a lo que hace falta aquí.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostBooru {
    pub source: String,
    pub preview: String,
    /// Nombre del artista según Danbooru, cuando lo publica.
    ///
    /// yande.re no lo separa del resto de etiquetas, así que ahí va vacío.
    /// Danbooru sí lo da en `tag_string_artist`, y es la llave de su base de
    /// artistas — que es donde están las redes chinas. Un post cuyo `source`
    /// no nombra a nadie (una URL de obra de Pixiv, por ejemplo) sigue siendo
    /// útil si trae este nombre.
    pub artista: String,
}


// ===================== BASE DE ARTISTAS DE DANBOORU =====================
//
// El campo `source` de un post da UNA dirección: aquella de la que se sacó esa
// imagen. Danbooru mantiene además una ficha por artista con TODAS las suyas,
// y ahí es donde están las redes chinas.
//
// Medido antes de escribir esto, en agosto de 2026:
//
//   GET /artists.json?search[url_matches]=*weibo.com*&limit=200&page=50
//   -> 200 nombres más. O sea, MÁS DE DIEZ MIL artistas con Weibo apuntado.
//
// Y no es un archivo muerto: las fichas que salieron se habían dado de alta
// ese mismo día. Danbooru llega a nombrar a un artista `weibo_7316173228`
// cuando no tiene ninguna otra cuenta, así que los autores que solo publican
// en chino están bien representados — justo los que el campo `source` de
// yande.re no encuentra nunca.
//
// Un ejemplo real de lo que devuelve:
//
//   {"name":"darkanglicanqpq","urls":[
//     {"url":"https://www.weibo.com/u/5366453585","is_active":true},
//     {"url":"https://www.weibo.com/n/DarkAnglicanQpQ","is_active":true}]}


/// Los boorus donde se busca la ficha de un artista, EN ORDEN.
///
/// Danbooru primero porque su base es mucho mayor. AIBooru después, y no es un
/// añadido decorativo: es el único de los dos que cataloga obra generada con
/// IA, así que sus artistas **no están en Danbooru por definición** —Danbooru
/// la rechaza por norma—. Sin esta segunda consulta, la cosecha los encontraba
/// y luego los dejaba sin ninguno de sus otros perfiles: una fila con nombre y
/// nada donde pulsar.
///
/// El orden importa por lo que cuesta: solo se pregunta al segundo cuando el
/// primero no sabe nada, así que a los artistas normales no les añade ni una
/// petición.
/// Dónde se busca la FICHA de un artista, en orden.
///
/// Son los mismos dos de `BOORUS_DANBOORU` y no es casualidad: Moebooru no
/// tiene base de artistas con direcciones —yande.re y Konachan dan el `source`
/// de cada imagen, no un perfil por autor—, así que preguntarles aquí sería
/// gastar una petición para nada.
pub const BOORUS_FICHA: &[&str] = &["danbooru.donmai.us", "aibooru.online"];

/// La misma consulta, contra el booru que se le diga.
///
/// AIBooru corre el motor de Danbooru, así que `artists.json` acepta los mismos
/// parámetros y devuelve la misma forma. Por eso basta con cambiar el host.
pub fn url_ficha_artista_en(host: &str, identificador: &str) -> String {
    format!(
        "https://{host}/artists.json\
         ?search%5Bany_name_or_url_matches%5D={}&limit=3&only=name,urls",
        porciento(identificador)
    )
}

/// Perfiles registrados en una respuesta de `artists.json`.
///
/// Se ignoran las direcciones marcadas como inactivas: Danbooru las conserva
/// para dejar constancia de que EXISTIERON, y ofrecer una cuenta borrada como
/// si estuviera viva es peor que no ofrecerla.
pub fn perfiles_de_ficha(json: &str) -> Vec<Perfil> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut out: Vec<Perfil> = Vec::new();
    for artista in v.as_array().map(Vec::as_slice).unwrap_or_default() {
        let urls = artista.get("urls").and_then(|u| u.as_array());
        for u in urls.map(Vec::as_slice).unwrap_or_default() {
            if u.get("is_active").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let Some(dir) = u.get("url").and_then(Value::as_str) else {
                continue;
            };
            // `x.com/i/user/1977786163923734528` es el enlace interno por ID
            // numérico que X publica junto al del nombre. Apunta al mismo sitio
            // y no se puede leer, así que sobra.
            if dir.contains("/i/user/") {
                continue;
            }
            if let Some(p) = perfil_de_fuente(dir) {
                if !out.iter().any(|q| q.sitio == p.sitio && q.id == p.id) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Percent-encoding de lo que no es seguro en una query.
fn porciento(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '~' => vec![c.to_string()],
            _ => {
                let mut b = [0u8; 4];
                c.encode_utf8(&mut b)
                    .as_bytes()
                    .iter()
                    .map(|x| format!("%{x:02X}"))
                    .collect()
            }
        })
        .collect()
}


// ======================= NOMBRES CHINOS DEL PERSONAJE =======================
//
// La etiqueta de un booru es `toki_(blue_archive)`, pero en Weibo ese mismo
// personaje se busca como `#飞鸟马时#`. El puente entre las dos cosas ya está
// escrito y mantenido por otros: la ficha del wiki de Danbooru guarda los
// nombres alternativos, y ahí está el chino literal.
//
//   GET /wiki_pages.json?search[title]=toki_(blue_archive)&only=title,other_names
//   {"other_names":["飛鳥馬トキ","Asuma_Toki","飞鸟马时","小时","トキ(ブルアカ)","トキ"]}
//
//   GET /wiki_pages.json?search[title]=tohsaka_rin
//   {"other_names":["遠坂凛","远坂凛"]}
//
// Una petición por búsqueda y no hay nada que mantener a mano.

/// La ficha del wiki de un personaje.
pub fn url_wiki(tag: &str) -> String {
    format!(
        "https://danbooru.donmai.us/wiki_pages.json\
         ?search%5Btitle%5D={}&only=title,other_names&limit=1",
        porciento(tag.trim())
    )
}

/// Nombres alternativos que sirven para buscar en una red china.
///
/// Se queda con los que son SOLO ideogramas y descarta:
///
/// - los que llevan kana —`飛鳥馬トキ`, `トキ(ブルアカ)`— porque son japoneses y
///   en Weibo no devuelven nada;
/// - los que llevan letras latinas —`Asuma_Toki`—, que son la romanización.
///
/// NO se intenta distinguir el chino simplificado del tradicional: `遠坂凛` y
/// `远坂凛` son los dos ideogramas puros y separarlos necesitaría una tabla de
/// conversión de varios miles de caracteres. Se ofrecen ambos y que elija
/// quien busca, que además tarda un clic. Inventarse cuál es el bueno sería
/// acertar la mitad de las veces sin decirlo.
pub fn alias_chinos(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for ficha in v.as_array().map(Vec::as_slice).unwrap_or_default() {
        let nombres = ficha.get("other_names").and_then(|n| n.as_array());
        for n in nombres.map(Vec::as_slice).unwrap_or_default() {
            let Some(nombre) = n.as_str() else { continue };
            if es_ideografico(nombre) && !out.iter().any(|x| x == nombre) {
                out.push(nombre.to_string());
            }
        }
    }
    out
}

/// ¿Está escrito solo con ideogramas? Ni kana, ni letras latinas.
fn es_ideografico(s: &str) -> bool {
    let mut hay_han = false;
    for c in s.chars() {
        let u = c as u32;
        match u {
            // Han, incluidas las extensiones de uso común
            0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF => hay_han = true,
            // Hiragana y katakana: es japonés, no sirve en Weibo
            0x3040..=0x30FF | 0x31F0..=0x31FF => return false,
            _ if c.is_ascii_alphabetic() => return false,
            // Signos de puntuación sueltos no descalifican
            _ => {}
        }
    }
    hay_han
}

/// Búsqueda de ese nombre como etiqueta en Weibo.
///
/// Weibo marca las etiquetas entre almohadillas: `#飞鸟马时#`. Sin ellas la
/// búsqueda es de texto libre y devuelve mucho ruido.
pub fn url_busqueda_weibo(nombre: &str) -> String {
    format!("https://s.weibo.com/weibo?q={}", porciento(&format!("#{}#", nombre.trim())))
}


// ========================== NOMBRES EN CHINO ==========================
//
// La etiqueta de un booru es `toki_(blue_archive)`; en Weibo ese personaje se
// busca como `#飞鸟马时#`. Son dos mundos distintos y esta lista es el puente.
//
// NINGUNO ESTÁ ESCRITO DE MEMORIA. Cada uno sale del campo `other_names` de la
// ficha del wiki de Danbooru, consultado uno a uno antes de entrar aquí — el
// mismo criterio que `SAMPLE_TAGS`, que se comprobó contra la API de Gelbooru.
// Un ejemplo que no devuelve nada es peor que no ponerlo: la primera búsqueda
// falla y parece que la función no sirve.
//
//   GET /wiki_pages.json?search[title]=toki_(blue_archive)&only=other_names
//   {"other_names":["飛鳥馬トキ","Asuma_Toki","飞鸟马时","小时", …]}
//
// De ahí se descarta lo que lleva kana —`飛鳥馬トキ` es japonés y en Weibo no
// devuelve nada— y la romanización.
//
// Y SE DESCARTA TAMBIÉN LO DE UN SOLO CARÁCTER, que es la regla menos obvia:
// el wiki da `空` para Aether y `荧` para Lumine, y en chino esos caracteres
// significan «cielo» y «luciérnaga». Buscarlos devuelve el idioma entero. Un
// nombre que no distingue no es un ejemplo, es una trampa.
//
// La lista es un SUBCONJUNTO comprobado, no las cincuenta y cinco de
// `SAMPLE_TAGS`: añadir uno es una línea, pero cada línea cuesta consultar su
// ficha y mirar que la búsqueda devuelva algo.
/// Una entrada del desplegable en chino.
///
/// `serie` va como DATO y no dentro de la etiqueta. La primera versión ponía
/// «Blue Archive — la serie» escrito a mano, y esa palabra en castellano se
/// colaba tal cual en la interfaz en inglés. Lo que se traduce lo pone la
/// interfaz; aquí solo va lo que no cambia de idioma.
pub struct Ejemplo {
    pub chino: &'static str,
    pub etiqueta: &'static str,
    pub serie: bool,
}

impl Ejemplo {
    const fn personaje(chino: &'static str, etiqueta: &'static str) -> Self {
        Self { chino, etiqueta, serie: false }
    }
    const fn serie(chino: &'static str, etiqueta: &'static str) -> Self {
        Self { chino, etiqueta, serie: true }
    }
}

// Un nombre con letra latina NO entra, aunque el wiki lo registre. Luffy es
// el caso: el wiki da `蒙奇·D·路飞`, con la D del apellido, que en Weibo obliga
// a escribir un carácter que nadie teclea al buscar. El mismo wiki ofrece
// `草帽路飞` —«Luffy el del sombrero de paja»—, todo ideogramas y de uso
// corriente. Se elige el nombre alternativo; la regla se queda como está.
pub const EJEMPLOS_CHINOS: &[Ejemplo] = &[
    // --- Blue Archive ---
    Ejemplo::personaje("飞鸟马时", "Toki — Blue Archive"),
    Ejemplo::personaje("砂狼白子", "Shiroko — Blue Archive"),
    Ejemplo::personaje("奥空绫音", "Ayane — Blue Archive"),
    Ejemplo::personaje("杏山和纱", "Kazusa — Blue Archive"),
    Ejemplo::personaje("京极皋月", "Satsuki — Blue Archive"),
    Ejemplo::personaje("枣伊吕波", "Iroha — Blue Archive"),
    Ejemplo::personaje("春原心奈", "Kokona — Blue Archive"),
    Ejemplo::serie("碧蓝档案", "Blue Archive"),
    // --- Type-Moon ---
    //
    // SOLO RIN. Akiha Tohno se quedó fuera y merece la explicación: su ficha
    // da `遠野秋葉`, que es el kanji JAPONÉS. En chino se escribiría
    // `远野秋叶`, y esa forma no está registrada. Poner la japonesa sería
    // ofrecer una búsqueda que puede no devolver nada.
    Ejemplo::personaje("远坂凛", "Rin Tohsaka — Fate"),
    // --- Evangelion ---
    Ejemplo::personaje("惣流·明日香·兰格雷", "Asuka Langley Soryu — Evangelion"),
    Ejemplo::personaje("式波·明日香·兰格雷", "Asuka Langley Shikinami — Rebuild"),
    // --- One Piece ---
    Ejemplo::personaje("草帽路飞", "Monkey D. Luffy — One Piece"),
    Ejemplo::personaje("娜美", "Nami — One Piece"),
    Ejemplo::personaje("山治", "Sanji — One Piece"),
    Ejemplo::personaje("弗兰奇", "Franky — One Piece"),
    Ejemplo::personaje("马尔科", "Marco — One Piece"),
    Ejemplo::personaje("萨博", "Sabo — One Piece"),
    Ejemplo::personaje("凯多", "Kaido — One Piece"),
    Ejemplo::serie("海贼王", "One Piece"),
    // --- Jujutsu Kaisen ---
    //
    // Faltan Mahito y Tengen a propósito: su ficha da `真人` y `天元`, que en
    // chino significan «persona real» y «origen celestial». Buscarlos devuelve
    // el idioma, no al personaje — el mismo caso que `空` y `荧`.
    Ejemplo::personaje("五条悟", "Gojo Satoru — Jujutsu Kaisen"),
    Ejemplo::personaje("两面宿傩", "Sukuna — Jujutsu Kaisen"),
    Ejemplo::personaje("胀相", "Choso — Jujutsu Kaisen"),
    Ejemplo::personaje("魔虚罗", "Mahoraga — Jujutsu Kaisen"),
    Ejemplo::serie("咒术回战", "Jujutsu Kaisen"),
    // --- Chainsaw Man ---
    Ejemplo::personaje("玛奇玛", "Makima — Chainsaw Man"),
    Ejemplo::personaje("电次", "Denji — Chainsaw Man"),
    Ejemplo::serie("电锯人", "Chainsaw Man"),
    // --- Genshin Impact ---
    Ejemplo::personaje("甘雨", "Ganyu — Genshin"),
    Ejemplo::personaje("钟离", "Zhongli — Genshin"),
    Ejemplo::personaje("散兵", "Scaramouche — Genshin"),
    Ejemplo::personaje("克洛琳德", "Clorinde — Genshin"),
    Ejemplo::personaje("达达利亚", "Tartaglia — Genshin"),
    Ejemplo::personaje("迪卢克", "Diluc — Genshin"),
    // --- Otros ---
    Ejemplo::personaje("初音未来", "Hatsune Miku"),
    Ejemplo::personaje("灶门祢豆子", "Nezuko — Demon Slayer"),
    Ejemplo::personaje("俾斯麦", "Bismarck — Azur Lane"),
    // --- Kaguya-sama --- (辉夜大小姐想让我告白)
    //
    // FUENTE DISTINTA AL RESTO, y conviene que se sepa: estos once salen de
    // 萌娘百科 y de la Wikipedia en chino, no del wiki de Danbooru, que dejó
    // de responder mientras se añadían. Son los nombres que usan los propios
    // aficionados chinos, que es justo lo que hace falta para buscar en Weibo.
    Ejemplo::serie("辉夜大小姐想让我告白", "Kaguya-sama: Love Is War"),
    Ejemplo::personaje("四宫辉夜", "Kaguya Shinomiya — Kaguya-sama"),
    Ejemplo::personaje("早坂爱", "Ai Hayasaka — Kaguya-sama"),
    Ejemplo::personaje("藤原千花", "Chika Fujiwara — Kaguya-sama"),

    // --- Shingeki no Kyojin --- (进击的巨人)
    //
    // Estos cuatro son transliteraciones de nombres occidentales, con el
    // punto medio que usa el chino para separarlos. Pasan la regla —son
    // ideogramas y el punto no descalifica— pero como etiqueta de Weibo son
    // menos seguros que un nombre japonés escrito en han. Por eso va también
    // la serie: esa sí es una etiqueta viva con toda seguridad.
    Ejemplo::serie("进击的巨人", "Attack on Titan"),
    Ejemplo::personaje("希斯特莉亚·雷斯", "Historia Reiss — Attack on Titan"),
    Ejemplo::personaje("阿尼·利昂纳德", "Annie Leonhart — Attack on Titan"),
    Ejemplo::personaje("萨莎·布劳斯", "Sasha Blouse — Attack on Titan"),
    Ejemplo::personaje("韩吉·佐耶", "Hange Zoe — Attack on Titan"),

    // --- Haruhi Suzumiya --- (凉宫春日)
    Ejemplo::personaje("凉宫春日", "Haruhi Suzumiya"),
    Ejemplo::personaje("长门有希", "Yuki Nagato — Haruhi Suzumiya"),
    Ejemplo::personaje("朝比奈实玖瑠", "Mikuru Asahina — Haruhi Suzumiya"),
    // --- Sueltos ---
    //
    // Kanna es la jefa de Seguridad Pública de Blue Archive; en japonés se
    // escribe 尾刃 カンナ, con el nombre en katakana, y el chino lo pasa
    // entero a han: 尾刃神奈. Es el título de su ficha en 萌娘百科.
    Ejemplo::personaje("尾刃神奈", "Kanna — Blue Archive"),
    // Carrera, el «原初之黄» de los demonios primigenios de Tensura.
    //
    // OJO CON EL ÚLTIMO CARÁCTER: la ficha de 萌娘百科 se titula 卡蕾菈, con
    // 菈, pero su propio texto usa 卡蕾拉, con 拉. Circulan las dos. Va la del
    // título, que es la forma canónica de esa enciclopedia; si en Weibo no
    // devuelve, la otra variante es lo primero que hay que probar.
    Ejemplo::personaje("卡蕾菈", "Carrera — Tensura"),
    // Akame. Su nombre japonés, アカメ, va en katakana y significa «ojo rojo»;
    // el chino lo traduce en vez de transcribirlo: 赤瞳. Coincide en 萌娘百科,
    // 百度百科 y el wiki de la obra, así que es la forma que usan de verdad.
    Ejemplo::personaje("赤瞳", "Akame — Akame ga Kill!"),
    // Y la serie, que en chino lleva signo de admiración en medio: 斩！赤红之瞳.
    // El signo es de ancho completo (！, U+FF01), no el ASCII.
    Ejemplo::serie("斩！赤红之瞳", "Akame ga Kill!"),
    // --- Roshidere ---
    //
    // 周防有希 es de las BUENAS para Weibo: nombre japonés escrito en han,
    // cuatro caracteres, sin transliteración de por medio. Título de su ficha
    // en 萌娘百科 y en 百度百科.
    Ejemplo::personaje("周防有希", "Yuki Suou — Roshidere"),
    // Alya: DOS ENTRADAS, y el porqué merece quedar escrito.
    //
    // Su nombre oficial en chino es 艾莉莎·米哈伊罗芙娜·九条 —trece caracteres
    // con dos puntos medios— y está bien: es el título de su ficha en 萌娘百科
    // y en 百度百科. Pero se PROBÓ en Weibo y devuelve CERO. Ser el nombre
    // correcto y ser lo que la gente teclea no es lo mismo, y para buscar solo
    // vale lo segundo. No volver a ponerlo.
    //
    // Estas dos son las formas vivas: la del título chino de la obra, y el
    // nombre en orden natural sin transcripción del patronímico.
    Ejemplo::personaje("艾莉同学", "Alya — Roshidere"),
    Ejemplo::personaje("九条艾莉莎", "Alya (nombre completo) — Roshidere"),
    // --- My Dress-Up Darling --- (更衣人偶坠入爱河)
    //
    // 喜多川海梦 es de los buenos: nombre japonés escrito en han, cinco
    // caracteres, sin transcripción de por medio. Coincide en 萌娘百科, en la
    // Wikipedia en chino y en 百度百科.
    Ejemplo::personaje("喜多川海梦", "Marin Kitagawa — My Dress-Up Darling"),
    Ejemplo::serie("更衣人偶坠入爱河", "My Dress-Up Darling"),
];

/// Lo mismo en Bilibili, que no usa almohadillas: allí la etiqueta se busca
/// como texto y el propio sitio reparte los resultados por tipo.
pub fn url_busqueda_bilibili(nombre: &str) -> String {
    format!("https://search.bilibili.com/all?keyword={}", porciento(nombre.trim()))
}

/// A qué red china lleva el botón.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RedChina {
    /// Ilustración y fotografía. Es donde está el artwork.
    #[default]
    Weibo,
    /// Vídeo, y también ilustración en sus columnas.
    Bilibili,
}

impl RedChina {
    pub fn nombre(self) -> &'static str {
        match self {
            RedChina::Weibo => "Weibo",
            RedChina::Bilibili => "Bilibili",
        }
    }

    pub fn url(self, nombre: &str) -> String {
        match self {
            RedChina::Weibo => url_busqueda_weibo(nombre),
            RedChina::Bilibili => url_busqueda_bilibili(nombre),
        }
    }
}



/// El `posts.json` del motor de Danbooru, sea cual sea el sitio que lo sirve.
pub fn url_cosecha_danbooru_en(host: &str, tag: &str, pagina: u32) -> String {
    format!(
        "https://{host}/posts.json?tags={}&limit=100&page={}",
        porciento(tag.trim()),
        pagina.max(1)
    )
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

    // --- Weibo ---
    //
    // Danbooru apunta dos formas y las dos aparecen: `/u/<uid numérico>`, que
    // es la canónica, y `/n/<apodo>`, que puede llevar caracteres chinos. Se
    // prefiere la numérica porque es la que entiende el extractor y la que no
    // cambia cuando el autor se cambia el nombre; el apodo se acepta también,
    // porque hay artistas de los que solo consta esa.
    if host_es(&host, "weibo.com") || host_es(&host, "weibo.cn") {
        return match seg.as_slice() {
            ["u", id, ..] if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) => {
                Some(Perfil {
                    sitio: Sitio::Weibo,
                    id: (*id).to_string(),
                    url: format!("https://weibo.com/u/{id}"),
                })
            }
            ["n", nombre, ..] if !nombre.is_empty() => Some(Perfil {
                sitio: Sitio::Weibo,
                id: (*nombre).to_string(),
                url: format!("https://weibo.com/n/{nombre}"),
            }),
            _ => None,
        };
    }

    // --- Lofter: el usuario ES el subdominio, `<usuario>.lofter.com` ---
    if host_es(&host, "lofter.com") && host != "lofter.com" {
        let id = host.trim_end_matches(".lofter.com").to_string();
        // `www` ya lo quitó `host_de`, pero el resto de subdominios de servicio
        // no son artistas. Los `imglfN.lofter.com` son sus CDN de imágenes y
        // aparecen a montones en cualquier página: sin esta línea, el listado
        // se llenaría de «artistas» llamados `imglf3`.
        let servicio = id.starts_with("imglf") || matches!(id.as_str(), "i" | "img" | "static" | "api");
        if id_plausible(&id) && !servicio {
            return Some(Perfil {
                sitio: Sitio::Lofter,
                url: format!("https://{id}.lofter.com"),
                id,
            });
        }
        return None;
    }

    // --- Bilibili: `space.bilibili.com/<uid>` ---
    //
    // Ya salió once veces en la primera medición de la v1.8.0 y se tiraba,
    // porque no había un `Sitio` donde ponerlo.
    if host_es(&host, "bilibili.com") {
        // El uid va en el subdominio `space.` o en el primer segmento, según
        // de dónde se haya copiado el enlace. Las dos formas circulan.
        let id = if host == "space.bilibili.com" {
            seg.first().copied().unwrap_or("")
        } else if let ["space", u, ..] = seg.as_slice() {
            u
        } else {
            ""
        };
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            return Some(Perfil {
                sitio: Sitio::Bilibili,
                id: id.to_string(),
                url: format!("https://space.bilibili.com/{id}"),
            });
        }
        return None;
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


/// Los boorus que corren MOEBOORU: mismo `post.json`, mismo `source`.
///
/// Son la fuente de artistas ORIGINALES. Su campo `source` apunta a la
/// publicación de la que salió cada imagen —X, Pixiv, Patreon, Fanbox—, que es
/// lo que convierte una etiqueta de personaje en una lista de perfiles.
///
/// Konachan entra porque no cuesta nada: corre el mismo motor que yande.re, así
/// que `parse_posts` lo entiende sin tocar una línea. Solo cambia el host.
pub const BOORUS_MOEBOORU: &[&str] = &["yande.re", "konachan.com"];

/// Los que corren el motor de DANBOORU: mismo `posts.json`, mismo
/// `tag_string_artist`.
///
/// Aportan el NOMBRE del artista, que es la llave de su base de fichas. Y
/// AIBooru aporta lo que ningún otro puede: los que generan con IA, que
/// Danbooru y los Moebooru rechazan por norma.
pub const BOORUS_DANBOORU: &[(&str, &str)] =
    &[("Danbooru", "danbooru.donmai.us"), ("AIBooru", "aibooru.online")];

/// La consulta de Moebooru, contra el host que se le diga.
pub fn url_cosecha_moebooru(host: &str, tag: &str, pagina: u32) -> String {
    format!(
        "https://{host}/post.json?tags={}&limit=100&page={}",
        porciento(tag.trim()),
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
            artista: String::new(),
        })
        .collect()
}

/// Lo mismo para Danbooru, que nombra sus campos de otra manera.
///
/// Su miniatura es `preview_file_url` y el autor viene aparte, en
/// `tag_string_artist`. Puede traer varios nombres separados por espacios
/// —una colaboración— y en ese caso se coge el primero: repartir un post entre
/// dos artistas inflaría las cuentas de los dos.
pub fn parse_posts_danbooru(json: &str) -> Vec<PostBooru> {
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
                .get("preview_file_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            artista: p
                .get("tag_string_artist")
                .and_then(Value::as_str)
                .unwrap_or("")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string(),
        })
        .collect()
}

/// Cuántas miniaturas de muestra se guardan por artista.
pub const MUESTRAS: usize = 4;

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
        ordenar_perfiles(&mut a.perfiles);
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
            PostBooru { source: "https://www.fanbox.cc/@inanakisiki/posts/1".into(), preview: String::new(), ..Default::default() },
            PostBooru { source: "https://inanakisiki.fanbox.cc/posts/2".into(), preview: String::new(), ..Default::default() },
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
            ..Default::default()
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
            PostBooru { source: "https://siino13.fanbox.cc/posts/1".into(), preview: "a".into(), ..Default::default() },
            PostBooru { source: "https://siino13.fanbox.cc/posts/2".into(), preview: "b".into(), ..Default::default() },
            PostBooru { source: "https://x.com/Siino_13/status/9".into(), preview: "c".into(), ..Default::default() },
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
                ..Default::default()
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
                ..Default::default()
            })
            .collect();
        let a = agrupar(&posts);
        assert_eq!(a[0].posts, 10, "se cuentan todos");
        assert_eq!(a[0].muestras.len(), MUESTRAS, "pero solo se guardan cuatro");
    }


    /// Las tres redes que se añadieron en la v1.8.5. Las dos primeras no
    /// aparecen NUNCA en el campo `source` de yande.re —medido: 135 fuentes,
    /// cero Weibo— así que la única vía es la ficha de artista de Danbooru.
    /// Bilibili sí salía, once veces en la medición de la v1.8.0, y se tiraba
    /// porque no había un `Sitio` donde meterlo.
    #[test]
    fn se_reconocen_las_redes_chinas() {
        let p = |u: &str| perfil_de_fuente(u).map(|p| (p.sitio, p.id, p.url));

        // Weibo: la forma canónica es el uid numérico.
        assert_eq!(
            p("https://www.weibo.com/u/5366453585"),
            Some((Sitio::Weibo, "5366453585".into(), "https://weibo.com/u/5366453585".into()))
        );
        // Y la del apodo, que puede ir en chino y a veces es la única que consta.
        assert_eq!(
            p("https://www.weibo.com/n/DarkAnglicanQpQ"),
            Some((Sitio::Weibo, "DarkAnglicanQpQ".into(), "https://weibo.com/n/DarkAnglicanQpQ".into()))
        );
        assert_eq!(
            p("https://weibo.com/n/\u{8997}\u{6a02}").map(|(s, _, _)| s),
            Some(Sitio::Weibo)
        );
        // Un post suelto no nombra a su autor.
        assert!(p("https://weibo.com/7187265342/QvRHJ0FYJ").is_none());

        // Lofter: el usuario ES el subdominio.
        assert_eq!(
            p("https://miuran.lofter.com/post/1e2f3a_abc"),
            Some((Sitio::Lofter, "miuran".into(), "https://miuran.lofter.com".into()))
        );
        // Los subdominios de servicio no son artistas.
        assert!(p("https://imglf3.lofter.com/img/abc.jpg").is_none());
        assert!(p("https://www.lofter.com/").is_none());

        // Bilibili, en sus dos formas.
        for u in ["https://space.bilibili.com/12345", "https://www.bilibili.com/space/12345"] {
            assert_eq!(
                p(u),
                Some((Sitio::Bilibili, "12345".into(), "https://space.bilibili.com/12345".into())),
                "{u}"
            );
        }
        assert!(p("https://www.bilibili.com/video/BV1xx411c7mD").is_none());

        // Por host y nunca por subcadena, como todo lo demás desde la v1.7.0.
        assert!(p("https://weibo.com.atacante.example/u/1").is_none());
        assert!(p("https://lofter.com.atacante.example/x").is_none());
        assert!(p("https://bilibili.com.atacante.example/space/1").is_none());

        // Los tres son abiertos: no cobran por creador.
        for s in [Sitio::Weibo, Sitio::Lofter, Sitio::Bilibili] {
            assert!(s.abierto(), "{s:?}");
        }
    }

    /// La ficha de artista de Danbooru es la que trae las redes chinas.
    /// Respuesta real de `artists.json?only=name,urls`, recortada.
    #[test]
    fn la_ficha_de_danbooru_da_todas_las_casas() {
        let j = r#"[{"name":"darkanglicanqpq","urls":[
            {"url":"https://www.weibo.com/u/5366453585","is_active":true},
            {"url":"https://www.weibo.com/n/DarkAnglicanQpQ","is_active":true},
            {"url":"https://x.com/dark_q","is_active":true},
            {"url":"https://x.com/i/user/1977786163923734528","is_active":true},
            {"url":"https://twitter.com/dark_q","is_active":true},
            {"url":"https://old.example/perdida","is_active":false}]}]"#;
        let v = perfiles_de_ficha(j);
        let tiene = |s: Sitio, id: &str| v.iter().any(|p| p.sitio == s && p.id == id);
        assert!(tiene(Sitio::Weibo, "5366453585"));
        assert!(tiene(Sitio::Weibo, "DarkAnglicanQpQ"));
        assert!(tiene(Sitio::X, "dark_q"));
        // `x.com/i/user/<id>` es el enlace interno por ID que X publica al lado
        // del del nombre: apunta al mismo sitio y no hay quien lo lea.
        assert!(!v.iter().any(|p| p.id.chars().all(|c| c.is_ascii_digit()) && p.sitio == Sitio::X));
        // `x.com` y `twitter.com` son el mismo perfil: una sola entrada.
        assert_eq!(v.iter().filter(|p| p.sitio == Sitio::X).count(), 1);
        // Una dirección desactivada es una cuenta que YA NO está. Ofrecerla
        // como viva es peor que no ofrecerla.
        assert_eq!(v.len(), 3, "{v:?}");

        // Nada que reventar con una respuesta rara.
        assert!(perfiles_de_ficha("[]").is_empty());
        assert!(perfiles_de_ficha("no soy json").is_empty());
        assert!(perfiles_de_ficha(r#"[{"name":"x"}]"#).is_empty());
        assert!(perfiles_de_ficha(r#"[{"name":"x","urls":[{"url":"nada"}]}]"#).is_empty());
    }

    #[test]
    fn las_urls_de_consulta_van_escapadas() {
        let u = url_ficha_artista_en(BOORUS_FICHA[0], "bismarck_(azur_lane)");
        assert!(u.contains("any_name_or_url_matches%5D=bismarck_%28azur_lane%29"), "{u}");
        assert!(u.contains("only=name,urls"), "{u}");
        assert!(!u.contains(' '), "una URL no puede llevar espacios: {u}");

        // AIBooru corre el mismo motor: misma ruta, mismos parámetros, misma
        // forma de respuesta. Lo único que cambia es el host.
        assert_eq!(
            url_cosecha_danbooru_en("aibooru.online", "tohsaka_rin", 1),
            "https://aibooru.online/posts.json?tags=tohsaka_rin&limit=100&page=1"
        );
        assert!(url_cosecha_danbooru_en("aibooru.online", "a b", 0).contains("tags=a%20b"), "escapa igual");
        assert!(url_cosecha_danbooru_en("aibooru.online", "x", 0).ends_with("page=1"), "la página nunca es 0");
        // Y no se confunden entre sí: un fallo aquí mandaría la cosecha de una
        // al sitio de la otra y el aviso nombraría al inocente.
        assert!(url_cosecha_danbooru_en("danbooru.donmai.us", "x", 1).contains("danbooru.donmai.us"));
        assert!(!url_cosecha_danbooru_en("aibooru.online", "x", 1).contains("donmai.us"));

        // La cosecha de yande.re y la de Danbooru comparten codificador.
        assert!(url_cosecha_moebooru("yande.re", "tohsaka_rin", 2).ends_with("tags=tohsaka_rin&limit=100&page=2"));
        assert!(url_cosecha_danbooru_en("danbooru.donmai.us", "tohsaka_rin", 0).ends_with("&page=1"), "página mínima 1");
        assert!(url_cosecha_danbooru_en("danbooru.donmai.us", "a b", 1).contains("tags=a%20b"));
    }


    /// Danbooru nombra sus campos distinto y, sobre todo, publica el AUTOR.
    /// Ese nombre es la llave de su base de artistas, que es donde están las
    /// redes chinas.
    #[test]
    fn los_posts_de_danbooru_traen_el_nombre_del_autor() {
        let j = r#"[
          {"source":"https://www.pixiv.net/artworks/1",
           "preview_file_url":"https://cdn/p1.jpg",
           "tag_string_artist":"darkanglicanqpq"},
          {"source":"",
           "preview_file_url":"https://cdn/p2.jpg",
           "tag_string_artist":"uno dos"},
          {"source":"https://x.com/alguien/status/3","preview_file_url":"https://cdn/p3.jpg"}
        ]"#;
        let v = parse_posts_danbooru(j);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].artista, "darkanglicanqpq");
        assert_eq!(v[0].preview, "https://cdn/p1.jpg");
        // Una colaboración trae dos nombres: se coge el primero, porque
        // repartir el post entre los dos inflaría las cuentas de ambos.
        assert_eq!(v[1].artista, "uno");
        // Sin autor apuntado, campo vacío y no se inventa nada.
        assert_eq!(v[2].artista, "");
        assert!(parse_posts_danbooru("no soy json").is_empty());
    }

    /// Fusionar es lo que convierte «una dirección por post» en «todas las
    /// casas del artista». Sin duplicar y con las abiertas delante.
    #[test]
    fn fusionar_anade_casas_sin_repetir() {
        let x = Perfil {
            sitio: Sitio::X,
            id: "alguien".into(),
            url: "https://x.com/alguien".into(),
        };
        let mut a = Artista::desde_perfiles(vec![x.clone()], 5, vec![]).unwrap();
        assert_eq!(a.perfiles().len(), 1);

        a.fusionar(vec![
            x.clone(), // el mismo: no se repite
            Perfil {
                sitio: Sitio::Weibo,
                id: "5366453585".into(),
                url: "https://weibo.com/u/5366453585".into(),
            },
            Perfil {
                sitio: Sitio::Fanbox,
                id: "alguien".into(),
                url: "https://www.fanbox.cc/@alguien".into(),
            },
        ]);
        assert_eq!(a.perfiles().len(), 3, "{:?}", a.perfiles());
        // Fanbox cobra por creador, así que va la última: lo que se puede
        // abrir hoy se enseña primero.
        assert_eq!(a.perfiles().last().unwrap().sitio, Sitio::Fanbox);
        assert!(a.principal().sitio.abierto());

        // Sin ninguna casa no hay artista: eso sería un hueco, no una fila.
        assert!(Artista::desde_perfiles(vec![], 3, vec![]).is_none());
    }


    /// El puente entre la etiqueta del booru y el hashtag de Weibo. Respuestas
    /// reales del wiki de Danbooru.
    ///
    /// Los nombres van con los caracteres LITERALES y no con escapes. En un
    /// `r#"…"#` los escapes de Rust no se interpretan, y `\u{XXXX}` tampoco es
    /// la sintaxis de JSON —que usa `\uXXXX`, sin llaves—, así que serde no
    /// parseaba nada y la función devolvía una lista vacía. Escribirlos tal
    /// cual quita el problema y además se leen.
    #[test]
    fn los_nombres_chinos_salen_del_wiki() {
        let toki = r#"[{"title":"toki_(blue_archive)","other_names":
            ["飛鳥馬トキ","Asuma_Toki","飞鸟马时","小时","トキ(ブルアカ)","トキ"]}]"#;
        let v = alias_chinos(toki);
        // El hashtag que se busca de verdad en Weibo.
        assert!(v.contains(&"飞鸟马时".to_string()), "{v:?}");
        assert!(v.contains(&"小时".to_string()), "{v:?}");
        // Con katakana dentro es japonés: en Weibo no devuelve nada.
        assert!(!v.iter().any(|x| x.contains('ト')), "{v:?}");
        // Y la romanización tampoco sirve.
        assert!(!v.iter().any(|x| x.contains("Toki")), "{v:?}");
        assert_eq!(v.len(), 2, "{v:?}");

        // Los dos de Rin se ofrecen: uno es tradicional y otro simplificado, y
        // separarlos necesitaría una tabla de miles de caracteres.
        let rin = r#"[{"title":"tohsaka_rin","other_names":["遠坂凛","远坂凛"]}]"#;
        assert_eq!(alias_chinos(rin), vec!["遠坂凛".to_string(), "远坂凛".to_string()]);

        assert!(alias_chinos("[]").is_empty());
        assert!(alias_chinos("no soy json").is_empty());
        assert!(alias_chinos(r#"[{"title":"x"}]"#).is_empty());
        // Sin un solo ideograma no hay nada que buscar en Weibo.
        assert!(alias_chinos(r#"[{"title":"x","other_names":["Hatsune_Miku","39"]}]"#).is_empty());
    }

    #[test]
    fn la_busqueda_de_weibo_lleva_almohadillas() {
        let u = url_busqueda_weibo("飞鸟马时");
        assert!(u.starts_with("https://s.weibo.com/weibo?q="), "{u}");
        // `#` va escapado, o Weibo lo tomaría por un fragmento de URL.
        assert!(u.contains("%23"), "{u}");
        assert!(!u.contains('#'), "{u}");
        assert!(url_wiki("toki_(blue_archive)").contains("title%5D=toki_%28blue_archive%29"));

    }


    /// Cada nombre de la lista sale del wiki de Danbooru, nunca de memoria.
    /// Este test no puede comprobar eso —haría falta red— pero sí las reglas
    /// que hacen que un nombre SIRVA para buscar.
    #[test]
    fn los_ejemplos_chinos_son_usables() {
        assert!(!EJEMPLOS_CHINOS.is_empty());
        for Ejemplo { chino, etiqueta, .. } in EJEMPLOS_CHINOS {
            // Ideogramas, sin kana ni romanización: es lo que se escribe en
            // el buscador de Weibo.
            assert!(es_ideografico(chino), "{chino} ({etiqueta}) no es chino");
            // De un solo carácter, no. El wiki da `空` para Aether y `荧` para
            // Lumine, que en chino son «cielo» y «luciérnaga»: buscarlos
            // devuelve el idioma entero.
            assert!(
                chino.chars().count() >= 2,
                "{chino} ({etiqueta}) es demasiado corto para distinguir nada"
            );
            assert!(!etiqueta.is_empty(), "{chino} sin etiqueta");
            // El punto medio separa nombre y apellido en las
            // transliteraciones (阿尼·利昂纳德). Se admite, pero ni al
            // principio ni al final: eso sería un nombre partido.
            assert!(!chino.starts_with('·') && !chino.ends_with('·'), "{chino}");
            // La etiqueta es lo que NO cambia de idioma. Si aquí se colara
            // texto en castellano, aparecería tal cual en la interfaz en
            // inglés — que es justo lo que pasó con «— la serie».
            for palabra in [" la serie", " the series", " el personaje"] {
                assert!(
                    !etiqueta.contains(palabra),
                    "{etiqueta}: lo traducible lo pone la interfaz, no la lista"
                );
            }
        }
        // Sin repetidos: dos filas iguales en un desplegable son un despiste.
        let mut vistos: Vec<&str> = EJEMPLOS_CHINOS.iter().map(|e| e.chino).collect();
        let antes = vistos.len();
        vistos.sort_unstable();
        vistos.dedup();
        assert_eq!(vistos.len(), antes, "hay nombres repetidos");

        // Y cada uno produce una búsqueda válida en las dos redes.
        for Ejemplo { chino, .. } in EJEMPLOS_CHINOS {
            let w = url_busqueda_weibo(chino);
            assert!(w.starts_with("https://s.weibo.com/weibo?q=%23"), "{w}");
            assert!(url_busqueda_bilibili(chino).contains("keyword=%"), "{chino}");
        }
    }

    #[test]
    fn la_url_de_cosecha_escapa_la_etiqueta() {
        assert_eq!(
            url_cosecha_moebooru("yande.re", "yukinoshita_yukino", 1),
            "https://yande.re/post.json?tags=yukinoshita_yukino&limit=100&page=1"
        );
        // Los paréntesis de las etiquetas de obra deben ir escapados.
        assert!(url_cosecha_moebooru("yande.re", "artoria_pendragon_(fate)", 2).contains("%28fate%29"));
        assert!(url_cosecha_moebooru("yande.re", "x", 0).ends_with("page=1"), "la página nunca es 0");
    }

    /// Un artista de IA está en AIBooru y NO puede estar en Danbooru, que
    /// rechaza esa obra por norma. Preguntar solo al primero dejaba esas filas
    /// con un nombre y ningún perfil donde pulsar.
    #[test]
    fn la_ficha_se_busca_en_los_dos_boorus() {
        assert_eq!(BOORUS_FICHA.len(), 2);
        // Danbooru PRIMERO: su base es mucho mayor, así que al artista
        // corriente esto no le cuesta ni una petición de más.
        assert_eq!(BOORUS_FICHA[0], "danbooru.donmai.us");
        assert_eq!(BOORUS_FICHA[1], "aibooru.online");

        // Misma consulta, mismo motor: solo cambia el host.
        for h in BOORUS_FICHA {
            let u = url_ficha_artista_en(h, "wlop");
            assert!(u.starts_with(&format!("https://{h}/artists.json")), "{u}");
            assert!(u.contains("any_name_or_url_matches%5D=wlop"), "{u}");
            assert!(u.contains("only=name,urls"), "{u}");
        }
        // El envoltorio de siempre sigue apuntando a Danbooru.
                // Y escapa igual que antes.
        assert!(url_ficha_artista_en(BOORUS_FICHA[0], "a b").contains("a%20b"));
    }

    /// La base de artistas bebe de CUATRO sitios, y cada familia aporta algo
    /// que la otra no puede: los Moebooru dan el `source` —el enlace al perfil
    /// original— y los del motor de Danbooru dan el NOMBRE del artista, que es
    /// la llave de su ficha. AIBooru, además, es el único que cataloga obra de
    /// IA: los demás la rechazan por norma.
    #[test]
    fn la_cosecha_bebe_de_las_dos_familias() {
        assert_eq!(BOORUS_MOEBOORU, &["yande.re", "konachan.com"]);
        assert_eq!(BOORUS_DANBOORU.len(), 2);
        assert!(BOORUS_DANBOORU.iter().any(|(n, _)| *n == "AIBooru"), "los de IA");

        // Cada familia, su propia ruta. Confundirlas devuelve 404.
        for h in BOORUS_MOEBOORU {
            let u = url_cosecha_moebooru(h, "tohsaka_rin", 1);
            assert!(u.starts_with(&format!("https://{h}/post.json")), "{u}");
            assert!(u.contains("tags=tohsaka_rin&limit=100&page=1"), "{u}");
        }
        for (_, h) in BOORUS_DANBOORU {
            let u = url_cosecha_danbooru_en(h, "tohsaka_rin", 1);
            assert!(u.starts_with(&format!("https://{h}/posts.json")), "{u}");
        }
        // Moebooru usa `post.json`; el motor de Danbooru, `posts.json`.
        assert!(url_cosecha_moebooru("yande.re", "x", 1).contains("/post.json"));
        assert!(url_cosecha_danbooru_en("danbooru.donmai.us", "x", 1).contains("/posts.json"));

        // Ningún host repetido entre las dos familias.
        let todos: Vec<&str> = BOORUS_MOEBOORU
            .iter()
            .copied()
            .chain(BOORUS_DANBOORU.iter().map(|(_, h)| *h))
            .collect();
        let mut u = todos.clone();
        u.sort_unstable();
        u.dedup();
        assert_eq!(u.len(), todos.len(), "hosts repetidos: {todos:?}");

        // La ficha solo se pide a los que TIENEN base de artistas. Preguntar a
        // un Moebooru sería gastar una petición para nada.
        for h in BOORUS_FICHA {
            assert!(BOORUS_DANBOORU.iter().any(|(_, d)| d == h), "{h}");
            assert!(!BOORUS_MOEBOORU.contains(h), "{h} no tiene fichas");
        }
    }
}
