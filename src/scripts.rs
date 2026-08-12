//! Scripts de consola que capturan enlaces en el navegador y los envían
//! directamente a la cola de Todo Downloader (estilo Click'n'Load).
//!
//! Se ejecutan en la pestaña del perfil (F12 → Consola). Como corren dentro de
//! la propia página, heredan la sesión y las firmas de la API, que es justo lo
//! que yt-dlp no puede replicar desde fuera.

/// Cabecera común: panel visual (HUD) + envío al receptor local.
///
/// Toda la información se muestra en un panel flotante dentro de la página, no
/// en la consola: los sitios como Douyin escupen cientos de errores propios
/// (CORS, xgplayer, zijieapi) y el progreso se perdía entre el ruido.
/// Texto en el idioma activo de la aplicación.
fn m(es: &'static str, en: &'static str) -> &'static str {
    if crate::i18n::lang() == crate::i18n::Lang::Es { es } else { en }
}

/// Comillas y barras escapadas para meter el texto en un literal de JavaScript.
fn js(t: &str) -> String {
    format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Diccionario `T` que se inyecta al principio de cada script.
///
/// POR QUÉ AQUÍ Y NO EN `i18n.rs`: estos textos no los pinta la aplicación,
/// los pinta el NAVEGADOR. Viajan dentro del JavaScript que el usuario pega en
/// la consola, así que el idioma se resuelve al generar el script y se manda
/// ya traducido. El script no puede preguntarle nada a la aplicación: puede
/// estar ejecutándose sin que el receptor esté siquiera encendido.
fn dic() -> String {
    let pares: &[(&str, &str, &str)] = &[
        ("capturando", "Capturando…", "Capturing…"),
        ("enlaces", "enlaces", "links"),
        ("archivos", "archivos", "files"),
        ("detener", "■ Detener y enviar", "■ Stop and send"),
        ("guardar", "💾 Guardar JSON", "💾 Save JSON"),
        ("deteniendo", "Deteniendo…", "Stopping…"),
        ("cerrar", "Cerrar", "Close"),
        ("enviando", "Enviando…", "Sending…"),
        ("enviado", "Enviado a la app ✓", "Sent to the app ✓"),
        ("terminado", "Terminado", "Finished"),
        ("enCola", "Ya están en la cola de descargas", "They are in the download queue now"),
        ("enviadosLog", "enlaces enviados a Todo Downloader", "links sent to Todo Downloader"),
        ("respondio", "El receptor respondió ", "The receiver replied "),
        ("noApp", "App no encontrada — copiando al portapapeles",
                  "App not found — copying to the clipboard"),
        ("noRecep", "Receptor no disponible:", "Receiver unavailable:"),
        ("copiados", "📋 Copiados al portapapeles", "📋 Copied to the clipboard"),
        ("copiadosLog", "📋 Enlaces copiados — el LinkGrabber los detectará",
                        "📋 Links copied — LinkGrabber will pick them up"),
        ("usaGuardar", "Usa el botón 💾 Guardar JSON", "Use the 💾 Save JSON button"),
        ("jsonGuardado", "JSON guardado con", "JSON saved with"),
        ("elementos", "elementos", "items"),
        ("sinApi", "Sin respuestas de la API", "No replies from the API"),
        ("scrollBloqueado", "¿scroll bloqueado o login?", "scroll blocked, or sign-in needed?"),
        ("analizados", "analizados", "scanned"),
        ("sinUrl", "sin URL", "no URL"),
        ("capturados", "capturados", "captured"),
        ("diagnostico", "Diagnóstico:", "Diagnostics:"),
        ("apiNoResp", "La API no respondió: inicia sesión y recarga el perfil",
                      "The API did not reply: sign in and reload the profile"),
        ("respRecibidas", "Respuestas recibidas", "Replies received"),
        ("sinUtiles", "pero sin enlaces utilizables", "but no usable links"),
        ("ejecutaDouyin", "Todo Downloader: ejecuta esto en una página de perfil de Douyin (/user/...)",
                          "Todo Downloader: run this on a Douyin profile page (/user/...)"),
        ("leyendo", "Leyendo publicaciones…", "Reading posts…"),
        ("iniciada", "Captura iniciada — sigue el progreso en el panel de la esquina",
                     "Capture started — follow the progress in the corner panel"),
        ("vacia", "Respuesta vacía, reintentando", "Empty reply, retrying"),
        ("publicaciones", "publicaciones", "posts"),
        ("videos", "vídeos", "videos"),
        ("imagenes", "imágenes", "images"),
        ("reintentando", "Reintentando", "Retrying"),
        ("analizandoPag", "Analizando la página…", "Analyzing the page…"),
        ("pagina", "página", "page"),
        ("fotos", "fotos", "photos"),
        ("sinAlbumes", "No se ven álbumes en esta página", "No albums visible on this page"),
        ("albumesPag", "álbumes en esta página", "albums on this page"),
        ("album", "Álbum", "Album"),
        ("albumOmitido", "álbum omitido:", "album skipped:"),
        ("abreAlbum", "Abre un álbum o la página de una modelo/agencia",
                      "Open an album, or a model/agency page"),
        ("sinFotos", "No se ha encontrado ninguna foto", "No photo was found"),
        ("postSinId", "Abre un post concreto: la URL no lleva ningún identificador",
                      "Open a specific post: the URL carries no identifier"),
        ("postBuscando", "Buscando los archivos del post…", "Looking for the post files…"),
        ("postAytdlp", "El reproductor no expone una URL descargable (blob:). Se manda el post a la cola para que lo resuelva yt-dlp.",
                       "The player exposes no downloadable URL (blob:). The post is sent to the queue for yt-dlp to resolve."),
        ("postNada", "No se ha encontrado ninguna imagen en este post. Pasa el carrusel una vez a mano y vuelve a intentarlo.",
                     "No image was found in this post. Step through the carousel once by hand and try again."),
    ];
    let cuerpo: String = pares
        .iter()
        .map(|&(k, es, en)| format!("  {k}: {},\n", js(m(es, en))))
        .collect();
    format!("const T = {{\n{cuerpo}}};\n")
}

fn sender(port: u16) -> String {
    let dic = dic();
    format!(
        r#"
// ---- Panel visual de Todo Downloader ----
const TD_PORT = {port};
{dic}

function tdHud() {{
    document.getElementById('__td_hud')?.remove();
    const el = document.createElement('div');
    el.id = '__td_hud';
    el.style.cssText = [
        'position:fixed', 'top:18px', 'right:18px', 'z-index:2147483647',
        'background:#151821', 'color:#E8EAF0',
        'font:13px/1.5 -apple-system,Segoe UI,Roboto,sans-serif',
        'border:1px solid #262B39', 'border-radius:14px',
        'padding:14px 16px', 'min-width:250px',
        'box-shadow:0 10px 40px rgba(0,0,0,.55)', 'user-select:none'
    ].join(';');
    el.innerHTML = `
      <div style="display:flex;align-items:center;gap:9px;margin-bottom:10px">
        <div style="width:26px;height:26px;border-radius:8px;background:#FE2C55;
                    display:flex;align-items:center;justify-content:center;font-size:15px">⬇</div>
        <div style="line-height:1.15">
          <div style="font-weight:600">Todo <span style="color:#25F4EE">Downloader</span></div>
          <div id="__td_sub" style="font-size:10.5px;color:#8A90A0">${{T.capturando}}</div>
        </div>
        <div id="__td_x" style="margin-left:auto;cursor:pointer;color:#8A90A0;font-size:16px;padding:0 4px">×</div>
      </div>
      <div style="display:flex;align-items:baseline;gap:7px">
        <div id="__td_n" style="font-size:30px;font-weight:600;color:#25F4EE">0</div>
        <div style="font-size:11px;color:#8A90A0" id="__td_lbl">${{T.enlaces}}</div>
      </div>
      <div style="height:5px;background:#262B39;border-radius:3px;margin:10px 0 4px;overflow:hidden">
        <div id="__td_bar" style="height:100%;width:30%;background:linear-gradient(90deg,#FE2C55,#25F4EE);
             border-radius:3px;transition:width .3s;animation:__tdp 1.4s ease-in-out infinite"></div>
      </div>
      <div id="__td_msg" style="font-size:11px;color:#8A90A0;min-height:15px"></div>
      <button id="__td_stop" style="width:100%;margin-top:9px;padding:7px;border:0;border-radius:8px;
              background:#262B39;color:#E8EAF0;font-size:12px;cursor:pointer">${{T.detener}}</button>
      <button id="__td_save" style="width:100%;margin-top:6px;padding:7px;border:0;border-radius:8px;
              background:#FE2C55;color:#fff;font-size:12px;cursor:pointer;display:none">${{T.guardar}}</button>
      <style>@keyframes __tdp{{0%{{transform:translateX(-100%)}}100%{{transform:translateX(340%)}}}}</style>`;
    document.body.appendChild(el);
    el.querySelector('#__td_x').onclick = () => el.remove();
    el.querySelector('#__td_stop').onclick = () => {{
        window.__tdStop = true;
        el.querySelector('#__td_msg').textContent = T.deteniendo;
    }};
    // Guardar a archivo: vía manual siempre disponible al terminar
    el.querySelector('#__td_save').onclick = () => tdSaveFile(window.__tdItems || []);
    return {{
        n(v) {{ const e = document.getElementById('__td_n'); if (e) e.textContent = v; }},
        lbl(t) {{ const e = document.getElementById('__td_lbl'); if (e) e.textContent = t; }},
        msg(t) {{ const e = document.getElementById('__td_msg'); if (e) e.textContent = t; }},
        sub(t) {{ const e = document.getElementById('__td_sub'); if (e) e.textContent = t; }},
        done(v, ok, extra) {{
            const bar = document.getElementById('__td_bar');
            if (bar) {{ bar.style.animation = 'none'; bar.style.width = '100%'; }}
            const btn = document.getElementById('__td_stop');
            if (btn) {{ btn.textContent = T.cerrar; btn.onclick = () => document.getElementById('__td_hud')?.remove(); }}
            // El botón de guardar aparece al terminar, haya ido bien el envío o no
            const sv = document.getElementById('__td_save');
            if (sv && v > 0) sv.style.display = 'block';
            const sub = document.getElementById('__td_sub');
            if (sub) {{ sub.textContent = ok ? T.enviado : T.terminado; sub.style.color = ok ? '#3DDC84' : '#FFB454'; }}
            this.n(v);
            this.msg(extra || '');
        }}
    }};
}}

async function tdSend(items, hud, mode) {{
    if (!items.length) return false;
    const cuerpo = JSON.stringify({{ source: location.hostname, items, mode: mode || '' }});

    // Chrome y Vivaldi bloquean que una PÁGINA hable con 127.0.0.1 (Private
    // Network Access). GM_xmlhttpRequest corre en el contexto de la extensión
    // del gestor de userscripts, que no tiene esa restricción. Si no existe
    // —bookmarklet, o consola pelada— se cae a fetch, que en Firefox va bien.
    if (typeof GM_xmlhttpRequest === 'function') {{
        const ok = await new Promise(res => GM_xmlhttpRequest({{
            method: 'POST',
            url: `http://127.0.0.1:${{TD_PORT}}/add`,
            headers: {{ 'Content-Type': 'application/json' }},
            data: cuerpo,
            onload: r => res(r.status >= 200 && r.status < 300),
            onerror: () => res(false),
            ontimeout: () => res(false)
        }}));
        if (ok) {{
            hud && hud.done(items.length, true, T.enCola);
            return true;
        }}
        hud && hud.msg(T.noApp);
        return false;
    }}

    try {{
        const res = await fetch(`http://127.0.0.1:${{TD_PORT}}/add`, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: cuerpo
        }});
        if (res.ok) {{
            hud && hud.done(items.length, true, T.enCola);
            console.log(`[TD] ✅ ${{items.length}} ${{T.enviadosLog}}`);
            return true;
        }}
        hud && hud.msg(T.respondio + res.status);
    }} catch (e) {{
        hud && hud.msg(T.noApp);
        console.warn('[TD]', T.noRecep, e.message);
    }}
    return false;
}}

function tdFallbackCopy(items, hud) {{
    const txt = items.map(i => i.url).join('\n');
    navigator.clipboard.writeText(txt).then(
        () => {{
            hud && hud.done(items.length, false, T.copiados);
            console.log('[TD]', T.copiadosLog);
        }},
        () => {{
            hud && hud.done(items.length, false, T.usaGuardar);
            console.log(txt);
        }}
    );
}}

/** Guarda un JSON con TODOS los metadatos (autor, título, portada) en el
 *  formato que importa la app: Descargas → Importar TXT/JSON.
 *
 *  Es la vía a prueba de balas: no depende del portapapeles (que exige que la
 *  pestaña tenga el foco) ni de que el navegador permita hablar con 127.0.0.1
 *  (Chrome lo restringe con Private Network Access). Descargar un archivo
 *  siempre funciona. */
function tdSaveFile(items) {{
    if (!items || !items.length) return;
    const data = {{ videos: items.map(i => ({{
        id: i.id, author: i.author, title: i.title,
        hqUrl: i.url, pageUrl: i.pageUrl, thumb: i.thumb || ''
    }})) }};
    const blob = new Blob([JSON.stringify(data, null, 2)], {{ type: 'application/json' }});
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `todo-downloader-${{location.hostname.replace(/^www\./, '')}}-${{items.length}}.json`;
    document.body.appendChild(a);
    a.click();
    setTimeout(() => {{ URL.revokeObjectURL(a.href); a.remove(); }}, 5000);
    console.log(`[TD] 💾 ${{T.jsonGuardado}} ${{items.length}} ${{T.elementos}}`);
}}
"#
    )
}

/// Script para perfiles de TikTok (interceptor de API + auto-scroll)
pub fn tiktok(port: u16) -> String {
    let rotulo = m("Capturador de TikTok", "TikTok capturer");
    let donde = m("Ejecutar en", "Run this on");
    let consola = m("Consola", "Console");
    format!(
        r#"/* Todo Downloader — {rotulo} — By Eric V. Gramunt
   {donde} https://www.tiktok.com/@usuario (F12 → {consola}) */
(() => {{
{sender}
const items = new Map();
// Muy amplio a propósito: TikTok cambia rutas con frecuencia. Antes solo se
// miraban 3 endpoints exactos y bastaba un renombrado suyo para no capturar nada.
const API = /item_list|\/api\/(post|repost|favorite|item|search)|aweme|\/feed/;
const diag = {{ hits: 0, scanned: 0, added: 0, noUrl: 0 }};

/** Saca la primera URL utilizable de un campo que puede venir en varias formas:
 *  string suelto, {{url_list:[…]}}, {{urlList:[…]}} o un array de strings. */
function firstUrl(x) {{
    if (!x) return '';
    if (typeof x === 'string') return x.startsWith('http') ? x : '';
    if (Array.isArray(x)) return firstUrl(x[0]);
    return firstUrl(x.url_list || x.urlList || x.url || x.playAddr || '');
}}

function add(it) {{
    diag.scanned++;
    if (!it || !it.id || items.has(it.id)) return;
    const v = it.video || {{}};
    const author = (it.author && (it.author.uniqueId || it.author.unique_id))
                || (location.pathname.match(/@([^/]+)/) || [])[1] || '';

    // Máxima calidad: mayor bitrate de bitrateInfo; si no, playAddr/downloadAddr
    const q = (v.bitrateInfo || v.bitrate_info || [])
        .map(b => ({{ br: b.Bitrate || b.bit_rate || 0,
                     u: firstUrl((b.PlayAddr || b.play_addr)) }}))
        .filter(x => x.u).sort((a, b) => b.br - a.br);

    let url = q.length ? q[0].u
            : firstUrl(v.playAddr) || firstUrl(v.play_addr)
              || firstUrl(v.downloadAddr) || firstUrl(v.download_addr);

    // Post de imágenes (carrusel): TikTok los sirve en imagePost
    const imgs = (it.imagePost && it.imagePost.images) || (it.image_post_info && it.image_post_info.images) || [];
    if (!url && imgs.length) {{
        imgs.forEach((im, n) => {{
            const iu = firstUrl(im.imageURL || im.display_image || im.owner_watermark_image || im);
            if (!iu) return;
            const key = it.id + '_' + n;
            if (items.has(key)) return;
            items.set(key, {{ id: key, author, title: it.desc || '', url: iu, thumb: iu,
                pageUrl: `https://www.tiktok.com/@${{author}}/photo/${{it.id}}` }});
            diag.added++;
        }});
        return;
    }}

    if (!url) {{ diag.noUrl++; return; }}
    if (url.startsWith('http://')) url = 'https://' + url.slice(7);
    const thumb = firstUrl(v.cover) || firstUrl(v.originCover) || firstUrl(v.origin_cover) || firstUrl(v.dynamicCover);
    items.set(it.id, {{ id: it.id, author, title: it.desc || '', url, thumb,
        pageUrl: `https://www.tiktok.com/@${{author}}/video/${{it.id}}` }});
    diag.added++;
}}

/** Recorre CUALQUIER JSON buscando objetos que parezcan publicaciones.
 *  Mucho más resistente que asumir `itemList`: si TikTok renombra el contenedor,
 *  esto los sigue encontrando por su forma (id + video/imagePost). */
function harvest(o, depth) {{
    if (!o || typeof o !== 'object' || (depth || 0) > 6) return;
    if (Array.isArray(o)) {{ o.forEach(x => harvest(x, (depth || 0) + 1)); return; }}
    if (o.id && (o.video || o.imagePost || o.image_post_info || o.desc !== undefined)) {{
        add(o);
    }}
    for (const k in o) {{
        const v = o[k];
        if (v && typeof v === 'object') harvest(v, (depth || 0) + 1);
    }}
}}

function ingest(d) {{
    if (!d) return;
    diag.hits++;
    const list = d.itemList || d.items || d.aweme_list
              || (d.itemInfo && d.itemInfo.itemStruct ? [d.itemInfo.itemStruct] : null);
    if (list && list.length) list.forEach(add);
    else harvest(d, 0);   // formato desconocido: búsqueda por forma
}}

// Interceptar fetch y XHR
if (!window.__td_hooked) {{
    window.__td_hooked = true;
    const of = window.fetch;
    window.fetch = async function (...a) {{
        const r = await of.apply(this, a);
        try {{
            const u = typeof a[0] === 'string' ? a[0] : (a[0] && a[0].url) || '';
            if (API.test(u)) r.clone().json().then(ingest).catch(() => {{}});
        }} catch (e) {{}}
        return r;
    }};
    const oo = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (m, u, ...r) {{ this.__u = u; return oo.call(this, m, u, ...r); }};
    const os = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function (...a) {{
        this.addEventListener('load', () => {{
            try {{ if (API.test(this.__u || '')) ingest(JSON.parse(this.responseText)); }} catch (e) {{}}
        }});
        return os.apply(this, a);
    }};
}}

// Datos ya embebidos en la página
try {{
    const el = document.getElementById('__UNIVERSAL_DATA_FOR_REHYDRATION__');
    if (el) {{
        const s = JSON.parse(el.textContent)['__DEFAULT_SCOPE__'] || {{}};
        const d = s['webapp.video-detail'];
        if (d && d.itemInfo && d.itemInfo.itemStruct) add(d.itemInfo.itemStruct);
    }}
}} catch (e) {{}}

// Auto-scroll hasta agotar el perfil
(async () => {{
    const hud = tdHud();
    hud.sub('Desplazando el perfil…');
    console.log('[TD]', T.iniciada);
    let idle = 0, last = 0;
    window.__tdStop = false;
    while (!window.__tdStop && idle < 8) {{
        window.scrollTo(0, document.body.scrollHeight);
        await new Promise(r => setTimeout(r, 1400));
        if (items.size === last) {{
            idle++;
            // Diagnóstico: distingue «no llegan respuestas» de «llegan pero no
            // se extrae nada». Sin esto era imposible saber qué falla.
            hud.msg(diag.hits === 0
                ? `${{T.sinApi}} (${{idle}}/8) — ${{T.scrollBloqueado}}`
                : `API: ${{diag.hits}} · ${{T.analizados}}: ${{diag.scanned}} · ${{T.sinUrl}}: ${{diag.noUrl}} (${{idle}}/8)`);
        }} else {{
            idle = 0;
            last = items.size;
            hud.msg(`API: ${{diag.hits}} · ${{T.capturados}}: ${{diag.added}}`);
        }}
        hud.n(items.size);
    }}
    const arr = [...items.values()];
    window.__tdItems = arr;   // lo usa el botón «💾 Guardar JSON»
    console.log('[TD]', T.diagnostico, diag);
    if (!arr.length) {{
        hud.done(0, false, diag.hits === 0
            ? T.apiNoResp
            : `${{T.respRecibidas}} (${{diag.hits}}) ${{T.sinUtiles}}`);
        return;
    }}
    hud.sub(T.enviando);
    if (!(await tdSend(arr, hud))) tdFallbackCopy(arr, hud);
}})();
}})();
"#,
        sender = sender(port)
    )
}

/// Script para perfiles de Douyin — vídeos e imágenes (API directa, como el
/// script original que ya funciona: Douyin no firma estas peticiones)
pub fn douyin(port: u16) -> String {
    let rotulo = m("Capturador de Douyin (vídeos + imágenes)", "Douyin capturer (videos + images)");
    let donde = m("Ejecutar en", "Run this on");
    let consola = m("Consola", "Console");
    format!(
        r#"/* Todo Downloader — {rotulo} — By Eric V. Gramunt
   {donde} https://www.douyin.com/user/... (F12 → {consola}) */
(() => {{
{sender}
const SEC = location.pathname.replace('/user/', '').split('?')[0];
const Q = 100;                 // calidad de imagen (q100 = máxima)
const WANT_VIDEOS = true;
const WANT_IMAGES = true;

const items = [];
const seen = new Set();
const posts = new Set();   // publicaciones distintas (un carrusel = 1 publicación)
let vids = 0, imgs = 0;

async function api(cursor) {{
    const u = `https://www.douyin.com/aweme/v1/web/aweme/post/?device_platform=webapp&aid=6383`
        + `&channel=channel_pc_web&sec_user_id=${{SEC}}&max_cursor=${{cursor}}&count=20`
        + `&version_code=170400&version_name=17.4.0`;
    const r = await fetch(u, {{ credentials: 'include', headers: {{ accept: 'application/json, text/plain, */*' }} }});
    if (!r.ok) throw new Error('HTTP ' + r.status);
    return r.json();
}}

// Fuerza la máxima calidad en las URL de imagen de Douyin.
// Cubre las tres variantes que usa su CDN: ":q75" en la ruta, "&quality=75"
// y el parámetro suelto "?q=75" / "&q=75".
// NO se tocan los parámetros de tamaño (w/h): poner 0 no devuelve el original,
// hace que el CDN rechace la petición.
function hq(url) {{
    return url
        .replace(/:q\d+/g, ':q' + Q)
        .replace(/([?&])quality=\d+/g, '$1quality=' + Q)
        .replace(/([?&])q=\d+/g, '$1q=' + Q);
}}

function collect(aw) {{
    const id = aw.aweme_id;
    const author = (aw.author && (aw.author.nickname || aw.author.unique_id)) || 'douyin';
    const title = (aw.desc || '').slice(0, 60);

    posts.add(id);

    // Post de imágenes (carrusel: cada foto es un archivo aparte)
    if (WANT_IMAGES && Array.isArray(aw.images) && aw.images.length) {{
        aw.images.forEach((img, n) => {{
            let u = (img.download_url_list && img.download_url_list[0])
                 || (img.url_list && img.url_list[0]) || '';
            if (!u) return;
            u = hq(u);
            const key = id + '_' + n;
            if (seen.has(key)) return;
            seen.add(key);
            // Miniatura: la última entrada de url_list suele ser la vista previa
            // webp ligera (la original iría por download_url_list)
            const thumb = (img.url_list && img.url_list[img.url_list.length - 1]) || '';
            items.push({{ id: key, author, title, url: u, thumb, pageUrl: `https://www.douyin.com/note/${{id}}` }});
            imgs++;
        }});
        return;
    }}

    // Vídeo
    if (WANT_VIDEOS && aw.video) {{
        const list = (aw.video.play_addr && aw.video.play_addr.url_list)
                  || (aw.video.download_addr && aw.video.download_addr.url_list) || [];
        let u = list[list.length - 1] || '';
        if (!u || seen.has(id)) return;
        seen.add(id);
        if (u.startsWith('http://')) u = 'https://' + u.slice(7);
        const thumb = (aw.video.cover && aw.video.cover.url_list && aw.video.cover.url_list[0]) || '';
        items.push({{ id, author, title, url: u, thumb, pageUrl: `https://www.douyin.com/video/${{id}}` }});
        vids++;
    }}
}}

(async () => {{
    if (!SEC || !location.pathname.includes('/user/')) {{
        alert(T.ejecutaDouyin);
        return;
    }}
    const hud = tdHud();
    hud.sub(T.leyendo);
    console.log('[TD]', T.iniciada);

    let cursor = 0, more = true, fails = 0, pages = 0;
    window.__tdStop = false;
    while (more && !window.__tdStop && fails < 5) {{
        try {{
            const d = await api(cursor);
            if (!d || !d.aweme_list) {{
                fails++;
                hud.msg(`${{T.vacia}} (${{fails}}/5)`);
                await new Promise(r => setTimeout(r, 2000));
                continue;
            }}
            fails = 0;
            pages++;
            d.aweme_list.forEach(collect);
            more = d.has_more === 1;
            cursor = d.max_cursor;
            hud.n(items.length);
            hud.lbl(T.archivos);
            hud.msg(`${{posts.size}} ${{T.publicaciones}} · ${{vids}} ${{T.videos}} · ${{imgs}} ${{T.imagenes}}`);
            await new Promise(r => setTimeout(r, 1500));
        }} catch (e) {{
            fails++;
            hud.msg(`${{T.reintentando}} (${{fails}}/5)…`);
            await new Promise(r => setTimeout(r, 3000));
        }}
    }}
    hud.sub(T.enviando);
    window.__tdItems = items;   // lo usa el botón «💾 Guardar JSON»
    const resumen = `${{posts.size}} ${{T.publicaciones}} → ${{vids}} ${{T.videos}} + ${{imgs}} ${{T.imagenes}}`;
    if (await tdSend(items, hud)) {{
        hud.msg(resumen);
    }} else {{
        tdFallbackCopy(items, hud);
    }}
    console.log(`[TD] ${{resumen}} = ${{items.length}} archivos`);
}})();
}})();
"#,
        sender = sender(port)
    )
}

/// Script para V2PH: el NAVEGADOR recorre el álbum y la app solo descarga.
///
/// POR QUÉ EXISTE: V2PH pasó a rechazar con 403 las peticiones de la
/// aplicación aunque llevaran sesión y las cabeceras correctas, mientras el
/// navegador seguía entrando sin problema. Eso ocurre por debajo de las
/// cabeceras —en la huella del handshake TLS—, así que no hay cabecera ni
/// cookie que lo arregle desde fuera.
///
/// Aquí las peticiones las hace la pestaña donde el usuario ya está: su
/// sesión, su IP y su huella. No imita a un navegador, ES el navegador. De
/// paso resuelve el muro de las 10 fotos, porque la sesión va incluida.
///
/// La aplicación solo se queda con la descarga de `cdn.v2ph.com`, que no está
/// protegido. Si algún día también lo estuviera, queda el botón de guardar
/// JSON para importarlo a mano.
pub fn v2ph(port: u16) -> String {
    format!(
        r#"{cabecera}
(async () => {{
    const hud = tdHud();
    hud.msg(T.analizandoPag);

    const RETARDO = 900;      // ms entre páginas: el navegador también es un cliente
    const MAX_ALBUMES = 12;   // tope al recorrer un listado
    const MAX_PAGINAS = 60;   // salvaguarda contra una paginación rota

    const dormir = ms => new Promise(r => setTimeout(r, ms));

    // Descarga una página del sitio y la convierte en documento
    async function pedir(url) {{
        const r = await fetch(url, {{ credentials: 'include' }});
        if (!r.ok) throw new Error('HTTP ' + r.status + ' en ' + url);
        return new DOMParser().parseFromString(await r.text(), 'text/html');
    }}

    // Última página según los enlaces de paginación: se toma el máximo en vez
    // de buscar el rótulo «Último», que está traducido a diez idiomas.
    function ultimaPagina(doc) {{
        let max = 1;
        doc.querySelectorAll('a[href*="page="]').forEach(a => {{
            const m = a.getAttribute('href').match(/[?&]page=(\d+)/);
            if (m) max = Math.max(max, parseInt(m[1], 10));
        }});
        return Math.min(max, MAX_PAGINAS);
    }}

    // Las fotos del álbum viven en /photos/. Las portadas de «galerías
    // relacionadas» están en /album/, otra ruta, y por eso no se cuelan.
    function fotosDe(doc) {{
        return [...doc.querySelectorAll('img[src*="cdn.v2ph.com/photos/"]')]
            .map(i => i.src);
    }}

    function tituloDe(doc) {{
        const og = doc.querySelector('meta[property="og:title"]');
        return (og && og.content) || (doc.title || '').replace(/ - V2PH$/, '');
    }}

    function modeloDe(doc) {{
        const a = doc.querySelector('a[href*="/actor/"]');
        if (!a) return '';
        return (a.textContent || '').trim()
            || (a.getAttribute('href').match(/\/actor\/([^/.]+)/) || [])[1] || '';
    }}

    /** Recorre TODAS las páginas internas de un álbum. */
    async function album(base, aviso) {{
        const id = (base.match(/\/album\/([^/?#]+)/) || [])[1] || 'v2ph';
        const doc1 = await pedir(base);
        const titulo = tituloDe(doc1);
        const autor = modeloDe(doc1) || id;
        const ultima = ultimaPagina(doc1);

        const vistas = new Set();
        const fotos = [];
        for (const u of fotosDe(doc1)) if (!vistas.has(u)) {{ vistas.add(u); fotos.push(u); }}

        for (let p = 2; p <= ultima; p++) {{
            aviso(`${{titulo.slice(0, 40)}} — ${{T.pagina}} ${{p}}/${{ultima}} (${{fotos.length}} ${{T.fotos}})`);
            await dormir(RETARDO);
            let doc;
            try {{ doc = await pedir(base.split('?')[0] + '?page=' + p); }}
            catch (e) {{ console.warn('[TD]', e.message); break; }}
            const nuevas = fotosDe(doc).filter(u => !vistas.has(u));
            // Sin fotos nuevas = fin del álbum, aunque la paginación prometiera más
            if (!nuevas.length) break;
            nuevas.forEach(u => {{ vistas.add(u); fotos.push(u); }});
        }}

        const total = fotos.length;
        return fotos.map((u, i) => ({{
            url: u,
            author: autor,
            title: `${{titulo}} (${{i + 1}}/${{total}})`,
            pageUrl: base,
            id: `${{id}}_${{String(i + 1).padStart(3, '0')}}`,
            thumb: u
        }}));
    }}

    try {{
        const ruta = location.pathname;
        let items = [];

        if (/^\/album\//.test(ruta)) {{
            items = await album(location.origin + ruta, m => hud.msg(m));
        }} else if (/^\/(actor|company|category|country)\//.test(ruta)) {{
            // Listado: se recorren los álbumes de ESTA página, no del sitio entero
            const enlaces = [...new Set(
                [...document.querySelectorAll('a[href*="/album/"]')]
                    .map(a => a.href.split('?')[0])
            )].slice(0, MAX_ALBUMES);

            if (!enlaces.length) throw new Error(T.sinAlbumes);
            hud.msg(`${{enlaces.length}} ${{T.albumesPag}}`);

            for (let k = 0; k < enlaces.length; k++) {{
                hud.msg(`${{T.album}} ${{k + 1}}/${{enlaces.length}}…`);
                try {{
                    items = items.concat(await album(enlaces[k], m => hud.msg(`[${{k + 1}}/${{enlaces.length}}] ${{m}}`)));
                }} catch (e) {{
                    console.warn('[TD]', T.albumOmitido, e.message);
                }}
                hud.n(items.length);
                await dormir(RETARDO);
            }}
        }} else {{
            throw new Error(T.abreAlbum);
        }}

        if (!items.length) throw new Error(T.sinFotos);
        hud.n(items.length);

        const ok = await tdSend(items, hud);
        if (!ok) {{ tdFallbackCopy(items, hud); tdSaveFile(items); }}
    }} catch (e) {{
        hud.msg('❌ ' + e.message);
        console.error('[TD]', e);
    }}
}})();
"#,
        cabecera = sender(port)
    )
}

// ============================================================================
//  Captura de un post suelto (Douyin y TikTok)
// ============================================================================

/// Cuerpo compartido del capturador de un post suelto.
///
/// POR QUÉ NO SE LLAMA A LA API: la de Douyin va firmada (X-Bogus, msToken) y
/// pedirla desde fuera devuelve vacío. El capturador de perfiles lo esquiva
/// interceptando las respuestas mientras haces scroll, pero para un post ya
/// abierto esa respuesta YA PASÓ. Así que se lee lo que la página tiene
/// delante, que además resultó ser más completo de lo esperado.
///
/// CÓMO SE IDENTIFICAN LAS DIAPOSITIVAS, medido sobre el DOM real de Douyin:
///
/// Cada diapositiva se pinta dos veces. Una pequeña y centrada, que es la que
/// se ve, y otra a ANCHO COMPLETO por detrás, desenfocada, que hace de fondo.
/// El visor es un feed vertical de posts y cada uno ocupa una franja:
///
/// ```text
///     2100x415 @ (0, -415)     post anterior
///     2100x415 @ (0, 0)        ESTE post, diapositiva 1
///     2100x415 @ (2100, 0)     ESTE post, diapositiva 2
///     2100x415 @ (0, +415)     post siguiente
/// ```
///
/// Las diapositivas de un mismo post comparten `top` y se separan en `left`;
/// los posts distintos se separan en `top`. Con eso se identifica el post
/// entero sin depender de nombres de clase (ofuscados), sin pulsar flechas y
/// sin esperar a que cargue nada: ya está todo en la página.
///
/// Lo que hubo antes —recorrer el carrusel a golpe de clic— nunca funcionó:
/// el botón no se encontraba porque su clase cambia en cada despliegue, y
/// aunque se hubiera encontrado habría sido dar un rodeo para llegar a algo
/// que ya estaba ahí. Se descubrió volcando el DOM en vez de suponiéndolo.
///
/// LOS VÍDEOS NO SE EXTRAEN. En un post de vídeo el `<video>` expone un
/// `blob:` de Media Source Extensions, que solo existe dentro de esa pestaña.
/// Y lo que SÍ tiene URL en la página es la música (`ies-music/….mp3`) y los
/// vídeos de otros posts del feed: cogerlos sería descargar cualquier cosa
/// menos lo que se pidió. Se le pasa el post a yt-dlp, que para eso está.
///
/// LA CALIDAD NO SE RESUELVE AQUÍ. Douyin sirve el `~noop` —el original sin
/// marca de agua ni recompresión— directamente en el DOM, así que se coge tal
/// cual. Y para lo que no lo traiga, `quality_variants()` en la aplicación ya
/// lo intenta. Repetir esa lógica en JavaScript sería tener dos sitios donde
/// equivocarse.
///
/// Los comentarios DENTRO del script van en inglés y son cortos a propósito:
/// es un archivo que el usuario pega en su navegador, no código que nadie
/// vaya a mantener desde ahí. El razonamiento vive aquí, que es donde se lee.
fn post_core() -> &'static str {
    r#"
const TD_IMG = /(douyinpic|byteimg|ibyteimg|tiktokcdn|bytecdn)\.com\//i;

// Post id, from the URL.
function tdPostId() {
    const u = new URL(location.href);
    const modal = u.searchParams.get('modal_id');
    if (modal) return modal;
    const m = u.pathname.match(/\/(?:note|video|photo)\/(\d+)/);
    return m ? m[1] : '';
}

// Key for one photo, ignoring the CDN processing: `~noop` and `~tplv-…` of the
// same original share this prefix.
function tdBase(u) {
    const i = u.indexOf('~');
    return (i > 0 ? u.slice(0, i) : u.split('?')[0]);
}

// Slides of the open post. Each slide is painted twice: small and centred (the
// one you see) and full width behind it, blurred. Slides of one post share
// `top` and differ in `left`; different posts differ in `top`. `~noop` is the
// unprocessed original and is preferred when present.
function tdSlides() {
    const anchos = [];
    for (const img of document.querySelectorAll('img')) {
        const u = img.currentSrc || img.src || '';
        if (!u || !TD_IMG.test(u)) continue;
        const r = img.getBoundingClientRect();
        if (r.width < innerWidth * 0.85) continue;
        anchos.push({ u, top: r.top, alto: r.height });
    }
    if (!anchos.length) return { urls: [] };

    const cy = innerHeight / 2;
    let ref = anchos.find(o => o.top <= cy && o.top + o.alto >= cy);
    if (!ref) {
        ref = anchos.reduce((a, b) =>
            Math.abs(a.top + a.alto / 2 - cy) < Math.abs(b.top + b.alto / 2 - cy) ? a : b);
    }
    const tol = Math.max(20, ref.alto * 0.3);

    const porFoto = new Map();
    for (const o of anchos) {
        if (Math.abs(o.top - ref.top) > tol) continue;
        const b = tdBase(o.u);
        const previa = porFoto.get(b);
        if (!previa || (o.u.indexOf('~noop') >= 0 && previa.indexOf('~noop') < 0)) {
            porFoto.set(b, o.u);
        }
    }
    return { urls: [...porFoto.values()] };
}

// Is the centre of the window a video player?
function tdEsVideo() {
    const el = document.elementFromPoint(innerWidth / 2, innerHeight / 2);
    if (el && el.tagName === 'VIDEO') return true;
    for (const v of document.querySelectorAll('video')) {
        const r = v.getBoundingClientRect();
        if (r.width > innerWidth * 0.15 && r.height > innerHeight * 0.3
            && r.top < innerHeight / 2 && r.bottom > innerHeight / 2) return true;
    }
    return false;
}

// Author and title, for the subfolder and the file name.
function tdMeta() {
    const t = (document.title || '').replace(/\s*[-|]\s*(抖音|TikTok).*$/, '').trim();
    const a = document.querySelector('[class*="account-name"],[data-e2e="user-title"],h1');
    return { autor: (a && a.textContent.trim()) || location.hostname.replace(/^www\./, ''),
             titulo: t.slice(0, 80) };
}

async function tdCapturarPost(modo) {
    const hud = tdHud();
    const id = tdPostId();
    if (!id) { hud.done(0, false, T.postSinId); return; }

    hud.lbl(T.archivos);
    hud.msg(T.postBuscando);

    const meta = tdMeta();
    const canon = h => (location.hostname.indexOf('tiktok') >= 0
        ? 'https://www.tiktok.com/@' + encodeURIComponent(meta.autor) + '/' + h + '/' + id
        : 'https://www.douyin.com/' + h + '/' + id);

    // Video post: the player exposes only a `blob:`, unusable outside this tab.
    // The post URL goes to the application and yt-dlp resolves it. The blurred
    // backdrop is the poster and serves as the thumbnail.
    if (tdEsVideo()) {
        const portada = (tdSlides().urls || [])[0] || '';
        const item = [{ id: id, author: meta.autor, title: meta.titulo,
                        url: canon('video'), pageUrl: canon('video'), thumb: portada }];
        window.__tdItems = item;
        hud.n(1);
        hud.msg(T.postAytdlp);
        if (!(await tdSend(item, hud, modo))) tdFallbackCopy(item, hud);
        return;
    }

    const r = tdSlides();
    if (!r.urls.length) { hud.done(0, false, T.postNada); return; }

    const pagina = canon('note');
    const items = r.urls.map((u, i) => ({
        id: id + '_' + String(i + 1).padStart(2, '0'),
        author: meta.autor,
        title: meta.titulo,
        url: u,
        pageUrl: pagina,
        thumb: u
    }));

    window.__tdItems = items;
    hud.n(items.length);
    hud.msg(items.length + ' ' + T.imagenes);
    if (!(await tdSend(items, hud, modo))) tdFallbackCopy(items, hud);
}

"#
}

/// Userscript para Tampermonkey o Violentmonkey.
///
/// POR QUÉ ES LA VÍA BUENA EN CHROME Y VIVALDI: `GM_xmlhttpRequest` corre en
/// el contexto de la extensión, que no está sujeta a Private Network Access.
/// Una página normal —y por tanto un bookmarklet— no puede hablar con
/// `127.0.0.1` en los navegadores basados en Chromium.
pub fn userscript(port: u16) -> String {
    let nucleo = post_core();
    let envio = sender(port);
    let boton = m("Capturar este post", "Capture this post");
    let rotulo = m("captura de un post", "single post capture");
    let descripcion = m(
        "Añade un botón para enviar las fotos o el vídeo del post abierto a Todo Downloader",
        "Adds a button to send the photos or the video of the open post to Todo Downloader",
    );
    format!(
        r#"// ==UserScript==
// @name         Todo Downloader — {rotulo}
// @namespace    https://github.com/AcidClawX41/todo-downloader
// @version      1.1
// @description  {descripcion}
// @match        https://www.douyin.com/*
// @match        https://www.tiktok.com/*
// @grant        GM_xmlhttpRequest
// @connect      127.0.0.1
// @run-at       document-idle
// ==/UserScript==
(() => {{
'use strict';
{envio}
{nucleo}

// Botón flotante. Se pinta siempre y se activa solo cuando hay un post
// abierto: en Douyin y TikTok la navegación no recarga la página, así que
// mirar la URL una vez al cargar no serviría de nada.
const b = document.createElement('button');
b.textContent = '⬇ {boton}';
b.style.cssText = [
    'position:fixed','right:18px','bottom:18px','z-index:2147483646',
    'padding:10px 14px','border:0','border-radius:10px',
    'background:#FE2C55','color:#fff','font:13px/1 sans-serif',
    'cursor:pointer','box-shadow:0 6px 24px rgba(0,0,0,.45)','display:none'
].join(';');
b.onclick = () => tdCapturarPost('post');
document.body.appendChild(b);

setInterval(() => {{ b.style.display = tdPostId() ? 'block' : 'none'; }}, 800);
}})();
"#
    )
}

/// Bookmarklet: una sola línea `javascript:` para arrastrar a la barra.
///
/// LO QUE NO PUEDE HACER: corre dentro de la página, así que en Chrome y
/// Vivaldi choca con Private Network Access y no alcanza a `127.0.0.1`. Ahí
/// cae al respaldo de siempre —portapapeles y botón de guardar JSON—, que
/// funciona en todas partes. En Firefox llega directo a la aplicación.
pub fn bookmarklet(port: u16) -> String {
    let cuerpo = format!("{}\n{}\ntdCapturarPost('post');", sender(port), post_core());

    // Un bookmarklet es una URL: los comentarios de línea se comerían todo lo
    // que viniera detrás, y hay que codificar lo que no es seguro en una URL.
    let sin_comentarios: String = cuerpo
        .lines()
        .map(|l| {
            let t = l.trim_start();
            if t.starts_with("//") || t.starts_with("*") || t.starts_with("/*") { "" } else { l }
        })
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let escapado: String = sin_comentarios
        .chars()
        .map(|c| match c {
            '%' => "%25".into(),
            '"' => "%22".into(),
            '#' => "%23".into(),
            '&' => "%26".into(),
            '+' => "%2B".into(),
            '?' => "%3F".into(),
            ' ' => "%20".into(),
            '\n' => "%0A".into(),
            _ => c.to_string(),
        })
        .collect();

    format!("javascript:(()=>{{{escapado}}})();")
}
