use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde::Serialize;

use crate::config::UpstreamSetupUpdate;
use crate::gateway::api::routes::AppState;

pub async fn setup_page() -> Html<&'static str> {
    Html(SETUP_HTML)
}

pub async fn setup_get(State(state): State<AppState>) -> impl IntoResponse {
    match state.config_mgr.setup_view() {
        Ok(view) => Json(view).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn setup_init(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(resp) = require_admin(&state, &headers) {
        return resp;
    }
    match state.config_mgr.write_default_setup() {
        Ok(view) => Json(SetupResponse {
            ok: true,
            message: "default upstream setup applied (cloud model=auto, edge empty)",
            upstream: view,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn setup_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<UpstreamSetupUpdate>,
) -> Response {
    if let Some(resp) = require_admin(&state, &headers) {
        return resp;
    }
    match state.config_mgr.apply_setup(&patch) {
        Ok(view) => Json(SetupResponse {
            ok: true,
            message: "upstream setup updated",
            upstream: view,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct SetupResponse {
    ok: bool,
    message: &'static str,
    upstream: crate::config::UpstreamSetupView,
}

fn require_admin(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let config = state.config_mgr.get();
    let Some(expected) = config.admin_token.as_ref() else {
        return None;
    };
    let provided = headers
        .get("x-flowy-admin-token")
        .and_then(|v| v.to_str().ok());
    if provided == Some(expected.as_str()) {
        return None;
    }
    Some(
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid admin token"})),
        )
            .into_response(),
    )
}

const SETUP_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Flowy Router — 上游模型设置</title>
  <style>
    :root { color-scheme: light dark; font-family: system-ui, sans-serif; }
    body { max-width: 720px; margin: 2rem auto; padding: 0 1rem; line-height: 1.5; }
    h1 { font-size: 1.35rem; }
    fieldset { border: 1px solid #8884; border-radius: 8px; margin: 1rem 0; padding: 1rem; }
    legend { padding: 0 0.4rem; font-weight: 600; }
    label { display: block; margin: 0.6rem 0 0.2rem; font-size: 0.9rem; }
    input { width: 100%; box-sizing: border-box; padding: 0.45rem 0.55rem; border-radius: 6px; border: 1px solid #8886; }
    .row { display: flex; gap: 0.75rem; flex-wrap: wrap; }
    button { margin-top: 1rem; margin-right: 0.5rem; padding: 0.5rem 1rem; border-radius: 6px; border: 1px solid #8886; cursor: pointer; }
    #status { margin-top: 1rem; white-space: pre-wrap; font-size: 0.9rem; }
    .hint { color: #888; font-size: 0.85rem; }
  </style>
</head>
<body>
  <h1>Flowy Router — 上游模型设置</h1>
  <p class="hint">云端默认 model=<code>auto</code>（保留客户端模型名）；端侧默认可留空。保存后立即生效，无需重启 Gateway。</p>
  <label for="admin_token">Admin Token（若 config 中配置了 admin_token）</label>
  <input id="admin_token" type="password" placeholder="X-Flowy-Admin-Token" autocomplete="off" />

  <fieldset>
    <legend>云端 Cloud</legend>
    <label for="cloud_url">Base URL（OpenAI 兼容，含 /v1）</label>
    <input id="cloud_url" placeholder="https://api.deepseek.com/v1" />
    <label for="cloud_model">Model</label>
    <input id="cloud_model" placeholder="auto" />
    <label for="cloud_key">API Key</label>
    <input id="cloud_key" type="password" placeholder="留空则不修改已保存的 key" autocomplete="off" />
  </fieldset>

  <fieldset>
    <legend>端侧 Edge</legend>
    <label for="edge_url">Base URL</label>
    <input id="edge_url" placeholder="http://127.0.0.1:11434/v1" />
    <label for="edge_model">Model（可选，空=auto）</label>
    <input id="edge_model" placeholder="" />
    <label for="edge_key">API Key</label>
    <input id="edge_key" type="password" placeholder="留空则不修改" autocomplete="off" />
    <label><input id="edge_clear" type="checkbox" /> 清除端侧配置</label>
  </fieldset>

  <div class="row">
    <button type="button" id="load">加载当前配置</button>
    <button type="button" id="defaults">恢复默认（cloud=auto，edge 空）</button>
    <button type="button" id="save">保存</button>
  </div>
  <div id="status"></div>

  <script>
    const status = document.getElementById('status');
    function headers() {
      const h = { 'Content-Type': 'application/json' };
      const t = document.getElementById('admin_token').value.trim();
      if (t) h['X-Flowy-Admin-Token'] = t;
      return h;
    }
    function fill(view) {
      const cloud = view.cloud || {};
      const edge = view.edge || {};
      document.getElementById('cloud_url').value = cloud.base_url || '';
      document.getElementById('cloud_model').value = cloud.model || 'auto';
      document.getElementById('edge_url').value = edge.base_url || '';
      document.getElementById('edge_model').value = edge.model || '';
      document.getElementById('edge_clear').checked = false;
      document.getElementById('cloud_key').value = '';
      document.getElementById('edge_key').value = '';
    }
    async function load() {
      status.textContent = 'Loading…';
      try {
        const r = await fetch('/v1/admin/setup');
        const j = await r.json();
        if (!r.ok) throw new Error(j.error || r.statusText);
        fill(j);
        status.textContent = 'Loaded.';
      } catch (e) { status.textContent = 'Error: ' + e.message; }
    }
    async function defaults() {
      status.textContent = 'Applying defaults…';
      try {
        const r = await fetch('/v1/admin/setup/init', { method: 'POST', headers: headers() });
        const j = await r.json();
        if (!r.ok) throw new Error(j.error || r.statusText);
        fill(j.upstream);
        status.textContent = j.message || 'OK';
      } catch (e) { status.textContent = 'Error: ' + e.message; }
    }
    async function save() {
      status.textContent = 'Saving…';
      const body = {
        cloud: {
          base_url: document.getElementById('cloud_url').value,
          model: document.getElementById('cloud_model').value || 'auto',
        },
        edge: {
          clear: document.getElementById('edge_clear').checked,
          base_url: document.getElementById('edge_url').value,
          model: document.getElementById('edge_model').value || null,
        }
      };
      const ck = document.getElementById('cloud_key').value;
      const ek = document.getElementById('edge_key').value;
      if (ck) body.cloud.api_key = ck;
      if (ek) body.edge.api_key = ek;
      try {
        const r = await fetch('/v1/admin/setup', { method: 'POST', headers: headers(), body: JSON.stringify(body) });
        const j = await r.json();
        if (!r.ok) throw new Error(j.error || r.statusText);
        fill(j.upstream);
        status.textContent = j.message || 'Saved.';
      } catch (e) { status.textContent = 'Error: ' + e.message; }
    }
    document.getElementById('load').onclick = load;
    document.getElementById('defaults').onclick = defaults;
    document.getElementById('save').onclick = save;
    load();
  </script>
</body>
</html>
"#;
