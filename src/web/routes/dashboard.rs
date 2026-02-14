use askama::Template;
use axum::{extract::State, response::Html};
use log::{info, debug};
use std::collections::HashMap;

use crate::web::AppState;
use crate::web::auth::AuthAdmin;

fn is_catch_all(source: &str, domain: Option<&str>) -> bool {
    let normalized = source.trim().to_ascii_lowercase();
    if normalized == "*" || normalized.starts_with("*@") {
        return true;
    }
    if let Some(domain) = domain {
        let d = domain.to_ascii_lowercase();
        if normalized == d || normalized == format!("@{}", d) {
            return true;
        }
    }
    false
}

// ── View models ──

struct DomainCard {
    id: i64,
    domain: String,
    active_label: String,
    dkim_label: String,
    catch_all_label: String,
    footer_label: String,
}

struct CatchAllRow {
    domain: String,
    status: String,
    destination: String,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    hostname: &'a str,
    stats: crate::db::Stats,
    domain_cards: Vec<DomainCard>,
    catch_rows: Vec<CatchAllRow>,
}

pub async fn page(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET / — dashboard requested");
    let stats = state.db.get_stats();
    debug!(
        "[web] dashboard stats: domains={}, accounts={}, aliases={}, tracked={}, opens={}",
        stats.domain_count, stats.account_count, stats.alias_count,
        stats.tracked_count, stats.open_count
    );

    let domains = state.db.list_domains();
    let aliases = state.db.list_all_aliases_with_domain();

    // Build catch-all map
    let mut catch_all_map: HashMap<i64, (String, bool)> = HashMap::new();
    for alias in &aliases {
        if is_catch_all(&alias.source, alias.domain_name.as_deref()) {
            catch_all_map.insert(alias.domain_id, (alias.destination.clone(), alias.active));
        }
    }

    // Build domain cards
    let domain_cards: Vec<DomainCard> = domains.iter().map(|d| {
        let active_label = if d.active { "Accepting mail" } else { "Suspended" }.to_string();
        let dkim_label = if d.dkim_public_key.is_some() {
            format!("Selector {} ready", d.dkim_selector)
        } else {
            "Missing DKIM key".to_string()
        };
        let catch_all_label = match catch_all_map.get(&d.id) {
            Some((dest, true)) => format!("Catch-all → {}", dest),
            Some((dest, false)) => format!("Catch-all disabled ({})", dest),
            None => "No catch-all alias".to_string(),
        };
        let footer_label = match d.footer_html.as_deref() {
            Some(html) if !html.trim().is_empty() => "Footer injected",
            _ => "No footer",
        }.to_string();
        DomainCard { id: d.id, domain: d.domain.clone(), active_label, dkim_label, catch_all_label, footer_label }
    }).collect();

    // Build catch-all rows
    let catch_rows: Vec<CatchAllRow> = domains.iter().map(|d| {
        let (status, destination) = match catch_all_map.get(&d.id) {
            Some((dest, true)) => ("Protected".to_string(), dest.clone()),
            Some((dest, false)) => ("Disabled".to_string(), dest.clone()),
            None => ("Uncovered".to_string(), "—".to_string()),
        };
        CatchAllRow { domain: d.domain.clone(), status, destination }
    }).collect();

    let tmpl = DashboardTemplate {
        nav_active: "Dashboard",
        flash: None,
        hostname: &state.hostname,
        stats,
        domain_cards,
        catch_rows,
    };
    Html(tmpl.render().unwrap())
}
