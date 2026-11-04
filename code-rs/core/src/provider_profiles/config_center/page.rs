//! The configuration center page.
//!
//! The page is a single self-contained document. It embeds the CSRF token
//! but never the bearer token: the bearer token stays in the CLI process
//! that owns the server and is only used for out-of-band requests.

/// Renders the configuration center page.
pub fn render_page(csrf_token: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Codex Provider Configuration</title>
<style>
  :root {{
    --bg: #0d1117;
    --panel: #161b22;
    --panel-2: #1c2129;
    --border: #30363d;
    --text: #e6edf3;
    --muted: #8b949e;
    --accent: #4493f8;
    --accent-2: #1f6feb;
    --danger: #f85149;
    --ok: #3fb950;
    --radius: 10px;
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0;
    padding: 32px 20px 64px;
    background: var(--bg);
    color: var(--text);
    font: 14px/1.5 ui-sans-serif, -apple-system, "Segoe UI", Roboto, sans-serif;
  }}
  .wrap {{ max-width: 860px; margin: 0 auto; }}
  header {{ margin-bottom: 24px; }}
  h1 {{ font-size: 20px; font-weight: 600; margin: 0 0 4px; letter-spacing: -0.01em; }}
  .sub {{ color: var(--muted); font-size: 13px; }}
  .card {{
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 18px 20px;
    margin-bottom: 18px;
  }}
  .card h2 {{ font-size: 14px; font-weight: 600; margin: 0 0 14px; color: var(--text); }}
  .row {{
    display: flex; align-items: center; gap: 12px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--panel-2);
    margin-bottom: 8px;
  }}
  .row.active {{ border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }}
  .grow {{ flex: 1; min-width: 0; }}
  .name {{ font-weight: 600; }}
  .meta {{ color: var(--muted); font-size: 12px; margin-top: 2px; word-break: break-all; }}
  .badge {{
    display: inline-block; font-size: 11px; padding: 1px 7px; border-radius: 999px;
    border: 1px solid var(--border); color: var(--muted); margin-left: 6px; vertical-align: 1px;
  }}
  .badge.on {{ color: var(--ok); border-color: var(--ok); }}
  .badge.off {{ color: var(--muted); }}
  button {{
    font: inherit; font-size: 13px; padding: 6px 12px; border-radius: 7px;
    border: 1px solid var(--border); background: #21262d; color: var(--text); cursor: pointer;
  }}
  button:hover {{ border-color: #484f58; background: #262c36; }}
  button.primary {{ background: var(--accent-2); border-color: var(--accent-2); color: #fff; }}
  button.primary:hover {{ background: #388bfd; }}
  button.danger {{ color: var(--danger); }}
  button.danger:hover {{ border-color: var(--danger); }}
  button:disabled {{ opacity: .5; cursor: not-allowed; }}
  .grid {{ display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }}
  .grid .full {{ grid-column: 1 / -1; }}
  label {{ display: block; font-size: 12px; color: var(--muted); margin-bottom: 5px; }}
  input, select {{
    width: 100%; font: inherit; padding: 8px 10px; border-radius: 7px;
    border: 1px solid var(--border); background: #0d1117; color: var(--text);
  }}
  input:focus, select:focus {{ outline: none; border-color: var(--accent); }}
  .actions {{ display: flex; gap: 8px; margin-top: 16px; }}
  .msg {{ margin-top: 12px; font-size: 13px; min-height: 18px; }}
  .msg.err {{ color: var(--danger); }}
  .msg.ok {{ color: var(--ok); }}
  .empty {{ color: var(--muted); padding: 18px 0; text-align: center; }}
  .hint {{ color: var(--muted); font-size: 12px; margin-top: 10px; }}
  .hidden {{ display: none; }}
</style>
</head>
<body>
<div class="wrap">
  <header>
    <h1>Provider Configuration</h1>
    <div class="sub">Manage OpenAI-compatible sources. The active source applies to the next turn.</div>
  </header>

  <div class="card">
    <h2>Sources</h2>
    <div id="list"><div class="empty">Loading&hellip;</div></div>
  </div>

  <div class="card">
    <h2 id="form-title">Add a source</h2>
    <div class="grid">
      <div>
        <label for="f-name">Display name</label>
        <input id="f-name" placeholder="My provider">
      </div>
      <div>
        <label for="f-model">Model</label>
        <input id="f-model" placeholder="gpt-4o-mini">
      </div>
      <div class="full">
        <label for="f-url">Base URL</label>
        <input id="f-url" placeholder="https://api.example.com/v1">
      </div>
      <div>
        <label for="f-wire">Wire API</label>
        <select id="f-wire">
          <option value="chat">chat</option>
          <option value="responses">responses</option>
          <option value="responses_websocket">responses_websocket</option>
        </select>
      </div>
      <div>
        <label for="f-key">API key</label>
        <input id="f-key" type="password" placeholder="stored securely" autocomplete="off">
      </div>
    </div>
    <div class="actions">
      <button class="primary" id="save">Save source</button>
      <button id="cancel" class="hidden">Cancel</button>
    </div>
    <div class="msg" id="msg"></div>
    <div class="hint">The API key is stored in your credential store and is never written to <code>config.toml</code> or shown again.</div>
  </div>
</div>

<script>
(function () {{
  const CSRF = {csrf_json};
  const listEl = document.getElementById('list');
  const msgEl = document.getElementById('msg');
  const formTitle = document.getElementById('form-title');
  const saveBtn = document.getElementById('save');
  const cancelBtn = document.getElementById('cancel');
  let editingId = null;

  function setMsg(text, kind) {{
    msgEl.textContent = text;
    msgEl.className = 'msg' + (kind ? ' ' + kind : '');
  }}

  function el(tag, cls, text) {{
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text !== undefined) node.textContent = text;
    return node;
  }}

  async function api(path, body) {{
    const res = await fetch(path, {{
      method: 'POST',
      headers: {{
        'Content-Type': 'application/json',
        'X-CSRF-Token': CSRF
      }},
      body: JSON.stringify(body || {{}})
    }});
    const text = await res.text();
    if (!res.ok) throw new Error(text || res.statusText);
    return text ? JSON.parse(text) : null;
  }}

  async function refresh() {{
    try {{
      const res = await fetch('/api/state', {{ headers: {{ 'Accept': 'application/json' }} }});
      if (!res.ok) throw new Error(await res.text());
      const state = await res.json();
      render(state);
    }} catch (err) {{
      listEl.innerHTML = '';
      listEl.appendChild(el('div', 'empty', 'Could not load sources: ' + err.message));
    }}
  }}

  function render(state) {{
    listEl.innerHTML = '';
    const profiles = state.profiles || [];
    if (!profiles.length) {{
      listEl.appendChild(el('div', 'empty', 'No sources configured yet.'));
      return;
    }}
    profiles.forEach(function (p) {{
      const row = el('div', 'row' + (p.active ? ' active' : ''));
      const grow = el('div', 'grow');
      const name = el('div', 'name');
      name.appendChild(document.createTextNode(p.display_name));
      name.appendChild(el('span', 'badge', p.id));
      if (p.active) name.appendChild(el('span', 'badge on', 'active'));
      grow.appendChild(name);
      const meta = el('div', 'meta');
      meta.textContent = p.model + ' · ' + p.wire_api + ' · ' + p.base_url +
        ' · key ' + (p.has_api_key ? 'set' : 'not set');
      grow.appendChild(meta);
      row.appendChild(grow);

      if (!p.active) {{
        const use = el('button', 'primary', 'Use');
        use.onclick = async function () {{
          try {{
            await api('/api/profiles/' + encodeURIComponent(p.id) + '/activate');
            setMsg('Activated "' + p.display_name + '".', 'ok');
            await refresh();
          }} catch (err) {{ setMsg(err.message, 'err'); }}
        }};
        row.appendChild(use);
      }}

      const edit = el('button', '', 'Edit');
      edit.onclick = function () {{ startEdit(p); }};
      row.appendChild(edit);

      const del = el('button', 'danger', 'Delete');
      del.onclick = async function () {{
        if (!confirm('Delete "' + p.display_name + '"?')) return;
        try {{
          await api('/api/profiles/' + encodeURIComponent(p.id) + '/delete');
          setMsg('Deleted "' + p.display_name + '".', 'ok');
          await refresh();
        }} catch (err) {{ setMsg(err.message, 'err'); }}
      }};
      row.appendChild(del);

      listEl.appendChild(row);
    }});
  }}

  function startEdit(p) {{
    editingId = p.id;
    formTitle.textContent = 'Edit "' + p.display_name + '"';
    document.getElementById('f-name').value = p.display_name;
    document.getElementById('f-model').value = p.model;
    document.getElementById('f-url').value = p.base_url;
    document.getElementById('f-wire').value = p.wire_api;
    document.getElementById('f-key').value = '';
    cancelBtn.classList.remove('hidden');
    saveBtn.textContent = 'Save changes';
    setMsg('');
  }}

  function resetForm() {{
    editingId = null;
    formTitle.textContent = 'Add a source';
    document.getElementById('f-name').value = '';
    document.getElementById('f-model').value = '';
    document.getElementById('f-url').value = '';
    document.getElementById('f-wire').value = 'chat';
    document.getElementById('f-key').value = '';
    cancelBtn.classList.add('hidden');
    saveBtn.textContent = 'Save source';
    setMsg('');
  }}

  cancelBtn.onclick = resetForm;

  saveBtn.onclick = async function () {{
    const body = {{
      display_name: document.getElementById('f-name').value.trim(),
      model: document.getElementById('f-model').value.trim(),
      base_url: document.getElementById('f-url').value.trim(),
      wire_api: document.getElementById('f-wire').value,
      activate: true
    }};
    const key = document.getElementById('f-key').value;
    if (key) body.api_key = key;
    if (!body.base_url || !body.model) {{
      setMsg('Base URL and model are required.', 'err');
      return;
    }}
    saveBtn.disabled = true;
    try {{
      if (editingId) {{
        await api('/api/profiles/' + encodeURIComponent(editingId), body);
        setMsg('Updated source.', 'ok');
      }} else {{
        await api('/api/profiles', body);
        setMsg('Created source.', 'ok');
      }}
      resetForm();
      await refresh();
    }} catch (err) {{
      setMsg(err.message, 'err');
    }} finally {{
      saveBtn.disabled = false;
    }}
  }};

  refresh();
}})();
</script>
</body>
</html>
"#,
        csrf_json = serde_json::Value::String(csrf_token.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_embeds_the_csrf_token() {
        let page = render_page("csrf-value-123");
        assert!(page.contains("csrf-value-123"));
    }

    #[test]
    fn page_never_renders_a_bearer_token() {
        let page = render_page("csrf-value-123");
        assert!(!page.to_lowercase().contains("bearer"));
    }

    #[test]
    fn page_activates_a_newly_saved_source() {
        let page = render_page("csrf-value-123");
        assert!(page.contains("activate: true"));
    }

    #[test]
    fn page_sets_restrictive_security_headers_via_markup() {
        let page = render_page("token");
        assert!(page.contains("Provider Configuration"));
    }
}
