//! Lectura nativa de las cookies del navegador.
//!
//! POR QUÉ SOLO FIREFOX: guarda sus cookies en un SQLite **sin cifrar**, y en
//! los tres sistemas operativos por igual.
//!
//! Los derivados de Chromium cifran el valor, y con qué depende del sistema:
//! en Windows con DPAPI y, desde Chrome 127, con App-Bound Encryption, que
//! además exige que la lectura venga del propio navegador; en macOS con el
//! Llavero y en Linux con gnome-keyring o kwallet, que sí son accesibles con
//! permiso del usuario. O sea que el caso imposible es Windows, no Chromium
//! entero — un matiz que el aviso de la interfaz daba mal.
//!
//! Aun así este módulo se queda en Firefox: cubrir tres almacenes de claves
//! distintos para ahorrar una exportación no compensa, y `cookies.txt` sirve
//! para todos.
//!
//! POR QUÉ HACE FALTA: yt-dlp y gallery-dl saben leer el navegador solos, pero
//! son procesos aparte. Los motores nativos —V2PH, y cualquiera que venga
//! después— hacen sus propias peticiones HTTP y no pueden aprovechar aquello.
//! Sin esto, un motor nativo obliga al usuario a exportar un archivo a mano.
//!
//! QUÉ NO HACE: no descifra nada, no pide contraseñas y no guarda copias. Lee,
//! filtra por dominio y devuelve una cabecera `Cookie`. Las cookies de otros
//! sitios ni se miran.

use std::path::{Path, PathBuf};

/// Ubicaciones donde Firefox guarda sus perfiles, por sistema operativo.
///
/// En Linux se contemplan además las instalaciones de Snap y Flatpak, que
/// mueven el directorio y son hoy el reparto por defecto en Ubuntu y Fedora.
fn raices_firefox() -> Vec<PathBuf> {
    let mut v = Vec::new();

    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            v.push(PathBuf::from(appdata).join("Mozilla").join("Firefox"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            v.push(
                PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("Firefox"),
            );
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let h = PathBuf::from(home);
            v.push(h.join(".mozilla").join("firefox"));
            // Snap (Ubuntu) y Flatpak: mismo contenido, otra ruta
            v.push(h.join("snap").join("firefox").join("common").join(".mozilla").join("firefox"));
            v.push(
                h.join(".var")
                    .join("app")
                    .join("org.mozilla.firefox")
                    .join(".mozilla")
                    .join("firefox"),
            );
        }
    }

    v
}

/// Perfil marcado como predeterminado en `profiles.ini`, y si su ruta es
/// relativa a la raíz o absoluta.
///
/// Firefox admite varios perfiles y el nombre de carpeta no basta para saber
/// cuál se usa: la sección `[Install…]` apunta al que abre el navegador al
/// arrancar. Se lee esa clave `Default=`, que siempre es relativa.
///
/// Se acepta también la de `[ProfileN]` con `Default=1`, que es la forma
/// antigua y sigue apareciendo en instalaciones veteranas. Esas secciones
/// llevan además `IsRelative`: con `0` la ruta es ABSOLUTA, porque el perfil
/// se ha movido a otro disco. Concatenarla con la raíz produciría un disparate.
pub fn perfil_predeterminado(ini: &str) -> Option<(String, bool)> {
    let mut seccion = String::new();
    let mut candidato_install: Option<(String, bool)> = None;
    let mut candidato_profile: Option<(String, bool)> = None;
    let mut ruta_actual: Option<String> = None;
    let mut relativa = true;
    let mut es_default = false;

    fn cerrar(
        ruta: &Option<String>,
        relativa: bool,
        def: bool,
        dst: &mut Option<(String, bool)>,
    ) {
        if def && dst.is_none() {
            if let Some(r) = ruta {
                *dst = Some((r.clone(), relativa));
            }
        }
    }

    for linea in ini.lines() {
        let l = linea.trim();
        if l.starts_with('[') {
            // Cerrar la sección anterior antes de cambiar
            if seccion.starts_with("Profile") {
                cerrar(&ruta_actual, relativa, es_default, &mut candidato_profile);
            }
            seccion = l.trim_matches(['[', ']']).to_string();
            ruta_actual = None;
            relativa = true;
            es_default = false;
            continue;
        }
        let Some((clave, valor)) = l.split_once('=') else { continue };
        let (clave, valor) = (clave.trim(), valor.trim());

        if seccion.starts_with("Install") && clave == "Default" && !valor.is_empty() {
            // La sección Install manda: es el perfil que Firefox abre de verdad
            if candidato_install.is_none() {
                candidato_install = Some((valor.to_string(), true));
            }
        } else if seccion.starts_with("Profile") {
            match clave {
                "Path" => ruta_actual = Some(valor.to_string()),
                "IsRelative" => relativa = valor != "0",
                "Default" => es_default = valor == "1",
                _ => {}
            }
        }
    }
    if seccion.starts_with("Profile") {
        cerrar(&ruta_actual, relativa, es_default, &mut candidato_profile);
    }

    candidato_install.or(candidato_profile)
}

/// Perfiles con `cookies.sqlite` dentro de un directorio, el más reciente
/// primero: el perfil usado hace menos es el que más probablemente tiene sesión.
fn escanear(dir: &Path) -> Vec<PathBuf> {
    let Ok(entradas) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = entradas
        .flatten()
        .map(|e| e.path().join("cookies.sqlite"))
        .filter(|p| p.is_file())
        .map(|p| {
            let t = p
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (t, p)
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// Todas las rutas a `cookies.sqlite` que se encuentren, la más probable primero.
fn bases_de_cookies() -> Vec<PathBuf> {
    let mut fuera: Vec<PathBuf> = Vec::new();

    for raiz in raices_firefox() {
        // 1) El perfil que declara `profiles.ini`
        if let Ok(ini) = std::fs::read_to_string(raiz.join("profiles.ini")) {
            if let Some((rel, relativa)) = perfil_predeterminado(&ini) {
                let ruta = rel.replace('/', std::path::MAIN_SEPARATOR_STR);
                let dir = if relativa {
                    raiz.join(ruta)
                } else {
                    // IsRelative=0: el perfil vive fuera de la raíz
                    PathBuf::from(ruta)
                };
                let p = dir.join("cookies.sqlite");
                if p.is_file() {
                    fuera.push(p);
                }
            }
        }
        // 2) Red de seguridad, por si el `.ini` falta o miente.
        //
        //    LAS DOS RUTAS SON NECESARIAS: Windows y macOS meten los perfiles
        //    en una subcarpeta `Profiles/`, pero en Linux cuelgan DIRECTAMENTE
        //    de la raíz. Escanear solo `Profiles/` dejaba a Linux sin plan B.
        fuera.extend(escanear(&raiz.join("Profiles")));
        fuera.extend(escanear(&raiz));
    }

    fuera.dedup();
    fuera
}

/// Copia la base a un temporal antes de abrirla.
///
/// NO ES OPCIONAL: con Firefox abierto, `cookies.sqlite` está bloqueado y en
/// modo WAL. Abrirla en su sitio devuelve «database is locked» o, peor, deja
/// archivos `-shm` sueltos en el perfil del usuario. Se copia también el `-wal`
/// porque las cookies de la sesión actual pueden vivir solo ahí: sin él, la
/// sesión recién iniciada no aparecería.
fn copia_temporal(origen: &Path) -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir().join(format!(
        "td-cookies-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::copy(origen, &base)?;

    let wal = origen.with_extension("sqlite-wal");
    if wal.is_file() {
        let _ = std::fs::copy(&wal, base.with_extension("sqlite-wal"));
    }
    Ok(base)
}

fn limpiar(tmp: &Path) {
    let _ = std::fs::remove_file(tmp);
    let _ = std::fs::remove_file(tmp.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("sqlite-shm"));
}

/// ¿El host de una cookie corresponde a este dominio o a un subdominio suyo?
///
/// Comparación estructural, no `contains`: `v2ph.com.atacante.net` no es
/// `v2ph.com`, y mandarle nuestra sesión sería regalarla.
pub fn host_coincide(host_cookie: &str, dominio: &str) -> bool {
    let h = host_cookie.trim_start_matches('.').to_ascii_lowercase();
    let d = dominio.trim_start_matches('.').to_ascii_lowercase();
    h == d || h.ends_with(&format!(".{d}"))
}

/// Resultado de buscar cookies, con el rastro de lo que se intentó.
///
/// LA TRAZA NO ES UN LUJO: sin ella, «no hay cookies» puede significar que no
/// hay Firefox, que el perfil no se encontró, que la base no se pudo abrir o
/// que sencillamente no hay sesión de ese sitio. Son cuatro problemas con
/// cuatro soluciones distintas, y adivinar cuál es cuesta una tarde.
pub struct Hallazgo {
    pub cookie: Option<String>,
    pub traza: String,
}

/// Cabecera `Cookie` con las cookies de Firefox para un dominio, y el rastro.
pub fn firefox_cookie_header_diag(dominio: &str) -> Hallazgo {
    let bases = bases_de_cookies();
    let mut lineas: Vec<String> = Vec::new();

    if bases.is_empty() {
        let raices: Vec<String> = raices_firefox()
            .iter()
            .map(|r| format!("{} ({})", r.display(), if r.is_dir() { "existe" } else { "no existe" }))
            .collect();
        return Hallazgo {
            cookie: None,
            traza: format!(
                "Firefox: no se encontró ningún cookies.sqlite.\nRaíces buscadas:\n  {}",
                raices.join("\n  ")
            ),
        };
    }

    for base in &bases {
        // Solo se muestra el nombre del perfil, no la ruta completa: no hace
        // falta enseñar el nombre de usuario del sistema en un diagnóstico.
        let etiqueta = base
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".into());

        let tmp = match copia_temporal(base) {
            Ok(t) => t,
            Err(e) => {
                lineas.push(format!("{etiqueta}: no se pudo copiar ({e})"));
                continue;
            }
        };
        let resultado = leer_cookies(&tmp, dominio);
        limpiar(&tmp);

        match resultado {
            Ok(Some((h, nombres))) => {
                lineas.push(format!(
                    "{etiqueta}: {} cookie(s) de {dominio} → {}",
                    nombres.len(),
                    nombres.join(", ")
                ));
                lineas.push(
                    "NOTA: Firefox guarda en cookies.sqlite solo las cookies CON caducidad. \
                     Las de sesión (las que caducan al cerrar el navegador) viven en memoria \
                     y NO están en ese archivo, así que no se pueden leer desde fuera."
                        .into(),
                );
                return Hallazgo { cookie: Some(h), traza: lineas.join("\n") };
            }
            Ok(None) => lineas.push(format!("{etiqueta}: sin cookies de {dominio}")),
            Err(e) => lineas.push(format!("{etiqueta}: error al leer ({e})")),
        }
    }

    Hallazgo {
        cookie: None,
        traza: format!("Firefox, {} perfil(es) revisados:\n  {}", bases.len(), lineas.join("\n  ")),
    }
}

/// Consulta la tabla `moz_cookies`. Aislada para que el resto del módulo no
/// dependa de rusqlite y se pueda probar por separado.
///
/// El error de SQLite se propaga en vez de tragarse: una base bloqueada, un
/// esquema cambiado y una tabla vacía son cosas distintas.
fn leer_cookies(db: &Path, dominio: &str) -> Result<Option<(String, Vec<String>)>, String> {
    use rusqlite::{Connection, OpenFlags};

    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;

    // `host` existe en todas las versiones de Firefox que importan. No se
    // filtra por dominio en SQL: hacerlo con LIKE '%dominio' aceptaría
    // «malv2ph.com». El filtro va en Rust, con comparación estructural.
    let mut stmt = conn
        .prepare("SELECT host, name, value FROM moz_cookies")
        .map_err(|e| e.to_string())?;
    let filas = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut pares: Vec<String> = Vec::new();
    let mut total = 0usize;
    for fila in filas.flatten() {
        total += 1;
        let (host, nombre, valor) = fila;
        if nombre.is_empty() || !host_coincide(&host, dominio) {
            continue;
        }
        pares.push(format!("{nombre}={valor}"));
    }
    if pares.is_empty() {
        // Distinguir «base vacía» de «base llena sin este dominio»
        if total == 0 {
            return Err("la tabla moz_cookies está vacía".into());
        }
        return Ok(None);
    }
    // Los NOMBRES viajan al diagnóstico; los valores no salen de aquí jamás.
    // Sin los nombres no hay forma de ver que falta justo la cookie de sesión.
    let nombres: Vec<String> = pares
        .iter()
        .filter_map(|p| p.split('=').next().map(|s| s.to_string()))
        .collect();
    Ok(Some((pares.join("; "), nombres)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_dominio_se_compara_entero() {
        assert!(host_coincide("v2ph.com", "v2ph.com"));
        assert!(host_coincide(".v2ph.com", "v2ph.com"));
        assert!(host_coincide("www.v2ph.com", "v2ph.com"));
        assert!(host_coincide("cdn.v2ph.com", "v2ph.com"));
        // Lo que NO debe pasar nunca
        assert!(!host_coincide("v2ph.com.atacante.net", "v2ph.com"));
        assert!(!host_coincide("malv2ph.com", "v2ph.com"));
        assert!(!host_coincide("v2ph.company", "v2ph.com"));
        assert!(!host_coincide("", "v2ph.com"));
    }

    #[test]
    fn la_seccion_install_manda_sobre_default_1() {
        // Caso real de Windows: hay dos perfiles y el que abre Firefox es el
        // que declara [Install…], no el que lleva Default=1.
        let ini = "\
[Profile1]
Name=viejo
Path=Profiles/aaaaaaaa.default
Default=1

[Profile0]
Name=default-release
Path=Profiles/bbbbbbbb.default-release

[Install4F96D1932A9F858E]
Default=Profiles/bbbbbbbb.default-release
Locked=1
";
        assert_eq!(
            perfil_predeterminado(ini),
            Some(("Profiles/bbbbbbbb.default-release".into(), true))
        );
    }

    #[test]
    fn sin_seccion_install_vale_el_default_1() {
        let ini = "\
[Profile0]
Name=default
Path=Profiles/xxxx.default
Default=1

[Profile1]
Name=otro
Path=Profiles/yyyy.otro
";
        assert_eq!(
            perfil_predeterminado(ini),
            Some(("Profiles/xxxx.default".into(), true))
        );
    }

    #[test]
    fn un_perfil_movido_de_disco_no_se_concatena_con_la_raiz() {
        // IsRelative=0: la ruta es absoluta porque el perfil está en otro sitio
        let ini = "\
[Profile0]
Name=default
IsRelative=0
Path=D:\\Perfiles\\firefox\\principal
Default=1
";
        assert_eq!(
            perfil_predeterminado(ini),
            Some(("D:\\Perfiles\\firefox\\principal".into(), false))
        );
    }

    #[test]
    fn en_linux_la_ruta_del_perfil_no_lleva_subcarpeta() {
        // Reparto típico de Linux: los perfiles cuelgan de la raíz
        let ini = "\
[Profile0]
Name=default
IsRelative=1
Path=abcd1234.default-release
Default=1
";
        assert_eq!(
            perfil_predeterminado(ini),
            Some(("abcd1234.default-release".into(), true))
        );
    }

    #[test]
    fn un_ini_sin_predeterminado_no_inventa_uno() {
        let ini = "[Profile0]\nName=solo\nPath=Profiles/zzzz.solo\n";
        assert_eq!(perfil_predeterminado(ini), None);
        assert_eq!(perfil_predeterminado(""), None);
        assert_eq!(perfil_predeterminado("basura sin secciones"), None);
    }
}
