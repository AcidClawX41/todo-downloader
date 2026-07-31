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
fn sender(port: u16) -> String {
    format!(
        r#"
// ---- Panel visual de Todo Downloader ----
const TD_PORT = {port};

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
          <div id="__td_sub" style="font-size:10.5px;color:#8A90A0">Capturando…</div>
        </div>
        <div id="__td_x" style="margin-left:auto;cursor:pointer;color:#8A90A0;font-size:16px;padding:0 4px">×</div>
      </div>
      <div style="display:flex;align-items:baseline;gap:7px">
        <div id="__td_n" style="font-size:30px;font-weight:600;color:#25F4EE">0</div>
        <div style="font-size:11px;color:#8A90A0" id="__td_lbl">enlaces</div>
      </div>
      <div style="height:5px;background:#262B39;border-radius:3px;margin:10px 0 4px;overflow:hidden">
        <div id="__td_bar" style="height:100%;width:30%;background:linear-gradient(90deg,#FE2C55,#25F4EE);
             border-radius:3px;transition:width .3s;animation:__tdp 1.4s ease-in-out infinite"></div>
      </div>
      <div id="__td_msg" style="font-size:11px;color:#8A90A0;min-height:15px"></div>
      <button id="__td_stop" style="width:100%;margin-top:9px;padding:7px;border:0;border-radius:8px;
              background:#262B39;color:#E8EAF0;font-size:12px;cursor:pointer">■ Detener y enviar</button>
      <style>@keyframes __tdp{{0%{{transform:translateX(-100%)}}100%{{transform:translateX(340%)}}}}</style>`;
    document.body.appendChild(el);
    el.querySelector('#__td_x').onclick = () => el.remove();
    el.querySelector('#__td_stop').onclick = () => {{
        window.__tdStop = true;
        el.querySelector('#__td_msg').textContent = 'Deteniendo…';
    }};
    return {{
        n(v) {{ const e = document.getElementById('__td_n'); if (e) e.textContent = v; }},
        lbl(t) {{ const e = document.getElementById('__td_lbl'); if (e) e.textContent = t; }},
        msg(t) {{ const e = document.getElementById('__td_msg'); if (e) e.textContent = t; }},
        sub(t) {{ const e = document.getElementById('__td_sub'); if (e) e.textContent = t; }},
        done(v, ok, extra) {{
            const bar = document.getElementById('__td_bar');
            if (bar) {{ bar.style.animation = 'none'; bar.style.width = '100%'; }}
            const btn = document.getElementById('__td_stop');
            if (btn) {{ btn.textContent = 'Cerrar'; btn.onclick = () => document.getElementById('__td_hud')?.remove(); }}
            const sub = document.getElementById('__td_sub');
            if (sub) {{ sub.textContent = ok ? 'Enviado a la app ✓' : 'Terminado'; sub.style.color = ok ? '#3DDC84' : '#FFB454'; }}
            this.n(v);
            this.msg(extra || '');
        }}
    }};
}}

async function tdSend(items, hud) {{
    if (!items.length) return false;
    try {{
        const res = await fetch(`http://127.0.0.1:${{TD_PORT}}/add`, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ source: location.hostname, items }})
        }});
        if (res.ok) {{
            hud && hud.done(items.length, true, 'Ya están en la cola de descargas');
            console.log(`[TD] ✅ ${{items.length}} enlaces enviados a Todo Downloader`);
            return true;
        }}
        hud && hud.msg('El receptor respondió ' + res.status);
    }} catch (e) {{
        hud && hud.msg('App no encontrada — copiando al portapapeles');
        console.warn('[TD] Receptor no disponible:', e.message);
    }}
    return false;
}}

function tdFallbackCopy(items, hud) {{
    const txt = items.map(i => i.url).join('\n');
    navigator.clipboard.writeText(txt).then(
        () => {{
            hud && hud.done(items.length, false, '📋 Copiados al portapapeles');
            console.log('[TD] 📋 Enlaces copiados — el LinkGrabber los detectará');
        }},
        () => {{
            hud && hud.done(items.length, false, 'Copia manual desde la consola');
            console.log(txt);
        }}
    );
}}
"#
    )
}

/// Script para perfiles de TikTok (interceptor de API + auto-scroll)
pub fn tiktok(port: u16) -> String {
    format!(
        r#"/* Todo Downloader — Capturador de TikTok — By Eric V. Gramunt
   Ejecutar en https://www.tiktok.com/@usuario (F12 → Consola) */
(() => {{
{sender}
const items = new Map();
const API = /\/api\/(post|repost|favorite)\/item_list|\/api\/item\/detail/;

function add(it) {{
    if (!it || !it.id || items.has(it.id)) return;
    const v = it.video || {{}};
    const author = (it.author && it.author.uniqueId) || (location.pathname.match(/@([^/]+)/) || [])[1] || '';
    const q = (v.bitrateInfo || [])
        .map(b => ({{ br: b.Bitrate || 0, u: (b.PlayAddr && b.PlayAddr.UrlList || []).slice(-1)[0] || '' }}))
        .filter(x => x.u).sort((a, b) => b.br - a.br);
    let url = q.length ? q[0].u : (v.playAddr || v.downloadAddr || '');
    if (!url) return;
    if (url.startsWith('http://')) url = 'https://' + url.slice(7);
    items.set(it.id, {{ id: it.id, author, title: it.desc || '', url,
        pageUrl: `https://www.tiktok.com/@${{author}}/video/${{it.id}}` }});
}}

function ingest(d) {{
    if (!d) return;
    const list = d.itemList || d.items || (d.itemInfo && d.itemInfo.itemStruct ? [d.itemInfo.itemStruct] : []);
    list.forEach(add);
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
    console.log('[TD] Captura iniciada — sigue el progreso en el panel de la esquina');
    let idle = 0, last = 0;
    window.__tdStop = false;
    while (!window.__tdStop && idle < 8) {{
        window.scrollTo(0, document.body.scrollHeight);
        await new Promise(r => setTimeout(r, 1400));
        if (items.size === last) {{
            idle++;
            hud.msg(`Buscando más… (${{idle}}/8)`);
        }} else {{
            idle = 0;
            last = items.size;
            hud.msg('');
        }}
        hud.n(items.size);
    }}
    const arr = [...items.values()];
    hud.sub('Enviando…');
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
    format!(
        r#"/* Todo Downloader — Capturador de Douyin (vídeos + imágenes) — By Eric V. Gramunt
   Ejecutar en https://www.douyin.com/user/... (F12 → Consola) */
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
            items.push({{ id: key, author, title, url: u, pageUrl: `https://www.douyin.com/note/${{id}}` }});
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
        items.push({{ id, author, title, url: u, pageUrl: `https://www.douyin.com/video/${{id}}` }});
        vids++;
    }}
}}

(async () => {{
    if (!SEC || !location.pathname.includes('/user/')) {{
        alert('Todo Downloader: ejecuta esto en una página de perfil de Douyin (/user/...)');
        return;
    }}
    const hud = tdHud();
    hud.sub('Leyendo publicaciones…');
    console.log('[TD] Captura iniciada — sigue el progreso en el panel de la esquina');

    let cursor = 0, more = true, fails = 0, pages = 0;
    window.__tdStop = false;
    while (more && !window.__tdStop && fails < 5) {{
        try {{
            const d = await api(cursor);
            if (!d || !d.aweme_list) {{
                fails++;
                hud.msg(`Respuesta vacía, reintentando (${{fails}}/5)`);
                await new Promise(r => setTimeout(r, 2000));
                continue;
            }}
            fails = 0;
            pages++;
            d.aweme_list.forEach(collect);
            more = d.has_more === 1;
            cursor = d.max_cursor;
            hud.n(items.length);
            hud.lbl('archivos');
            hud.msg(`${{posts.size}} publicaciones · ${{vids}} vídeos · ${{imgs}} imágenes`);
            await new Promise(r => setTimeout(r, 1500));
        }} catch (e) {{
            fails++;
            hud.msg(`Reintentando (${{fails}}/5)…`);
            await new Promise(r => setTimeout(r, 3000));
        }}
    }}
    hud.sub('Enviando…');
    const resumen = `${{posts.size}} publicaciones → ${{vids}} vídeos + ${{imgs}} imágenes`;
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
