//! Listado de repositorios de Hugging Face: ver antes de descargar.
//!
//! Un modelo no es un archivo. `Qwen/Qwen3-32B` son diecisiete shards de casi
//! 4 GB cada uno más `config.json`, `tokenizer.json`, `vocab.json`,
//! `merges.txt` y `model.safetensors.index.json`. Copiar veintidós enlaces a
//! mano desde la pestaña «Files and versions» no es una forma razonable de
//! usar un gestor de descargas, y además invita al fallo clásico: bajarse los
//! 62 GB de pesos y olvidarse del index, con lo que no carga nada.
//!
//! Así que se lista primero, igual que con una galería. Hugging Face publica
//! el árbol del repositorio en una API abierta, sin necesidad de token:
//!
//! ```text
//! GET https://huggingface.co/api/models/Qwen/Qwen3-32B/tree/main?recursive=true
//! [{"type":"file","size":3957109648,"path":"model-00001-of-00017.safetensors"}, …]
//! ```
//!
//! Todo lo de este módulo es PURO: entra texto, sale estructura. Ni red ni
//! disco, para que se pueda probar entero sin tocar Hugging Face.

use serde_json::Value;

/// Un repositorio, sin el host: `Qwen/Qwen3-32B`.
///
/// Modelos y datasets viven en rutas de API distintas y en URLs de descarga
/// distintas, y esa es la única razón de que sean variantes separadas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repo {
    Modelo(String),
    Dataset(String),
}

impl Repo {
    fn id(&self) -> &str {
        match self {
            Repo::Modelo(s) | Repo::Dataset(s) => s,
        }
    }
}

/// Qué hay al otro lado de una URL de Hugging Face.
///
/// Se distingue con detalle a propósito: una colección y un repositorio se
/// parecen mucho en la barra de direcciones, y «no se pudo descargar» no le
/// dice a nadie que lo que ha pegado es un índice de cuatro modelos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destino {
    /// Un repositorio listable.
    Repo { repo: Repo, revision: String },
    /// `/blob/` o `/resolve/`: un archivo suelto. Ya se descarga tal cual.
    Archivo,
    /// `/collections/…`: un índice de varios modelos, no un repositorio.
    Coleccion,
    /// `/spaces/…`: una aplicación, no pesos.
    Espacio,
    /// La página de un usuario u organización, la portada, la documentación…
    Otro,
}

/// Rutas del propio sitio que nunca son un repositorio, por mucho que tengan
/// dos segmentos y se parezcan a `owner/nombre`.
const RESERVADOS: &[&str] = &[
    "collections", "spaces", "datasets", "models", "docs", "blog", "learn",
    "posts", "papers", "settings", "api", "join", "login", "logout", "pricing",
    "enterprise", "chat", "new", "notifications", "organizations", "tasks",
    "inference-endpoints", "autotrain", "search", "changelog", "terms-of-service",
    "privacy", "brand", "support", "hub",
];

/// Trocea el camino de una URL en segmentos no vacíos, sin query ni fragmento.
fn segmentos(url: &str) -> Vec<&str> {
    let sin_esquema = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let camino = sin_esquema.split_once('/').map(|(_, c)| c).unwrap_or("");
    camino
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// Clasifica una URL de Hugging Face. `None` si no es de Hugging Face.
///
/// El host lo comprueba quien llama (`host_matches`), porque este módulo no
/// debe duplicar esa lógica: `huggingface.co.atacante.example` no es
/// Hugging Face y esa decisión vive en un solo sitio.
pub fn clasificar_ruta(url: &str) -> Destino {
    let seg = segmentos(url);

    // Datasets: `/datasets/owner/nombre[/tree/rev]`
    if seg.first() == Some(&"datasets") {
        return match seg.len() {
            0..=2 => Destino::Otro, // la portada de datasets o un solo segmento
            _ => repo_o_archivo(Repo::Dataset(format!("{}/{}", seg[1], seg[2])), &seg[3..]),
        };
    }
    if seg.first() == Some(&"collections") {
        return Destino::Coleccion;
    }
    if seg.first() == Some(&"spaces") {
        return Destino::Espacio;
    }
    if seg.first().is_some_and(|s| RESERVADOS.contains(s)) {
        return Destino::Otro;
    }
    // Modelos: `/owner/nombre[/tree/rev]`. Un solo segmento es la página de un
    // usuario o de una organización.
    match seg.len() {
        0..=1 => Destino::Otro,
        _ => repo_o_archivo(Repo::Modelo(format!("{}/{}", seg[0], seg[1])), &seg[2..]),
    }
}

/// Lo que va después de `owner/nombre` decide si es el repo o un archivo.
fn repo_o_archivo(repo: Repo, resto: &[&str]) -> Destino {
    match resto.first() {
        // `/blob/rev/archivo` y `/resolve/rev/archivo` apuntan a UN archivo.
        Some(&"blob") | Some(&"resolve") => Destino::Archivo,
        Some(&"tree") => Destino::Repo {
            revision: resto.get(1).unwrap_or(&"main").to_string(),
            repo,
        },
        // `/discussions`, `/commits`… no son el árbol, pero el repo es ese y
        // listarlo es más útil que decir que no.
        _ => Destino::Repo { repo, revision: "main".into() },
    }
}

/// URL de la API que devuelve el árbol completo del repositorio.
pub fn url_api(repo: &Repo, revision: &str) -> String {
    let seccion = match repo {
        Repo::Modelo(_) => "models",
        Repo::Dataset(_) => "datasets",
    };
    format!(
        "https://huggingface.co/api/{seccion}/{}/tree/{}?recursive=true",
        repo.id(),
        revision
    )
}

/// URL que dice si el repositorio tiene licencia que aceptar.
///
/// Con `expand[]=gated` la respuesta son 66 bytes en vez de los ~15 KB del
/// objeto completo, que trae hasta la lista de Spaces que usan el modelo.
/// Preguntarlo cuesta una petición y evita encolar cincuenta gigas que van a
/// fallar uno a uno con un 403 sin explicación.
pub fn url_gated(repo: &Repo) -> String {
    let seccion = match repo {
        Repo::Modelo(_) => "models",
        Repo::Dataset(_) => "datasets",
    };
    format!("https://huggingface.co/api/{seccion}/{}?expand[]=gated", repo.id())
}

/// `Some(modo)` si hay que aceptar condiciones. `gated` viene como `false`, o
/// como `"auto"` (basta con pulsar el botón) o `"manual"` (lo aprueba una
/// persona, y puede tardar días).
pub fn parsear_gated(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    match v.get("gated")? {
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("auto".into()),
        _ => None,
    }
}

/// URL de descarga directa de un archivo del repositorio.
///
/// `/resolve/` y no `/blob/`: `/blob/` es la página HTML que lo enseña.
pub fn url_descarga(repo: &Repo, revision: &str, ruta: &str) -> String {
    let prefijo = match repo {
        Repo::Modelo(_) => String::new(),
        Repo::Dataset(_) => "datasets/".into(),
    };
    format!(
        "https://huggingface.co/{prefijo}{}/resolve/{revision}/{}",
        repo.id(),
        escapar(ruta)
    )
}

/// Escapa lo justo para que una ruta con espacios o almohadillas no rompa la
/// URL. No es un codificador general: las rutas de Hugging Face son nombres de
/// archivo, y sobrecodificar rompería las barras de los subdirectorios.
fn escapar(ruta: &str) -> String {
    let mut s = String::with_capacity(ruta.len());
    for c in ruta.chars() {
        match c {
            ' ' => s.push_str("%20"),
            '#' => s.push_str("%23"),
            '?' => s.push_str("%3F"),
            '%' => s.push_str("%25"),
            _ => s.push(c),
        }
    }
    s
}

/// De qué tipo es un archivo del repositorio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    /// Los pesos: lo que ocupa.
    Peso,
    /// Pequeño pero imprescindible. Sin `config.json`, `tokenizer.json` o el
    /// `model.safetensors.index.json`, sesenta gigas de pesos no los carga
    /// nadie. Es el error más repetido al bajar modelos a mano.
    Esencial,
    /// README, licencia, imágenes de la tarjeta del modelo. Prescindible.
    Extra,
}

const EXT_PESOS: &[&str] = &[
    "safetensors", "gguf", "bin", "pt", "pth", "ckpt", "onnx", "msgpack", "h5", "npz",
];

/// Extensión en minúsculas de una ruta, sin el punto.
fn extension(ruta: &str) -> String {
    ruta.rsplit('/')
        .next()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn clase_de(ruta: &str) -> Clase {
    let nombre = ruta.rsplit('/').next().unwrap_or(ruta);
    let ext = extension(ruta);

    if EXT_PESOS.contains(&ext.as_str()) {
        return Clase::Peso;
    }
    // Lista por exclusión y no por inclusión, a propósito. Cada arquitectura
    // inventa sus propios archivos de configuración —`preprocessor_config`,
    // `chat_template.jinja`, el `modeling_*.py` que exige `trust_remote_code`—
    // y una lista blanca se quedaría corta cada dos meses, dejando fuera algo
    // imprescindible. Aquí lo prescindible es lo que hay que enumerar, que es
    // corto y no cambia.
    let prescindible = matches!(
        ext.as_str(),
        "md" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ipynb" | "pdf"
    ) || nombre.eq_ignore_ascii_case(".gitattributes")
        || nombre.eq_ignore_ascii_case("LICENSE")
        || nombre.to_ascii_uppercase().starts_with("LICENSE.");

    if prescindible {
        Clase::Extra
    } else {
        Clase::Esencial
    }
}

/// Un archivo del repositorio, ya clasificado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archivo {
    pub ruta: String,
    pub bytes: u64,
    pub clase: Clase,
    /// Etiqueta de cuantización si es un GGUF que la lleva («Q4_K_M»).
    pub cuant: Option<String>,
    /// Marcado de salida para la rejilla.
    pub marcado: bool,
}

/// Algo que conviene mirar antes de darle a descargar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Aviso {
    /// El repositorio trae los MISMOS pesos en `.bin` y en `.safetensors`.
    /// Bajar los dos es duplicar decenas de gigas para nada.
    FormatoDuplicado { safetensors: usize, bin: usize },
    /// Varias cuantizaciones GGUF. Son alternativas: se elige UNA.
    Cuantizaciones(Vec<String>),
    /// El repositorio se puede LISTAR pero no descargar sin aceptar antes sus
    /// condiciones en la web. Sin esto, las cincuenta filas de la cola
    /// fallarían una a una con un 403 que no menciona la licencia.
    ConLicencia(String),
}

/// Resultado de listar un repositorio.
#[derive(Clone, Debug, Default)]
pub struct Listado {
    pub archivos: Vec<Archivo>,
    pub avisos: Vec<Aviso>,
}

impl Listado {
    /// Archivos marcados y lo que ocupan.
    ///
    /// Se enseña ANTES de descargar por una razón muy concreta: un modelo de
    /// 27B son sesenta y tantos gigas, y eso no se deduce de una lista de
    /// veintidós nombres. Saberlo antes de darle al botón es la diferencia
    /// entre elegir y enterarte a mitad de la noche.
    pub fn marcado(&self) -> (usize, u64) {
        self.archivos
            .iter()
            .filter(|a| a.marcado)
            .fold((0, 0), |(n, b), a| (n + 1, b + a.bytes))
    }

    /// Lo mismo para el repositorio entero.
    pub fn total(&self) -> (usize, u64) {
        (self.archivos.len(), self.archivos.iter().map(|a| a.bytes).sum())
    }
}

/// Etiqueta de cuantización de un GGUF, si la lleva.
///
/// Se parte por `-` y `.` pero NO por `_`: la etiqueta es `Q4_K_M` entera, no
/// tres trozos. Se reconocen las dos familias que usa llama.cpp: `Q…`/`IQ…` y
/// los formatos en coma flotante.
fn cuantizacion(ruta: &str) -> Option<String> {
    if extension(ruta) != "gguf" {
        return None;
    }
    let nombre = ruta.rsplit('/').next().unwrap_or(ruta);
    nombre
        .split(['-', '.'])
        .map(|t| t.to_ascii_uppercase())
        .find(|t| {
            matches!(t.as_str(), "F16" | "F32" | "BF16" | "FP16" | "FP32" | "FP8")
                || t.strip_prefix('I')
                    .unwrap_or(t)
                    .strip_prefix('Q')
                    .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        })
}

/// Parsea el árbol devuelto por la API y decide qué viene marcado.
///
/// Marcado por defecto: los pesos y lo esencial, o sea el modelo listo para
/// usar. Fuera el README, la licencia y las imágenes de la tarjeta.
///
/// SOBRE LAS VARIANTES: solo se detectan las dos que son objetivamente
/// alternativas y no admiten discusión —el mismo peso en `.bin` y en
/// `.safetensors`, y varias cuantizaciones GGUF—. Deliberadamente NO se trata
/// como variante un subdirectorio: en un modelo de difusión `transformer/`,
/// `text_encoder/` y `vae/` son COMPONENTES y hacen falta todos. Confundir un
/// componente con una alternativa dejaría al usuario con un modelo a medias, y
/// eso es peor que no avisar de nada.
pub fn parsear_arbol(json: &str) -> Result<Listado, String> {
    let raiz: Value = serde_json::from_str(json).map_err(|e| format!("JSON ilegible: {e}"))?;

    // La API devuelve `{"error": "..."}` con 401/404 en vez de una lista.
    if let Some(msg) = raiz.get("error").and_then(Value::as_str) {
        return Err(msg.to_string());
    }
    let Some(lista) = raiz.as_array() else {
        return Err("la API no devolvió una lista de archivos".into());
    };

    let mut archivos: Vec<Archivo> = lista
        .iter()
        .filter(|e| e.get("type").and_then(Value::as_str) != Some("directory"))
        .filter_map(|e| {
            let ruta = e.get("path").and_then(Value::as_str)?.to_string();
            if ruta.is_empty() {
                return None;
            }
            // El tamaño real de un archivo LFS está anidado; el de fuera es el
            // del puntero, unos 135 bytes. Sin esto, un shard de 4 GB
            // aparecería en la lista como si pesara nada.
            let bytes = e
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(Value::as_u64)
                .or_else(|| e.get("size").and_then(Value::as_u64))
                .unwrap_or(0);
            Some(Archivo {
                clase: clase_de(&ruta),
                cuant: cuantizacion(&ruta),
                bytes,
                ruta,
                marcado: false,
            })
        })
        .collect();

    if archivos.is_empty() {
        return Err("el repositorio no tiene archivos".into());
    }
    archivos.sort_by(|a, b| a.ruta.cmp(&b.ruta));

    let mut avisos = Vec::new();

    // ¿Los mismos pesos en dos formatos?
    let safet = archivos.iter().filter(|a| extension(&a.ruta) == "safetensors").count();
    let bins = archivos.iter().filter(|a| extension(&a.ruta) == "bin").count();
    let duplicado = safet > 0 && bins > 0;
    if duplicado {
        avisos.push(Aviso::FormatoDuplicado { safetensors: safet, bin: bins });
    }

    // ¿Varias cuantizaciones GGUF?
    let mut cuants: Vec<String> = archivos.iter().filter_map(|a| a.cuant.clone()).collect();
    cuants.sort();
    cuants.dedup();
    let varias_cuants = cuants.len() > 1;
    if varias_cuants {
        avisos.push(Aviso::Cuantizaciones(cuants));
    }

    for a in &mut archivos {
        a.marcado = match a.clase {
            Clase::Extra => false,
            Clase::Esencial => true,
            // Con alternativas sobre la mesa no se elige por el usuario: se
            // deja sin marcar y se avisa. Marcar la que nos parezca mejor
            // podría costarle sesenta gigas de descarga equivocada.
            Clase::Peso if varias_cuants && a.cuant.is_some() => false,
            Clase::Peso if duplicado && extension(&a.ruta) == "bin" => false,
            Clase::Peso => true,
        };
    }

    Ok(Listado { archivos, avisos })
}

/// Nombre con el que se guarda cada archivo, en el mismo orden que la lista.
///
/// El nombre a secas, que es el que espera quien va a cargarlo: ComfyUI busca
/// `hunyuanimage2.1_refiner_fp8_e4m3fn.safetensors` dentro de su propia
/// carpeta `models/diffusion_models/`, y no reconoce nada más.
///
/// Solo se antepone la carpeta cuando dos archivos del repositorio se llaman
/// igual, y pasa de verdad: un modelo de difusión tiene un `config.json` en
/// `transformer/`, otro en `text_encoder/` y otro en `vae/`. Anteponerla a
/// TODOS para cubrir ese caso ensuciaría los nombres de los repositorios
/// normales, que son la mayoría.
pub fn nombres(archivos: &[Archivo]) -> Vec<String> {
    use std::collections::HashMap;
    let base = |r: &str| r.rsplit('/').next().unwrap_or(r).to_string();

    let mut cuantos: HashMap<String, usize> = HashMap::new();
    for a in archivos {
        *cuantos.entry(base(&a.ruta)).or_default() += 1;
    }

    archivos
        .iter()
        .map(|a| {
            let b = base(&a.ruta);
            if cuantos.get(&b).copied().unwrap_or(0) > 1 {
                // La carpeta que lo contiene, no la ruta entera: `vae` basta
                // para distinguirlo de `transformer`.
                match a.ruta.rsplit('/').nth(1) {
                    Some(padre) => format!("{padre}_{b}"),
                    None => b,
                }
            } else {
                b
            }
        })
        .collect()
}

/// Tamaño legible: «3.9 GB».
pub fn tam_legible(bytes: u64) -> String {
    const U: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_distingue_un_repo_de_lo_que_no_lo_es() {
        assert_eq!(
            clasificar_ruta("https://huggingface.co/Qwen/Qwen3.8-27B-FP8"),
            Destino::Repo { repo: Repo::Modelo("Qwen/Qwen3.8-27B-FP8".into()), revision: "main".into() }
        );
        assert_eq!(
            clasificar_ruta("https://huggingface.co/Qwen/Qwen3-32B/tree/refs%2Fpr%2F1"),
            Destino::Repo { repo: Repo::Modelo("Qwen/Qwen3-32B".into()), revision: "refs%2Fpr%2F1".into() }
        );
        assert_eq!(
            clasificar_ruta("https://huggingface.co/datasets/openai/gsm8k"),
            Destino::Repo { repo: Repo::Dataset("openai/gsm8k".into()), revision: "main".into() }
        );

        // Un archivo suelto NO es un listado: ya se descarga por su cuenta.
        assert_eq!(
            clasificar_ruta("https://huggingface.co/Qwen/X/blob/main/model.safetensors"),
            Destino::Archivo
        );
        assert_eq!(
            clasificar_ruta("https://huggingface.co/Qwen/X/resolve/main/model.safetensors"),
            Destino::Archivo
        );

        // Lo que se parece a un repo en la barra de direcciones y no lo es.
        assert_eq!(clasificar_ruta("https://huggingface.co/collections/Qwen/qwen38"), Destino::Coleccion);
        assert_eq!(clasificar_ruta("https://huggingface.co/spaces/foo/bar"), Destino::Espacio);
        assert_eq!(clasificar_ruta("https://huggingface.co/Qwen"), Destino::Otro);
        assert_eq!(clasificar_ruta("https://huggingface.co/"), Destino::Otro);
        assert_eq!(clasificar_ruta("https://huggingface.co/models?sort=trending"), Destino::Otro);
        assert_eq!(clasificar_ruta("https://huggingface.co/docs/hub/index"), Destino::Otro);
    }

    #[test]
    fn las_urls_de_api_y_descarga_son_las_correctas() {
        let m = Repo::Modelo("Qwen/Qwen3-32B".into());
        assert_eq!(
            url_api(&m, "main"),
            "https://huggingface.co/api/models/Qwen/Qwen3-32B/tree/main?recursive=true"
        );
        assert_eq!(
            url_descarga(&m, "main", "model-00001-of-00017.safetensors"),
            "https://huggingface.co/Qwen/Qwen3-32B/resolve/main/model-00001-of-00017.safetensors"
        );
        // Los datasets llevan su prefijo en las DOS URLs.
        let d = Repo::Dataset("openai/gsm8k".into());
        assert_eq!(
            url_api(&d, "main"),
            "https://huggingface.co/api/datasets/openai/gsm8k/tree/main?recursive=true"
        );
        assert_eq!(
            url_descarga(&d, "main", "main/train.parquet"),
            "https://huggingface.co/datasets/openai/gsm8k/resolve/main/main/train.parquet"
        );
        // Las barras de los subdirectorios NO se escapan; los espacios sí.
        assert_eq!(
            url_descarga(&m, "main", "sub dir/a b.bin"),
            "https://huggingface.co/Qwen/Qwen3-32B/resolve/main/sub%20dir/a%20b.bin"
        );
    }

    #[test]
    fn lo_pequeno_e_imprescindible_no_se_confunde_con_lo_prescindible() {
        for r in ["model-00001-of-00017.safetensors", "pytorch_model.bin", "m.gguf", "w.onnx"] {
            assert_eq!(clase_de(r), Clase::Peso, "{r}");
        }
        for r in [
            "config.json",
            "tokenizer.json",
            "merges.txt",
            "vocab.json",
            "model.safetensors.index.json",
            "chat_template.jinja",
            "modeling_qwen.py",
            "tokenizer.model",
            "transformer/config.json",
        ] {
            assert_eq!(clase_de(r), Clase::Esencial, "{r}");
        }
        for r in ["README.md", "LICENSE", "LICENSE.txt", ".gitattributes", "assets/demo.png"] {
            assert_eq!(clase_de(r), Clase::Extra, "{r}");
        }
    }

    /// El tamaño de un archivo LFS va anidado. Cogiendo el de fuera, un shard
    /// de 4 GB aparecería en la lista como si pesara 135 bytes.
    #[test]
    fn se_lee_el_tamano_real_de_los_archivos_lfs() {
        let j = r#"[
            {"type":"file","size":728,"path":"config.json"},
            {"type":"file","size":135,"lfs":{"size":3957109648},"path":"model-00001-of-00002.safetensors"},
            {"type":"file","size":135,"lfs":{"size":3055341992},"path":"model-00002-of-00002.safetensors"},
            {"type":"file","size":58330,"path":"model.safetensors.index.json"},
            {"type":"file","size":16636,"path":"README.md"}
        ]"#;
        let l = parsear_arbol(j).unwrap();
        assert_eq!(l.archivos.len(), 5);
        let peso = l.archivos.iter().find(|a| a.ruta.starts_with("model-00001")).unwrap();
        assert_eq!(peso.bytes, 3_957_109_648);

        // Preselección: el modelo completo, sin el README.
        let marcados: Vec<&str> =
            l.archivos.iter().filter(|a| a.marcado).map(|a| a.ruta.as_str()).collect();
        assert_eq!(
            marcados,
            [
                "config.json",
                "model-00001-of-00002.safetensors",
                "model-00002-of-00002.safetensors",
                "model.safetensors.index.json"
            ]
        );
        assert!(l.avisos.is_empty());
        assert_eq!(l.marcado(), (4, 728 + 3_957_109_648 + 3_055_341_992 + 58_330));
        assert_eq!(l.total(), (5, 728 + 3_957_109_648 + 3_055_341_992 + 58_330 + 16_636));
    }

    #[test]
    fn los_mismos_pesos_en_dos_formatos_se_avisan_y_no_se_duplican() {
        let j = r#"[
            {"type":"file","size":728,"path":"config.json"},
            {"type":"file","lfs":{"size":1000},"path":"model.safetensors"},
            {"type":"file","lfs":{"size":1000},"path":"pytorch_model.bin"}
        ]"#;
        let l = parsear_arbol(j).unwrap();
        assert_eq!(l.avisos, vec![Aviso::FormatoDuplicado { safetensors: 1, bin: 1 }]);
        let m = |r: &str| l.archivos.iter().find(|a| a.ruta == r).unwrap().marcado;
        assert!(m("model.safetensors"));
        assert!(!m("pytorch_model.bin"), "no se baja el mismo peso dos veces");
    }

    #[test]
    fn varias_cuantizaciones_no_se_eligen_por_el_usuario() {
        let j = r#"[
            {"type":"file","size":700,"path":"config.json"},
            {"type":"file","lfs":{"size":10},"path":"Qwen3-32B-Q4_K_M.gguf"},
            {"type":"file","lfs":{"size":20},"path":"Qwen3-32B-Q8_0.gguf"},
            {"type":"file","lfs":{"size":30},"path":"Qwen3-32B-F16-00001-of-00003.gguf"}
        ]"#;
        let l = parsear_arbol(j).unwrap();
        assert_eq!(
            l.avisos,
            vec![Aviso::Cuantizaciones(vec!["F16".into(), "Q4_K_M".into(), "Q8_0".into()])]
        );
        // Ninguna marcada: elegir por él podría costarle la descarga entera.
        assert!(l.archivos.iter().filter(|a| a.cuant.is_some()).all(|a| !a.marcado));
        // Pero la configuración sí, que no es una alternativa.
        assert!(l.archivos.iter().find(|a| a.ruta == "config.json").unwrap().marcado);

        // Con UNA sola cuantización no hay nada que elegir: se marca.
        let j1 = r#"[{"type":"file","lfs":{"size":10},"path":"m-Q4_K_M.gguf"}]"#;
        let l1 = parsear_arbol(j1).unwrap();
        assert!(l1.avisos.is_empty());
        assert!(l1.archivos[0].marcado);
    }

    /// Los subdirectorios de un modelo de difusión son COMPONENTES, no
    /// alternativas. Tratarlos como variantes dejaría el modelo a medias.
    #[test]
    fn los_componentes_de_un_modelo_de_difusion_van_todos() {
        let j = r#"[
            {"type":"file","size":700,"path":"model_index.json"},
            {"type":"file","lfs":{"size":10},"path":"transformer/diffusion_pytorch_model.safetensors"},
            {"type":"file","lfs":{"size":20},"path":"text_encoder/model.safetensors"},
            {"type":"file","lfs":{"size":30},"path":"vae/diffusion_pytorch_model.safetensors"},
            {"type":"file","size":9,"path":"README.md"}
        ]"#;
        let l = parsear_arbol(j).unwrap();
        assert!(l.avisos.is_empty());
        assert_eq!(l.archivos.iter().filter(|a| a.marcado).count(), 4);
        assert!(!l.archivos.iter().find(|a| a.ruta == "README.md").unwrap().marcado);
    }

    #[test]
    fn los_errores_de_la_api_se_propagan_en_vez_de_dar_una_lista_vacia() {
        assert!(parsear_arbol(r#"{"error":"Repository not found"}"#)
            .unwrap_err()
            .contains("not found"));
        assert!(parsear_arbol("no soy json").is_err());
        assert!(parsear_arbol("[]").is_err());
        assert!(parsear_arbol(r#"{"algo":1}"#).is_err());
        // Los directorios no cuentan como archivos.
        assert!(parsear_arbol(r#"[{"type":"directory","path":"sub"}]"#).is_err());
    }


    /// Un repositorio con licencia se LISTA pero no se descarga. Detectarlo
    /// antes de encolar cincuenta gigas es la diferencia entre un aviso y
    /// cincuenta filas en rojo con un 403 que no menciona la licencia.
    #[test]
    fn se_detecta_si_hay_condiciones_que_aceptar() {
        assert_eq!(
            parsear_gated(r#"{"id":"a/b","gated":"auto"}"#).as_deref(),
            Some("auto")
        );
        assert_eq!(
            parsear_gated(r#"{"id":"a/b","gated":"manual"}"#).as_deref(),
            Some("manual")
        );
        assert_eq!(parsear_gated(r#"{"id":"a/b","gated":true}"#).as_deref(), Some("auto"));
        // Lo normal: nada que aceptar, ningún aviso.
        assert!(parsear_gated(r#"{"id":"a/b","gated":false}"#).is_none());
        // Y si la respuesta no sirve, tampoco se inventa un aviso.
        assert!(parsear_gated(r#"{"id":"a/b"}"#).is_none());
        assert!(parsear_gated("no soy json").is_none());

        assert_eq!(
            url_gated(&Repo::Modelo("Qwen/Qwen3-32B".into())),
            "https://huggingface.co/api/models/Qwen/Qwen3-32B?expand[]=gated"
        );
        assert_eq!(
            url_gated(&Repo::Dataset("openai/gsm8k".into())),
            "https://huggingface.co/api/datasets/openai/gsm8k?expand[]=gated"
        );
    }


    /// El nombre que se guarda es el que espera quien va a cargar el archivo.
    /// La ruta aplanada del repositorio, combinada además con el autor y
    /// recortada a 110 caracteres, producía esto:
    ///
    /// ```text
    /// Comfy-OrgHunyuanImage_2.1_ComfyUI_split_filesdiffusion_models…_sp.safetensors
    /// ```
    ///
    /// que ComfyUI no reconoce.
    #[test]
    fn el_nombre_guardado_es_el_del_archivo() {
        let mk = |r: &str| Archivo {
            ruta: r.into(),
            bytes: 1,
            clase: clase_de(r),
            cuant: None,
            marcado: true,
        };
        // Repositorio normal: el nombre a secas, sin rastro de la carpeta.
        let a = vec![
            mk("split_files/diffusion_models/hunyuanimage2.1_refiner_fp8_e4m3fn.safetensors"),
            mk("split_files/text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors"),
            mk("split_files/vae/hunyuan_image_2.1_vae_fp16.safetensors"),
            mk("README.md"),
        ];
        assert_eq!(
            nombres(&a),
            [
                "hunyuanimage2.1_refiner_fp8_e4m3fn.safetensors",
                "qwen_2.5_vl_7b_fp8_scaled.safetensors",
                "hunyuan_image_2.1_vae_fp16.safetensors",
                "README.md",
            ]
        );

        // Modelo de difusión: tres `config.json` que se pisarían entre ellos.
        // Solo esos llevan carpeta delante; el que es único, no.
        let b = vec![
            mk("model_index.json"),
            mk("transformer/config.json"),
            mk("text_encoder/config.json"),
            mk("vae/config.json"),
            mk("vae/diffusion_pytorch_model.safetensors"),
        ];
        assert_eq!(
            nombres(&b),
            [
                "model_index.json",
                "transformer_config.json",
                "text_encoder_config.json",
                "vae_config.json",
                "diffusion_pytorch_model.safetensors",
            ]
        );

        // Sin carpeta y repetido: no hay padre que anteponer, y no se rompe.
        assert_eq!(nombres(&[mk("a.bin"), mk("a.bin")]), ["a.bin", "a.bin"]);
        assert!(nombres(&[]).is_empty());
    }

    #[test]
    fn el_tamano_se_lee_como_lo_leeria_una_persona() {
        assert_eq!(tam_legible(0), "0 B");
        assert_eq!(tam_legible(728), "728 B");
        assert_eq!(tam_legible(1536), "1.5 KB");
        assert_eq!(tam_legible(3_957_109_648), "3.7 GB");
    }
}
