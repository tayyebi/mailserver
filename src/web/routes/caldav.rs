use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::db::{Account, CalDavCalendar, CalDavCalendarWithAccount, CalDavObject};
use crate::web::auth::AuthAdmin;
use crate::web::forms::CalDavCalendarForm;
use crate::web::AppState;

// ── Admin Templates ──

#[derive(Template)]
#[template(path = "caldav/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    calendars: Vec<CalDavCalendarWithAccount>,
    accounts: Vec<crate::db::Account>,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    status_code: u16,
    status_text: &'a str,
    title: &'a str,
    message: &'a str,
    back_url: &'a str,
    back_label: &'a str,
}

// ── Admin Handlers ──

pub async fn admin_list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /caldav — CalDAV admin list");
    let calendars = state.blocking_db(|db| db.list_all_caldav_calendars()).await;
    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;
    let tmpl = ListTemplate {
        nav_active: "CalDAV",
        flash: None,
        calendars,
        accounts,
    };
    Html(tmpl.render().unwrap())
}

pub async fn admin_create_calendar(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<CalDavCalendarForm>,
) -> Response {
    info!(
        "[web] POST /caldav/admin/calendars — creating calendar for email={}",
        form.email
    );
    let email = form.email.clone();
    let display_name = form.display_name.clone();
    let description = form.description.clone().unwrap_or_default();
    let color = form
        .color
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "#0000FF".to_string());

    let result = state
        .blocking_db(move |db| {
            let account = db.get_account_by_email(&email).ok_or("Account not found")?;
            let slug = make_slug(&display_name);
            db.create_caldav_calendar(account.id, &slug, &display_name, &description, &color)
                .map_err(|e| e.as_str().to_string())
        })
        .await;

    match result {
        Ok(_) => Redirect::to("/caldav").into_response(),
        Err(e) => {
            error!("[web] failed to create CalDAV calendar: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "CalDAV",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: &e,
                back_url: "/caldav",
                back_label: "Back",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn admin_delete_calendar(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /caldav/admin/calendars/{}/delete", id);
    state
        .blocking_db(move |db| db.delete_caldav_calendar(id))
        .await;
    Redirect::to("/caldav").into_response()
}

pub async fn admin_delete_object(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /caldav/admin/objects/{}/delete", id);
    state
        .blocking_db(move |db| db.delete_caldav_object(id))
        .await;
    Redirect::to("/caldav").into_response()
}

// ── CalDAV Protocol: Authentication ──

fn unauthorized_caldav() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"CalDAV\", charset=\"UTF-8\"",
        )
        .header(header::CONTENT_TYPE, "text/plain")
        .header(header::CONTENT_LENGTH, "0")
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn authenticate_caldav_account(state: &AppState, headers: &HeaderMap) -> Option<Account> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())?;

    if !auth_header.starts_with("Basic ") {
        return None;
    }

    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &auth_header[6..],
    )
    .ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    let (email, password) = credentials.split_once(':')?;

    let email = email.to_string();
    let password = password.to_string();

    let account = state
        .blocking_db(move |db| db.get_account_by_email(&email))
        .await?;

    if !account.active {
        warn!("[caldav] account is inactive: {:?}", account.username);
        return None;
    }

    if crate::auth::verify_password(&password, &account.password_hash) {
        Some(account)
    } else {
        warn!("[caldav] bad password for account: {:?}", account.username);
        None
    }
}

// ── CalDAV Protocol: Path Parsing ──

enum CalDavResource {
    Principal,
    CalendarHomeSet,
    Calendar(String),
    CalendarObject(String, String),
}

fn account_email(account: &Account) -> String {
    format!(
        "{}@{}",
        account.username,
        account.domain_name.as_deref().unwrap_or("")
    )
}

fn parse_caldav_resource(path: &str, email: &str) -> CalDavResource {
    let base = format!("/caldav/{}", email.to_lowercase());
    let rest = if path.to_lowercase().starts_with(&format!("{}/", base)) {
        &path[base.len() + 1..]
    } else if path.to_lowercase() == base || path.to_lowercase() == format!("{}/", base) {
        return CalDavResource::Principal;
    } else {
        return CalDavResource::Principal;
    };

    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        [] => CalDavResource::Principal,
        ["calendars"] => CalDavResource::CalendarHomeSet,
        ["calendars", slug] => CalDavResource::Calendar(slug.to_string()),
        ["calendars", slug, filename] => {
            CalDavResource::CalendarObject(slug.to_string(), filename.to_string())
        }
        _ => CalDavResource::Principal,
    }
}

// ── CalDAV Protocol: Handler Dispatch ──

pub async fn protocol_handler(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_string();
    let method = parts.method.clone();
    let headers = parts.headers.clone();

    debug!("[caldav] {} {}", method, path);

    let account = match authenticate_caldav_account(&state, &headers).await {
        Some(a) => a,
        None => return unauthorized_caldav(),
    };

    let email = account_email(&account);
    let expected_prefix_slash = format!("/caldav/{}/", email.to_lowercase());
    let expected_exact = format!("/caldav/{}", email.to_lowercase());
    let path_lower = path.to_lowercase();

    if !path_lower.starts_with(&expected_prefix_slash) && path_lower != expected_exact {
        warn!(
            "[caldav] access denied: {} tried to access {}",
            email, path
        );
        return StatusCode::FORBIDDEN.into_response();
    }

    let body_bytes = axum::body::to_bytes(body, 4 * 1024 * 1024)
        .await
        .unwrap_or_default();

    match method.as_str() {
        "OPTIONS" => handle_options(),
        "PROPFIND" => {
            let depth = headers
                .get("Depth")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("0")
                .to_string();
            handle_propfind(&state, &account, &path, &depth).await
        }
        "REPORT" => handle_report(&state, &account, &path, &body_bytes).await,
        "MKCALENDAR" => handle_mkcalendar(&state, &account, &path, &body_bytes).await,
        "GET" | "HEAD" => handle_get(&state, &account, &path).await,
        "PUT" => handle_put(&state, &account, &path, &body_bytes).await,
        "DELETE" => handle_delete(&state, &account, &path).await,
        "PROPPATCH" => handle_proppatch(&state, &account, &path).await,
        _ => {
            warn!("[caldav] method {} not allowed on {}", method, path);
            StatusCode::METHOD_NOT_ALLOWED.into_response()
        }
    }
}

// ── CalDAV Protocol: Method Handlers ──

fn handle_options() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            "Allow",
            "OPTIONS, GET, HEAD, PUT, DELETE, PROPFIND, REPORT, MKCALENDAR, PROPPATCH",
        )
        .header("DAV", "1, 2, 3, calendar-access")
        .header(header::CONTENT_LENGTH, "0")
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn handle_propfind(
    state: &AppState,
    account: &Account,
    path: &str,
    depth: &str,
) -> Response {
    let email = account_email(account);
    let resource = parse_caldav_resource(path, &email);

    match resource {
        CalDavResource::Principal => xml_multistatus(propfind_principal_xml(path, &email)),
        CalDavResource::CalendarHomeSet => {
            let account_id = account.id;
            let calendars = state
                .blocking_db(move |db| db.list_caldav_calendars_for_account(account_id))
                .await;
            xml_multistatus(propfind_calendar_home_xml(path, &email, &calendars))
        }
        CalDavResource::Calendar(slug) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let calendar = state
                .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
                .await;
            match calendar {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(cal) => {
                    if depth == "1" || depth == "infinity" {
                        let cal_id = cal.id;
                        let objects = state
                            .blocking_db(move |db| db.list_caldav_objects(cal_id))
                            .await;
                        xml_multistatus(propfind_calendar_with_objects_xml(
                            path, &email, &cal, &objects,
                        ))
                    } else {
                        xml_multistatus(propfind_calendar_xml(path, &cal))
                    }
                }
            }
        }
        CalDavResource::CalendarObject(slug, filename) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let calendar = state
                .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
                .await;
            match calendar {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(cal) => {
                    let cal_id = cal.id;
                    let filename2 = filename.clone();
                    let object = state
                        .blocking_db(move |db| {
                            db.get_caldav_object_by_filename(cal_id, &filename2)
                        })
                        .await;
                    match object {
                        None => StatusCode::NOT_FOUND.into_response(),
                        Some(obj) => {
                            xml_multistatus(propfind_object_xml(path, &obj))
                        }
                    }
                }
            }
        }
    }
}

async fn handle_report(
    state: &AppState,
    account: &Account,
    path: &str,
    body: &[u8],
) -> Response {
    let email = account_email(account);
    let resource = parse_caldav_resource(path, &email);

    // Determine the calendar context from path
    let (account_id, slug_opt) = match &resource {
        CalDavResource::Calendar(slug) => (account.id, Some(slug.clone())),
        CalDavResource::CalendarHomeSet => (account.id, None),
        _ => {
            return xml_multistatus(String::new());
        }
    };

    let report_body = std::str::from_utf8(body).unwrap_or("");

    // Check if this is calendar-multiget (list of specific hrefs)
    let is_multiget = report_body.contains("calendar-multiget");

    if let Some(slug) = slug_opt {
        let slug2 = slug.clone();
        let calendar = state
            .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
            .await;
        match calendar {
            None => return StatusCode::NOT_FOUND.into_response(),
            Some(cal) => {
                let cal_id = cal.id;
                let objects = state
                    .blocking_db(move |db| db.list_caldav_objects(cal_id))
                    .await;

                if is_multiget {
                    // Filter objects by requested hrefs
                    let requested_filenames = extract_hrefs_from_multiget(report_body, &email, &slug);
                    let filtered: Vec<&CalDavObject> = if requested_filenames.is_empty() {
                        objects.iter().collect()
                    } else {
                        objects
                            .iter()
                            .filter(|o| requested_filenames.contains(&o.filename))
                            .collect()
                    };
                    xml_multistatus(report_objects_xml(&filtered, &email, &slug))
                } else {
                    // calendar-query: return all objects
                    let all: Vec<&CalDavObject> = objects.iter().collect();
                    xml_multistatus(report_objects_xml(&all, &email, &slug))
                }
            }
        }
    } else {
        // Calendar home set report: list all calendars
        let calendars = state
            .blocking_db(move |db| db.list_caldav_calendars_for_account(account_id))
            .await;
        xml_multistatus(propfind_calendar_home_xml(path, &email, &calendars))
    }
}

async fn handle_mkcalendar(
    state: &AppState,
    account: &Account,
    path: &str,
    body: &[u8],
) -> Response {
    let email = account_email(account);
    let resource = parse_caldav_resource(path, &email);

    let slug = match resource {
        CalDavResource::Calendar(s) => s,
        _ => {
            return StatusCode::CONFLICT.into_response();
        }
    };

    // Try to extract display_name from MKCALENDAR XML body
    let display_name = extract_displayname_from_xml(body).unwrap_or_else(|| slug.clone());
    let description = extract_description_from_xml(body).unwrap_or_default();
    let color = extract_color_from_xml(body).unwrap_or_else(|| "#0000FF".to_string());

    let account_id = account.id;
    let slug2 = slug.clone();
    let result = state
        .blocking_db(move |db| {
            db.create_caldav_calendar(account_id, &slug2, &display_name, &description, &color)
        })
        .await;

    match result {
        Ok(_) => {
            info!("[caldav] MKCALENDAR created slug={} for {}", slug, email);
            Response::builder()
                .status(StatusCode::CREATED)
                .header(header::CONTENT_LENGTH, "0")
                .body(axum::body::Body::empty())
                .unwrap()
        }
        Err(e) => {
            if e.contains("duplicate") || e.contains("unique") || e.contains("UNIQUE") {
                StatusCode::METHOD_NOT_ALLOWED.into_response()
            } else {
                error!("[caldav] MKCALENDAR failed: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

async fn handle_get(state: &AppState, account: &Account, path: &str) -> Response {
    let email = account_email(account);
    let resource = parse_caldav_resource(path, &email);

    match resource {
        CalDavResource::CalendarObject(slug, filename) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let calendar = state
                .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
                .await;
            match calendar {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(cal) => {
                    let cal_id = cal.id;
                    let filename2 = filename.clone();
                    let object = state
                        .blocking_db(move |db| {
                            db.get_caldav_object_by_filename(cal_id, &filename2)
                        })
                        .await;
                    match object {
                        None => StatusCode::NOT_FOUND.into_response(),
                        Some(obj) => Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/calendar; charset=utf-8")
                            .header("ETag", format!("\"{}\"", obj.etag))
                            .body(axum::body::Body::from(obj.data))
                            .unwrap(),
                    }
                }
            }
        }
        CalDavResource::Calendar(slug) => {
            // Some clients do GET on a calendar collection — redirect to PROPFIND
            let account_id = account.id;
            let slug2 = slug.clone();
            let calendar = state
                .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
                .await;
            match calendar {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(cal) => {
                    let cal_id = cal.id;
                    let objects = state
                        .blocking_db(move |db| db.list_caldav_objects(cal_id))
                        .await;
                    // Return a combined iCalendar feed
                    let ics = build_icalendar_feed(&cal.display_name, &objects);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/calendar; charset=utf-8")
                        .body(axum::body::Body::from(ics))
                        .unwrap()
                }
            }
        }
        _ => {
            // Return principal info for GET on principal or home set
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(axum::body::Body::from(format!(
                    "<html><body><h1>CalDAV: {}</h1></body></html>",
                    email
                )))
                .unwrap()
        }
    }
}

async fn handle_put(
    state: &AppState,
    account: &Account,
    path: &str,
    body: &[u8],
) -> Response {
    let email = account_email(account);
    let resource = parse_caldav_resource(path, &email);

    let (slug, filename) = match resource {
        CalDavResource::CalendarObject(s, f) => (s, f),
        _ => return StatusCode::CONFLICT.into_response(),
    };

    let ics_data = match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let uid = extract_uid_from_ics(&ics_data).unwrap_or_else(|| filename.trim_end_matches(".ics").to_string());
    let etag = compute_etag(&ics_data);

    let account_id = account.id;
    let slug2 = slug.clone();
    let calendar = state
        .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
        .await;

    match calendar {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(cal) => {
            let cal_id = cal.id;
            let uid2 = uid.clone();
            let filename2 = filename.clone();
            let etag2 = etag.clone();
            let result = state
                .blocking_db(move |db| {
                    let r = db.create_or_update_caldav_object(cal_id, &uid2, &filename2, &etag2, &ics_data);
                    if r.is_ok() {
                        db.update_caldav_calendar_ctag(cal_id);
                    }
                    r
                })
                .await;

            match result {
                Ok(_) => {
                    info!("[caldav] PUT {} for {}", filename, email);
                    Response::builder()
                        .status(StatusCode::CREATED)
                        .header("ETag", format!("\"{}\"", etag))
                        .header(header::CONTENT_LENGTH, "0")
                        .body(axum::body::Body::empty())
                        .unwrap()
                }
                Err(e) => {
                    error!("[caldav] PUT failed: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
    }
}

async fn handle_delete(state: &AppState, account: &Account, path: &str) -> Response {
    let email = account_email(account);
    let resource = parse_caldav_resource(path, &email);

    match resource {
        CalDavResource::CalendarObject(slug, filename) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let calendar = state
                .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
                .await;
            match calendar {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(cal) => {
                    let cal_id = cal.id;
                    let filename2 = filename.clone();
                    state
                        .blocking_db(move |db| {
                            db.delete_caldav_object_by_filename(cal_id, &filename2);
                            db.update_caldav_calendar_ctag(cal_id);
                        })
                        .await;
                    info!("[caldav] DELETE {} for {}", filename, email);
                    StatusCode::NO_CONTENT.into_response()
                }
            }
        }
        CalDavResource::Calendar(slug) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let calendar = state
                .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
                .await;
            match calendar {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(cal) => {
                    let cal_id = cal.id;
                    state
                        .blocking_db(move |db| db.delete_caldav_calendar(cal_id))
                        .await;
                    warn!("[caldav] DELETE calendar {} for {}", slug, email);
                    StatusCode::NO_CONTENT.into_response()
                }
            }
        }
        _ => StatusCode::FORBIDDEN.into_response(),
    }
}

async fn handle_proppatch(state: &AppState, account: &Account, path: &str) -> Response {
    // Minimal PROPPATCH support — accept but do nothing for now
    let email = account_email(account);
    let resource = parse_caldav_resource(path, &email);
    let href = path.to_string();

    match resource {
        CalDavResource::Calendar(slug) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let calendar = state
                .blocking_db(move |db| db.get_caldav_calendar_by_slug(account_id, &slug2))
                .await;
            if calendar.is_none() {
                return StatusCode::NOT_FOUND.into_response();
            }
        }
        _ => {}
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>{}</D:href>
    <D:propstat>
      <D:prop/>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
        xml_escape(&href)
    );
    xml_multistatus(xml)
}

// ── XML Response Builders ──

fn xml_multistatus(inner: String) -> Response {
    Response::builder()
        .status(207)
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .header("DAV", "1, 2, 3, calendar-access")
        .body(axum::body::Body::from(inner))
        .unwrap()
}

fn propfind_principal_xml(_path: &str, email: &str) -> String {
    let href = format!("/caldav/{}/", email);
    let home = format!("/caldav/{}/calendars/", email);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/><D:principal/></D:resourcetype>
        <D:displayname>{email}</D:displayname>
        <D:principal-URL><D:href>{href}</D:href></D:principal-URL>
        <C:calendar-home-set><D:href>{home}</D:href></C:calendar-home-set>
        <C:calendar-user-address-set>
          <D:href>mailto:{email}</D:href>
          <D:href>{href}</D:href>
        </C:calendar-user-address-set>
        <D:current-user-principal><D:href>{href}</D:href></D:current-user-principal>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
        href = xml_escape(&href),
        email = xml_escape(email),
        home = xml_escape(&home),
    )
}

fn propfind_calendar_home_xml(_path: &str, email: &str, calendars: &[CalDavCalendar]) -> String {
    let home_href = format!("/caldav/{}/calendars/", email);
    let mut calendar_responses = String::new();
    for cal in calendars {
        let cal_href = format!("/caldav/{}/calendars/{}/", email, cal.slug);
        calendar_responses.push_str(&propfind_calendar_entry_xml(&cal_href, cal));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:CS="http://calendarserver.org/ns/">
  <D:response>
    <D:href>{home_href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:displayname>Calendars</D:displayname>
        <D:current-user-principal><D:href>/caldav/{email}/</D:href></D:current-user-principal>
        <C:calendar-home-set><D:href>{home_href}</D:href></C:calendar-home-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
{calendar_responses}</D:multistatus>"#,
        home_href = xml_escape(&home_href),
        email = xml_escape(email),
        calendar_responses = calendar_responses,
    )
}

fn propfind_calendar_entry_xml(href: &str, cal: &CalDavCalendar) -> String {
    format!(
        r#"  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>
        <D:displayname>{name}</D:displayname>
        <CS:getctag>{ctag}</CS:getctag>
        <D:getetag>{ctag}</D:getetag>
        <C:calendar-description>{desc}</C:calendar-description>
        <C:supported-calendar-component-set>
          <C:comp name="VEVENT"/>
          <C:comp name="VTODO"/>
          <C:comp name="VJOURNAL"/>
        </C:supported-calendar-component-set>
        <apple:calendar-color xmlns:apple="http://apple.com/ns/ical/">{color}</apple:calendar-color>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
"#,
        href = xml_escape(href),
        name = xml_escape(&cal.display_name),
        ctag = xml_escape(&cal.ctag),
        desc = xml_escape(&cal.description),
        color = xml_escape(&cal.color),
    )
}

fn propfind_calendar_xml(path: &str, cal: &CalDavCalendar) -> String {
    let href = path.to_string();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:CS="http://calendarserver.org/ns/">
{}</D:multistatus>"#,
        propfind_calendar_entry_xml(&href, cal)
    )
}

fn propfind_calendar_with_objects_xml(
    path: &str,
    email: &str,
    cal: &CalDavCalendar,
    objects: &[CalDavObject],
) -> String {
    let cal_href = path.to_string();
    let mut object_entries = String::new();
    for obj in objects {
        let obj_href = format!("/caldav/{}/calendars/{}/{}", email, cal.slug, obj.filename);
        object_entries.push_str(&propfind_object_entry_xml(&obj_href, obj));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:CS="http://calendarserver.org/ns/">
{}{}
</D:multistatus>"#,
        propfind_calendar_entry_xml(&cal_href, cal),
        object_entries
    )
}

fn propfind_object_entry_xml(href: &str, obj: &CalDavObject) -> String {
    format!(
        r#"  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getetag>"{etag}"</D:getetag>
        <D:getcontenttype>text/calendar; charset=utf-8</D:getcontenttype>
        <C:calendar-data>{data}</C:calendar-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
"#,
        href = xml_escape(href),
        etag = xml_escape(&obj.etag),
        data = xml_escape(&obj.data),
    )
}

fn propfind_object_xml(path: &str, obj: &CalDavObject) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
{}</D:multistatus>"#,
        propfind_object_entry_xml(path, obj)
    )
}

fn report_objects_xml(objects: &[&CalDavObject], email: &str, slug: &str) -> String {
    let mut entries = String::new();
    for obj in objects {
        let href = format!("/caldav/{}/calendars/{}/{}", email, slug, obj.filename);
        entries.push_str(&propfind_object_entry_xml(&href, obj));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
{}</D:multistatus>"#,
        entries
    )
}

// ── Utility Helpers ──

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn make_slug(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "calendar".to_string()
    } else {
        slug
    }
}

fn compute_etag(data: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    format!("{:x}", h.finish())
}

fn extract_uid_from_ics(ics: &str) -> Option<String> {
    for line in ics.lines() {
        if let Some(uid) = line.strip_prefix("UID:") {
            return Some(uid.trim().to_string());
        }
    }
    None
}

fn extract_displayname_from_xml(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    // Simple text search for displayname tag value
    let tag = "displayname>";
    let start = text.find(tag)? + tag.len();
    let end = text[start..].find('<')?;
    let name = text[start..start + end].trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn extract_description_from_xml(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let tag = "calendar-description>";
    let start = text.find(tag)? + tag.len();
    let end = text[start..].find('<')?;
    Some(text[start..start + end].trim().to_string())
}

fn extract_color_from_xml(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    for tag in &["calendar-color>", "CalendarColor>"] {
        if let Some(pos) = text.find(tag) {
            let start = pos + tag.len();
            if let Some(end) = text[start..].find('<') {
                let color = text[start..start + end].trim().to_string();
                if !color.is_empty() {
                    return Some(color);
                }
            }
        }
    }
    None
}

fn extract_hrefs_from_multiget(body: &str, email: &str, slug: &str) -> Vec<String> {
    let prefix = format!("/caldav/{}/calendars/{}/", email, slug);
    let mut filenames = Vec::new();
    let href_tag = "<D:href>";
    let mut remaining = body;
    while let Some(start) = remaining.find(href_tag) {
        remaining = &remaining[start + href_tag.len()..];
        if let Some(end) = remaining.find("</D:href>") {
            let href = remaining[..end].trim();
            if let Some(filename) = href.strip_prefix(&prefix) {
                filenames.push(filename.trim_end_matches('/').to_string());
            }
            remaining = &remaining[end..];
        } else {
            break;
        }
    }
    filenames
}

fn build_icalendar_feed(calendar_name: &str, objects: &[CalDavObject]) -> String {
    let mut ics = String::new();
    ics.push_str("BEGIN:VCALENDAR\r\n");
    ics.push_str("VERSION:2.0\r\n");
    ics.push_str("PRODID:-//Mailserver CalDAV//EN\r\n");
    ics.push_str(&format!("X-WR-CALNAME:{}\r\n", calendar_name));
    for obj in objects {
        // Strip outer VCALENDAR wrapper from individual objects
        let inner = strip_vcalendar_wrapper(&obj.data);
        ics.push_str(&inner);
    }
    ics.push_str("END:VCALENDAR\r\n");
    ics
}

fn strip_vcalendar_wrapper(ics: &str) -> String {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in ics.lines() {
        let upper = line.trim().to_uppercase();
        if upper == "BEGIN:VCALENDAR" {
            inside = true;
            continue;
        }
        if upper == "END:VCALENDAR" {
            inside = false;
            continue;
        }
        if inside {
            if upper.starts_with("VERSION:") || upper.starts_with("PRODID:") {
                continue;
            }
            lines.push(line.to_string());
        }
    }
    lines.join("\r\n") + "\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_slug_basic() {
        assert_eq!(make_slug("My Calendar"), "my-calendar");
    }

    #[test]
    fn make_slug_empty_returns_calendar() {
        assert_eq!(make_slug(""), "calendar");
        assert_eq!(make_slug("!!!"), "calendar");
    }

    #[test]
    fn extract_uid_from_ics_found() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:test-uid-123\r\nSUMMARY:Test\r\nEND:VEVENT\r\nEND:VCALENDAR";
        assert_eq!(extract_uid_from_ics(ics), Some("test-uid-123".to_string()));
    }

    #[test]
    fn extract_uid_from_ics_not_found() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Test\r\nEND:VEVENT\r\nEND:VCALENDAR";
        assert_eq!(extract_uid_from_ics(ics), None);
    }

    #[test]
    fn compute_etag_deterministic() {
        let data = "BEGIN:VCALENDAR\r\nEND:VCALENDAR";
        assert_eq!(compute_etag(data), compute_etag(data));
    }

    #[test]
    fn compute_etag_different_for_different_data() {
        assert_ne!(compute_etag("data1"), compute_etag("data2"));
    }

    #[test]
    fn xml_escape_chars() {
        assert_eq!(xml_escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }

    #[test]
    fn parse_caldav_resource_principal() {
        let r = parse_caldav_resource("/caldav/user@example.com/", "user@example.com");
        assert!(matches!(r, CalDavResource::Principal));
    }

    #[test]
    fn parse_caldav_resource_home_set() {
        let r = parse_caldav_resource("/caldav/user@example.com/calendars/", "user@example.com");
        assert!(matches!(r, CalDavResource::CalendarHomeSet));
    }

    #[test]
    fn parse_caldav_resource_calendar() {
        let r = parse_caldav_resource("/caldav/user@example.com/calendars/default/", "user@example.com");
        match r {
            CalDavResource::Calendar(slug) => assert_eq!(slug, "default"),
            _ => panic!("Expected Calendar"),
        }
    }

    #[test]
    fn parse_caldav_resource_object() {
        let r = parse_caldav_resource(
            "/caldav/user@example.com/calendars/default/event.ics",
            "user@example.com",
        );
        match r {
            CalDavResource::CalendarObject(slug, filename) => {
                assert_eq!(slug, "default");
                assert_eq!(filename, "event.ics");
            }
            _ => panic!("Expected CalendarObject"),
        }
    }

    #[test]
    fn strip_vcalendar_wrapper_removes_outer() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nBEGIN:VEVENT\r\nSUMMARY:Hello\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let result = strip_vcalendar_wrapper(ics);
        assert!(result.contains("BEGIN:VEVENT"));
        assert!(result.contains("SUMMARY:Hello"));
        assert!(!result.contains("BEGIN:VCALENDAR"));
        assert!(!result.contains("VERSION:"));
    }
}
