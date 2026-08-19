//! Buscador de X: construir la consulta y traducir un nombre de personaje a
//! las etiquetas con las que de verdad se publica.
//!
//! POR QUÉ ESTE MÓDULO NO PIDE NADA A X: gallery-dl le pasa la consulta a X
//! **tal cual**, sin interpretarla (`search_timeline(query)` en su extractor).
//! Así que la sintaxis completa de X está disponible y aquí no hace falta
//! escribir un motor de búsqueda: solo un constructor de consultas encima del
//! suyo, y un diccionario que resuelva cómo se llama cada personaje.
//!
//! EL DICCIONARIO ES EL VALOR REAL. El problema en X no es buscar, es saber qué
//! escribir: la mayoría del arte de Artoria está bajo `#アルトリア`, no bajo
//! `#Artoria`. Y `ゆきのん` —el apodo de Yukino en el fandom japonés— no lo
//! adivina ni quien sepa japonés. Esos nombres los publican los boorus en la
//! wiki de cada etiqueta, así que el diccionario ya existe y es de otros.

// APARCADO A PROPÓSITO, NO OLVIDADO.
//
// Este módulo está escrito y probado, pero todavía no lo llama nadie: falta
// comprobar si X responde a búsquedas con la sesión del usuario, y construir la
// interfaz encima de un supuesto sin verificar es el error que más caro ha
// salido en este proyecto. El `allow` documenta esa espera; **se quita en
// cuanto la pestaña de búsqueda lo use**, para que el aviso vuelva a servir
// para algo si alguna función se queda huérfana de verdad.
#![allow(dead_code)]

use serde_json::Value;

/// Lo que el usuario ha pedido, antes de convertirlo en sintaxis de X.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BusquedaX {
    /// Términos o hashtags. Varios se combinan con OR.
    pub terminos: Vec<String>,
    pub solo_imagenes: bool,
    pub solo_videos: bool,
    pub sin_retuits: bool,
    /// Mínimo de «me gusta». 0 = sin filtro.
    pub min_likes: u32,
    /// Código ISO del idioma, vacío = cualquiera.
    pub idioma: String,
}

/// Convierte la petición en la sintaxis de búsqueda de X.
///
/// Reglas que no son obvias:
///
/// - **Varios términos van entre paréntesis con OR.** Sin los paréntesis, X
///   aplica el resto de filtros solo al último: `#a OR #b filter:images`
///   significa «#a, o bien #b con imágenes», que no es lo que nadie quiere.
/// - **Imágenes y vídeos a la vez es `filter:media`**, no los dos filtros
///   juntos: `filter:images filter:videos` es una conjunción y no devuelve
///   nada, porque ningún post es las dos cosas.
/// - **Ninguno de los dos marcados no pone filtro**: se busca todo, incluido
///   texto suelto.
pub fn consulta(b: &BusquedaX) -> String {
    let mut partes: Vec<String> = Vec::new();

    let terminos: Vec<&str> = b
        .terminos
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    match terminos.len() {
        0 => {}
        1 => partes.push(terminos[0].to_string()),
        _ => partes.push(format!("({})", terminos.join(" OR "))),
    }

    match (b.solo_imagenes, b.solo_videos) {
        (true, true) => partes.push("filter:media".into()),
        (true, false) => partes.push("filter:images".into()),
        (false, true) => partes.push("filter:videos".into()),
        (false, false) => {}
    }
    if b.sin_retuits {
        partes.push("-filter:retweets".into());
    }
    if b.min_likes > 0 {
        partes.push(format!("min_faves:{}", b.min_likes));
    }
    let idioma = b.idioma.trim();
    if !idioma.is_empty() {
        partes.push(format!("lang:{idioma}"));
    }
    partes.join(" ")
}

/// URL lista para el listado, con la consulta codificada.
///
/// Codificación manual y no una dependencia: solo hay que escapar lo que X
/// interpretaría en la query, y añadir un crate para esto sería
/// desproporcionado. `#` es imprescindible —sin escapar cortaría la URL y
/// perdería el hashtag entero, que es justo lo que se busca.
pub fn url(b: &BusquedaX) -> String {
    let q = consulta(b);
    let mut out = String::from("https://x.com/search?q=");
    for c in q.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push_str("%20"),
            _ => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

/// Etiquetas de X sugeridas para un personaje, a partir de su ficha de booru.
///
/// Entra el JSON de `danbooru.donmai.us/wiki_pages.json?search[title]=…`, cuyo
/// campo `other_names` trae los nombres en japonés, chino y coreano, más las
/// romanizaciones alternativas. Probado contra la API real:
///
/// ```text
/// artoria_pendragon_(fate) → アルトリア・ペンドラゴン, アルトリア, 騎士王, …
/// yukinoshita_yukino       → 雪ノ下雪乃, ゆきのん, 八雪
/// ```
///
/// Se descartan los nombres con espacios: en X un hashtag no los admite, y
/// dejarlos produciría consultas rotas.
pub fn alias_de_wiki(json: &str, etiqueta: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    // El nombre de la etiqueta, sin el sufijo de obra entre paréntesis:
    // `artoria_pendragon_(fate)` → `ArtoriaPendragon`.
    let base = etiqueta
        .split('(')
        .next()
        .unwrap_or(etiqueta)
        .trim_matches(['_', ' '])
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<String>();
    if !base.is_empty() {
        out.push(base);
    }

    if let Ok(v) = serde_json::from_str::<Value>(json) {
        let paginas = v.as_array().cloned().unwrap_or_else(|| vec![v]);
        for p in paginas {
            let Some(nombres) = p.get("other_names").and_then(|n| n.as_array()) else {
                continue;
            };
            for n in nombres {
                let Some(s) = n.as_str() else { continue };
                let s = s.trim();
                // Sin espacios: un hashtag de X no los admite.
                if s.is_empty() || s.contains(char::is_whitespace) {
                    continue;
                }
                let s = s.to_string();
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> BusquedaX {
        BusquedaX {
            terminos: vec!["#Artoria".into()],
            sin_retuits: true,
            ..Default::default()
        }
    }

    #[test]
    fn un_termino_no_lleva_parentesis() {
        let b = base();
        assert_eq!(consulta(&b), "#Artoria -filter:retweets");
    }

    /// Sin los paréntesis, X aplicaría los filtros solo al último término.
    #[test]
    fn varios_terminos_van_agrupados_con_or() {
        let mut b = base();
        b.terminos = vec!["#アルトリア".into(), "#Artoria".into(), "#FGO".into()];
        b.solo_imagenes = true;
        b.min_likes = 200;
        assert_eq!(
            consulta(&b),
            "(#アルトリア OR #Artoria OR #FGO) filter:images -filter:retweets min_faves:200"
        );
    }

    /// `filter:images filter:videos` es una conjunción y no devuelve nada:
    /// ningún post es las dos cosas a la vez.
    #[test]
    fn imagenes_y_videos_a_la_vez_es_filter_media() {
        let mut b = base();
        b.solo_imagenes = true;
        b.solo_videos = true;
        assert!(consulta(&b).contains("filter:media"));
        assert!(!consulta(&b).contains("filter:images"));

        // Y ninguno marcado no pone filtro DE MEDIOS: se busca todo.
        //
        // Ojo con cómo se comprueba: `-filter:retweets` contiene «filter:», así
        // que buscar esa subcadena a secas daría un test que se contradice a sí
        // mismo. Hay que nombrar los tres filtros de medios.
        b.solo_imagenes = false;
        b.solo_videos = false;
        let q = consulta(&b);
        for f in ["filter:images", "filter:videos", "filter:media"] {
            assert!(!q.contains(f), "no debería filtrar por medios: {q}");
        }
        assert!(q.contains("-filter:retweets"), "pero el de retuits sí queda");
    }

    #[test]
    fn los_filtros_vacios_no_ensucian_la_consulta() {
        let b = BusquedaX {
            terminos: vec!["  ".into(), "#Rin".into(), "".into()],
            min_likes: 0,
            idioma: "  ".into(),
            ..Default::default()
        };
        assert_eq!(consulta(&b), "#Rin", "los términos vacíos se descartan");
    }

    /// Sin escapar, la almohadilla cortaría la URL y se perdería el hashtag
    /// entero — justo lo que se está buscando.
    #[test]
    fn la_url_escapa_almohadillas_y_japones() {
        let mut b = BusquedaX {
            terminos: vec!["#アルトリア".into()],
            ..Default::default()
        };
        let u = url(&b);
        assert!(u.starts_with("https://x.com/search?q="));
        assert!(!u.contains('#'), "la almohadilla debe ir escapada: {u}");
        assert!(u.contains("%23"));
        // アルトリア en UTF-8 percent-encoded
        assert!(u.contains("%E3%82%A2%E3%83%AB%E3%83%88%E3%83%AA%E3%82%A2"));

        b.terminos = vec!["#Rin".into()];
        b.solo_imagenes = true;
        assert_eq!(url(&b), "https://x.com/search?q=%23Rin%20filter%3Aimages");
    }

    /// Forma real devuelta por la API de Danbooru, verificada contra el sitio.
    #[test]
    fn el_diccionario_saca_los_nombres_japoneses() {
        let json = r#"[{"title":"yukinoshita_yukino",
                        "other_names":["雪ノ下雪乃","雪之下雪乃","ゆきのん","八雪"]}]"#;
        let a = alias_de_wiki(json, "yukinoshita_yukino");
        assert_eq!(a[0], "YukinoshitaYukino", "el nombre latino va primero");
        assert!(a.contains(&"ゆきのん".to_string()), "el apodo del fandom");
        assert!(a.contains(&"雪ノ下雪乃".to_string()));
    }

    #[test]
    fn el_sufijo_de_obra_no_entra_en_la_etiqueta() {
        let a = alias_de_wiki("[]", "artoria_pendragon_(fate)");
        assert_eq!(a, vec!["ArtoriaPendragon"], "sin el «(fate)»");
    }

    /// Un hashtag de X no admite espacios: dejarlos daría consultas rotas.
    #[test]
    fn los_nombres_con_espacios_se_descartan() {
        let json = r#"[{"other_names":["Saber Alter","セイバー","King of Knights"]}]"#;
        let a = alias_de_wiki(json, "saber_(fate)");
        assert!(a.contains(&"セイバー".to_string()));
        assert!(!a.iter().any(|s| s.contains(' ')));
    }

    #[test]
    fn un_json_roto_no_revienta_ni_pierde_el_nombre_base() {
        assert_eq!(alias_de_wiki("no es json", "tohsaka_rin"), vec!["TohsakaRin"]);
        assert_eq!(alias_de_wiki("{}", "tohsaka_rin"), vec!["TohsakaRin"]);
    }
}
