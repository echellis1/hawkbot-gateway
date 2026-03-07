use crate::config::{save_config, AppConfig, SharedConfig, CONFIG_PATH};
use crate::decoder::rtd_profile_for_sport_name;
use crate::mqtt::MqttPublisher;
use crate::schema::NormalizedScoreboardStatus;
use axum::extract::{Form, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use tokio::sync::watch;

const SUPPORTED_SPORTS: [&str; 5] = ["basketball", "volleyball", "football", "soccer", "lacrosse"];

#[derive(Clone)]
pub struct WebState {
    pub config: SharedConfig,
    pub config_tx: watch::Sender<AppConfig>,
    pub status_tx: watch::Sender<NormalizedScoreboardStatus>,
    pub status_rx: watch::Receiver<NormalizedScoreboardStatus>,
    pub mqtt: MqttPublisher,
}

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(get_index))
        .route("/status.json", get(get_status_json))
        .route("/%22/status.json/%22", get(get_status_json))
        .route("/\"/status.json/\"", get(get_status_json))
        .route("/admin", get(get_admin).post(post_admin))
        .route("/admin/simulate", axum::routing::post(post_admin_simulate))
        .route("/%22/admin/%22", get(get_admin).post(post_admin))
        .route("/\"/admin/\"", get(get_admin).post(post_admin))
        .with_state(state)
}

async fn get_index() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>Daktronics Gateway</title>
    <style>
      :root {
        color-scheme: dark;
        --bg: #0c1221;
        --surface: #101a30;
        --surface-2: #162340;
        --text: #ecf2ff;
        --muted: #93a4c3;
        --accent: #4d9eff;
        --accent-strong: #2e78d2;
        --border: rgba(147, 164, 195, 0.22);
      }

      * { box-sizing: border-box; }

      body {
        margin: 0;
        font-family: "Inter", "Segoe UI", Roboto, sans-serif;
        color: var(--text);
        background:
          radial-gradient(circle at top, rgba(77, 158, 255, 0.2), transparent 55%),
          var(--bg);
      }

      .app-shell {
        min-height: 100vh;
        display: grid;
        grid-template-rows: auto 1fr auto;
        gap: 1rem;
        padding: 1rem;
        max-width: 1100px;
        margin: 0 auto;
      }

      .app-header,
      .scoreboard-card,
      .info-grid,
      .app-footer {
        background: linear-gradient(160deg, var(--surface), var(--surface-2));
        border: 1px solid var(--border);
        border-radius: 1rem;
      }

      .app-header,
      .app-footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 1rem;
        flex-wrap: wrap;
      }

      .brand-title {
        margin: 0;
        font-size: clamp(1.1rem, 2.6vw, 1.5rem);
      }

      .brand-subtitle {
        margin: 0.25rem 0 0;
        color: var(--muted);
        font-size: 0.9rem;
      }

      .action-links {
        display: flex;
        gap: 0.6rem;
        flex-wrap: wrap;
      }

      .action-link {
        text-decoration: none;
        color: var(--text);
        background: rgba(77, 158, 255, 0.15);
        border: 1px solid rgba(77, 158, 255, 0.45);
        border-radius: 0.65rem;
        padding: 0.55rem 0.9rem;
        font-weight: 600;
        font-size: 0.9rem;
      }

      .action-link:hover {
        background: rgba(77, 158, 255, 0.25);
      }

      .scoreboard-card {
        padding: 1rem;
        display: grid;
        gap: 1rem;
      }

      .teams-grid {
        display: grid;
        grid-template-columns: 1fr auto 1fr;
        gap: 0.8rem;
        align-items: stretch;
      }

      .team-panel {
        background: rgba(12, 18, 33, 0.55);
        border: 1px solid var(--border);
        border-radius: 0.8rem;
        padding: 0.8rem;
        display: grid;
        gap: 0.35rem;
      }

      .team-label {
        color: var(--muted);
        font-size: 0.8rem;
        text-transform: uppercase;
        letter-spacing: 0.06em;
      }

      .team-name {
        font-size: clamp(1.15rem, 3.5vw, 1.65rem);
        font-weight: 700;
      }

      .score-value {
        font-size: clamp(2rem, 8vw, 3.3rem);
        font-weight: 800;
        line-height: 1;
      }

      .game-meta {
        min-width: 8rem;
        text-align: center;
        border-radius: 0.8rem;
        border: 1px solid var(--border);
        padding: 0.8rem;
        background: rgba(12, 18, 33, 0.5);
        display: grid;
        place-content: center;
        gap: 0.45rem;
      }

      .meta-label {
        color: var(--muted);
        text-transform: uppercase;
        letter-spacing: 0.08em;
        font-size: 0.72rem;
      }

      .meta-value {
        font-size: 1.15rem;
        font-weight: 700;
      }

      .info-grid {
        padding: 0.9rem;
        display: grid;
        gap: 0.7rem;
        grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
      }

      .info-chip {
        background: rgba(12, 18, 33, 0.55);
        border: 1px solid var(--border);
        border-radius: 0.7rem;
        padding: 0.7rem;
        display: grid;
        gap: 0.15rem;
      }

      .info-chip .label {
        color: var(--muted);
        font-size: 0.74rem;
        text-transform: uppercase;
        letter-spacing: 0.08em;
      }

      .info-chip .value {
        font-size: 1rem;
        font-weight: 700;
      }

      .footer-note {
        color: var(--muted);
        margin: 0;
        font-size: 0.85rem;
      }

      @media (max-width: 768px) {
        .teams-grid {
          grid-template-columns: 1fr;
        }

        .game-meta {
          order: -1;
        }

        .app-shell {
          padding: 0.75rem;
        }
      }
    </style>
  </head>
  <body>
    <div class="app-shell">
      <header class="app-header">
        <div>
          <h1 class="brand-title">Daktronics Gateway Dashboard</h1>
          <p class="brand-subtitle">Live scoreboard relay and control surface</p>
        </div>
        <nav class="action-links" aria-label="Primary actions">
          <a class="action-link" href="/status.json">View Status JSON</a>
          <a class="action-link" href="/admin">Open Admin Panel</a>
        </nav>
      </header>

      <main>
        <section class="scoreboard-card" aria-label="Primary scoreboard">
          <div class="teams-grid">
            <article class="team-panel">
              <span class="team-label">Home</span>
              <span class="team-name">Home Team</span>
              <span class="score-value">72</span>
            </article>

            <aside class="game-meta" aria-label="Game clock and period">
              <span class="meta-label">Period</span>
              <span class="meta-value">Q4</span>
              <span class="meta-label">Clock</span>
              <span class="meta-value">02:14</span>
            </aside>

            <article class="team-panel">
              <span class="team-label">Away</span>
              <span class="team-name">Away Team</span>
              <span class="score-value">68</span>
            </article>
          </div>

          <div class="info-grid" aria-label="Secondary game metadata">
            <div class="info-chip"><span class="label">Home Timeouts</span><span class="value">2</span></div>
            <div class="info-chip"><span class="label">Away Timeouts</span><span class="value">1</span></div>
            <div class="info-chip"><span class="label">Home Fouls</span><span class="value">4</span></div>
            <div class="info-chip"><span class="label">Away Fouls</span><span class="value">3</span></div>
            <div class="info-chip"><span class="label">Possession</span><span class="value">Home</span></div>
          </div>
        </section>
      </main>

      <footer class="app-footer">
        <p class="footer-note">Gateway is running and ready for sport-specific overlays.</p>
        <div class="action-links" aria-label="Footer actions">
          <a class="action-link" href="/status.json">Status Feed</a>
          <a class="action-link" href="/admin">Settings</a>
        </div>
      </footer>
    </div>
  </body>
</html>"#,
    )
}

async fn get_status_json(State(state): State<WebState>) -> Json<NormalizedScoreboardStatus> {
    Json(state.status_rx.borrow().clone())
}

async fn get_admin(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers).await {
        return unauthorized();
    }
    let cfg = state.config.read().await.clone();
    Html(render_admin_page(&cfg)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct AdminForm {
    controller_type: String,
    sport_type: String,
    serial_device: String,
    mqtt_host: String,
    mqtt_port: u16,
    mqtt_topic: String,
    publish_interval_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct SimulateForm {
    sport_type: String,
}

async fn post_admin(
    State(state): State<WebState>,
    headers: HeaderMap,
    Form(form): Form<AdminForm>,
) -> Response {
    if !authorized(&state, &headers).await {
        return unauthorized();
    }

    let mut cfg = state.config.read().await.clone();
    cfg.controller_type = form.controller_type;
    cfg.sport_type = form.sport_type;
    cfg.serial_device = form.serial_device;
    cfg.mqtt_host = form.mqtt_host;
    cfg.mqtt_port = form.mqtt_port;
    cfg.mqtt_topic = form.mqtt_topic;
    cfg.publish_interval_ms = form.publish_interval_ms;

    if let Err(err) = save_config(CONFIG_PATH, &cfg).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save config: {err}"),
        )
            .into_response();
    }

    {
        let mut guard = state.config.write().await;
        *guard = cfg.clone();
    }

    let _ = state.config_tx.send(cfg.clone());

    let mut current_status = state.status_rx.borrow().clone();
    current_status.controller_type = cfg.controller_type.clone();
    current_status.sport_type = cfg.sport_type.clone();
    current_status.timestamp_rfc3339 = chrono::Utc::now().to_rfc3339();
    current_status.extras = serde_json::json!({
        "rtd_profile": rtd_profile_for_sport_name(&cfg.sport_type),
    });
    let _ = state.status_tx.send(current_status);

    let mqtt = state.mqtt.clone();
    let cfg_for_publish = cfg.clone();
    tokio::spawn(async move {
        mqtt.publish_config(&cfg_for_publish).await;
    });

    Html(render_admin_page(&cfg)).into_response()
}

async fn post_admin_simulate(
    State(state): State<WebState>,
    headers: HeaderMap,
    Form(form): Form<SimulateForm>,
) -> Response {
    if !authorized(&state, &headers).await {
        return unauthorized();
    }

    let cfg = state.config.read().await.clone();
    let sport = if SUPPORTED_SPORTS.contains(&form.sport_type.as_str()) {
        form.sport_type.as_str()
    } else {
        cfg.sport_type.as_str()
    };

    let payload = simulated_status_for_sport(&cfg.controller_type, sport);
    let _ = state.status_tx.send(payload.clone());

    let mqtt = state.mqtt.clone();
    let topic = cfg.mqtt_topic.clone();
    let retain = cfg.mqtt_retain;
    tokio::spawn(async move {
        let _ = mqtt.publish_json(&topic, &payload, retain).await;
    });

    Html(render_admin_page(&cfg)).into_response()
}

async fn authorized(state: &WebState, headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value_str) = value.to_str() else {
        return false;
    };
    let Some(encoded) = value_str.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = STANDARD.decode(encoded) else {
        return false;
    };
    let Ok(decoded_str) = String::from_utf8(decoded) else {
        return false;
    };

    let cfg = state.config.read().await;
    decoded_str == format!("{}:{}", cfg.admin_user, cfg.admin_pass)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=admin")],
        "Unauthorized",
    )
        .into_response()
}

fn render_admin_page(cfg: &AppConfig) -> String {
    let controller_type_select =
        render_select("controller_type", &cfg.controller_type, &["all_sport_5000"]);
    let sport_type_select = render_select("sport_type", &cfg.sport_type, &SUPPORTED_SPORTS);

    let simulation_buttons = SUPPORTED_SPORTS
        .iter()
        .map(|sport| {
            format!(r#"<button class="btn btn-secondary" type="submit" name="sport_type" value="{sport}">{sport}</button>"#)
        })
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>Scoreboard Admin</title>
    <style>
      :root {{
        color-scheme: dark;
        --bg: #0c1221;
        --surface: #101a30;
        --surface-2: #162340;
        --text: #ecf2ff;
        --muted: #93a4c3;
        --accent: #4d9eff;
        --accent-strong: #2e78d2;
        --border: rgba(147, 164, 195, 0.22);
      }}

      * {{ box-sizing: border-box; }}

      body {{
        margin: 0;
        font-family: "Inter", "Segoe UI", Roboto, sans-serif;
        color: var(--text);
        background:
          radial-gradient(circle at top, rgba(77, 158, 255, 0.2), transparent 55%),
          var(--bg);
      }}

      .app-shell {{
        min-height: 100vh;
        display: grid;
        grid-template-rows: auto 1fr auto;
        gap: 1rem;
        padding: 1rem;
        max-width: 1100px;
        margin: 0 auto;
      }}

      .app-header,
      .app-footer,
      .section-card {{
        background: linear-gradient(160deg, var(--surface), var(--surface-2));
        border: 1px solid var(--border);
        border-radius: 1rem;
      }}

      .app-header,
      .app-footer {{
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 1rem;
        flex-wrap: wrap;
      }}

      .brand-title {{
        margin: 0;
        font-size: clamp(1.1rem, 2.6vw, 1.5rem);
      }}

      .brand-subtitle {{
        margin: 0.25rem 0 0;
        color: var(--muted);
        font-size: 0.9rem;
      }}

      .action-links {{
        display: flex;
        gap: 0.6rem;
        flex-wrap: wrap;
      }}

      .action-link {{
        text-decoration: none;
        color: var(--text);
        background: rgba(77, 158, 255, 0.15);
        border: 1px solid rgba(77, 158, 255, 0.45);
        border-radius: 0.65rem;
        padding: 0.55rem 0.9rem;
        font-weight: 600;
        font-size: 0.9rem;
      }}

      .action-link:hover {{
        background: rgba(77, 158, 255, 0.25);
      }}

      .admin-main {{
        display: grid;
        gap: 1rem;
      }}

      .settings-grid {{
        display: grid;
        gap: 1rem;
        grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
      }}

      .section-card {{
        padding: 1rem;
        display: grid;
        gap: 0.85rem;
      }}

      .section-title {{
        margin: 0;
        font-size: 1rem;
      }}

      .section-description {{
        margin: 0;
        color: var(--muted);
        font-size: 0.85rem;
      }}

      .form-grid {{
        display: grid;
        gap: 0.75rem;
      }}

      .form-group {{
        display: grid;
        gap: 0.35rem;
      }}

      .form-group label {{
        color: var(--muted);
        font-size: 0.85rem;
        font-weight: 600;
      }}

      input,
      select {{
        width: 100%;
        border-radius: 0.65rem;
        border: 1px solid var(--border);
        background: rgba(12, 18, 33, 0.55);
        color: var(--text);
        padding: 0.55rem 0.65rem;
      }}

      .button-row {{
        display: flex;
        flex-wrap: wrap;
        gap: 0.6rem;
      }}

      .btn {{
        border: 1px solid transparent;
        border-radius: 0.65rem;
        padding: 0.6rem 0.95rem;
        font-weight: 700;
        color: var(--text);
        cursor: pointer;
      }}

      .btn-primary {{
        background: linear-gradient(180deg, var(--accent), var(--accent-strong));
        border-color: rgba(77, 158, 255, 0.7);
      }}

      .btn-secondary {{
        background: rgba(77, 158, 255, 0.15);
        border-color: rgba(77, 158, 255, 0.45);
        text-transform: capitalize;
      }}

      .footer-note {{
        color: var(--muted);
        margin: 0;
        font-size: 0.85rem;
      }}

      @media (max-width: 768px) {{
        .app-shell {{
          padding: 0.75rem;
        }}
      }}
    </style>
  </head>
  <body>
    <div class="app-shell">
      <header class="app-header">
        <div>
          <h1 class="brand-title">Daktronics Gateway Admin</h1>
          <p class="brand-subtitle">Configuration and simulation controls</p>
        </div>
        <nav class="action-links" aria-label="Primary actions">
          <a class="action-link" href="/">Open Dashboard</a>
          <a class="action-link" href="/status.json">View Status JSON</a>
        </nav>
      </header>

      <main class="admin-main">
        <form method="post" action="/admin" class="settings-grid">
          <section class="section-card" aria-label="Connection settings">
            <h2 class="section-title">Connection settings</h2>
            <p class="section-description">Serial feed and MQTT broker details.</p>
            <div class="form-grid">
              <div class="form-group">
                <label for="serial_device">Serial Device</label>
                <input id="serial_device" name="serial_device" value="{}"/>
              </div>
              <div class="form-group">
                <label for="mqtt_host">MQTT Host</label>
                <input id="mqtt_host" name="mqtt_host" value="{}"/>
              </div>
              <div class="form-group">
                <label for="mqtt_port">MQTT Port</label>
                <input id="mqtt_port" name="mqtt_port" type="number" value="{}"/>
              </div>
            </div>
          </section>

          <section class="section-card" aria-label="Sport and controller settings">
            <h2 class="section-title">Sport/controller settings</h2>
            <p class="section-description">Select decoder profile and controller family.</p>
            <div class="form-grid">
              <div class="form-group">
                <label for="controller_type">Controller Type</label>
                {}
              </div>
              <div class="form-group">
                <label for="sport_type">Sport Type</label>
                {}
              </div>
            </div>
          </section>

          <section class="section-card" aria-label="Publish settings">
            <h2 class="section-title">Publish settings</h2>
            <p class="section-description">Topic and cadence used for outbound updates.</p>
            <div class="form-grid">
              <div class="form-group">
                <label for="mqtt_topic">MQTT Topic</label>
                <input id="mqtt_topic" name="mqtt_topic" value="{}"/>
              </div>
              <div class="form-group">
                <label for="publish_interval_ms">Publish Interval (ms)</label>
                <input id="publish_interval_ms" name="publish_interval_ms" type="number" value="{}"/>
              </div>
            </div>
          </section>

          <section class="section-card" aria-label="Save settings">
            <h2 class="section-title">Save configuration</h2>
            <p class="section-description">Apply changes to the active runtime config.</p>
            <div class="button-row">
              <button class="btn btn-primary" type="submit">Save</button>
            </div>
          </section>
        </form>

        <section class="section-card" aria-label="Simulation controls">
          <h2 class="section-title">Simulation controls</h2>
          <p class="section-description">Publish sample sport payloads for testing without serial input.</p>
          <form method="post" action="/admin/simulate">
            <div class="button-row">
              {}
            </div>
          </form>
        </section>
      </main>

      <footer class="app-footer">
        <p class="footer-note">Admin actions preserve existing backend routes and field names.</p>
      </footer>
    </div>
  </body>
</html>"#,
        cfg.serial_device,
        cfg.mqtt_host,
        cfg.mqtt_port,
        controller_type_select,
        sport_type_select,
        cfg.mqtt_topic,
        cfg.publish_interval_ms,
        simulation_buttons
    )
}

fn simulated_status_for_sport(
    controller_type: &str,
    sport_type: &str,
) -> NormalizedScoreboardStatus {
    let (segment_kind, segment_number, clock_main, home_score, away_score, extras) =
        match sport_type {
            "volleyball" => (
                "set",
                3,
                "00:00",
                21,
                18,
                serde_json::json!({"sets_home": 2, "sets_away": 1, "serving": "home"}),
            ),
            "football" => (
                "quarter",
                4,
                "02:14",
                28,
                24,
                serde_json::json!({"down": 3, "to_go": 7, "ball_on": 42}),
            ),
            "soccer" => (
                "half",
                2,
                "67:33",
                2,
                1,
                serde_json::json!({"shots_home": 11, "shots_away": 8, "fouls_home": 5, "fouls_away": 7}),
            ),
            "lacrosse" => (
                "quarter",
                3,
                "03:51",
                10,
                9,
                serde_json::json!({"penalties_home": 1, "penalties_away": 2}),
            ),
            _ => (
                "period",
                2,
                "08:45",
                56,
                49,
                serde_json::json!({"fouls_home": 3, "fouls_away": 4, "bonus_home": true, "bonus_away": false}),
            ),
        };

    NormalizedScoreboardStatus {
        schema_version: 1,
        timestamp_rfc3339: chrono::Utc::now().to_rfc3339(),
        controller_type: controller_type.to_string(),
        sport_type: sport_type.to_string(),
        clock_main: Some(clock_main.to_string()),
        clock_secondary: Some("24".to_string()),
        segment_kind: Some(segment_kind.to_string()),
        segment_number: Some(segment_number),
        segment_text: None,
        home_score: Some(home_score),
        away_score: Some(away_score),
        home_timeouts: Some(2),
        away_timeouts: Some(1),
        possession: Some("home".to_string()),
        extras: serde_json::json!({
            "mode": "simulated",
            "rtd_profile": rtd_profile_for_sport_name(sport_type),
            "sport_specific": extras,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{render_admin_page, simulated_status_for_sport};
    use crate::config::AppConfig;

    #[test]
    fn admin_form_uses_valid_html_attributes() {
        let cfg = AppConfig::default();
        let html = render_admin_page(&cfg);

        assert!(html.contains("<form method=\"post\" action=\"/admin\" class=\"settings-grid\">"));
        assert!(!html.contains("\\\""));
        assert!(html.contains("action=\"/admin/simulate\""));
    }

    #[test]
    fn simulation_payload_includes_selected_sport_profile() {
        let payload = simulated_status_for_sport("all_sport_5000", "football");

        assert_eq!(payload.sport_type, "football");
        assert_eq!(payload.segment_kind.as_deref(), Some("quarter"));
        assert_eq!(payload.extras["mode"], "simulated");
        assert_eq!(payload.extras["rtd_profile"], "rtd_football");
    }
}

fn render_select(name: &str, selected: &str, options: &[&str]) -> String {
    let mut html = format!("<select id=\"{name}\" name=\"{name}\">");

    for option in options {
        if *option == selected {
            html.push_str(&format!(
                "<option value=\"{option}\" selected>{option}</option>"
            ));
        } else {
            html.push_str(&format!("<option value=\"{option}\">{option}</option>"));
        }
    }

    if !options.contains(&selected) {
        html.push_str(&format!(
            "<option value=\"{selected}\" selected>{selected}</option>"
        ));
    }

    html.push_str("</select>");
    html
}
