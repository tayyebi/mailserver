use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::db::{Account, CardDavAddressBook, CardDavAddressBookWithAccount, CardDavObject};
use crate::web::auth::AuthAdmin;
use crate::web::forms::CardDavAddressBookForm;
use crate::web::AppState;

// ── Admin Templates ──

#[derive(Template)]
#[template(path = "carddav/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    addressbooks: Vec<CardDavAddressBookWithAccount>,
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
    info!("[web] GET /carddav — CardDAV admin list");
    let addressbooks = state.blocking_db(|db| db.list_all_carddav_addressbooks()).await;
    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;
    let tmpl = ListTemplate {
        nav_active: "CardDAV",
        flash: None,
        addressbooks,
        accounts,
    };
    Html(tmpl.render().unwrap())
}

pub async fn admin_create_addressbook(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<CardDavAddressBookForm>,
) -> Response {
    info!(
        "[web] POST /carddav/admin/addressbooks — creating address book for email={}",
        form.email
    );
    let email = form.email.clone();
    let display_name = form.display_name.clone();
    let description = form.description.clone().unwrap_or_default();

    let result = state
        .blocking_db(move |db| {
            let account = db.get_account_by_email(&email).ok_or("Account not found")?;
            let slug = make_slug(&display_name);
            db.create_carddav_addressbook(account.id, &slug, &display_name, &description)
        })
        .await;

    match result {
        Ok(_) => Redirect::to("/carddav").into_response(),
        Err(e) => {
            error!("[web] failed to create CardDAV address book: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "CardDAV",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: &e,
                back_url: "/carddav",
                back_label: "Back",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn admin_delete_addressbook(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /carddav/admin/addressbooks/{}/delete", id);
    state
        .blocking_db(move |db| db.delete_carddav_addressbook(id))
        .await;
    Redirect::to("/carddav").into_response()
}

pub async fn admin_delete_object(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /carddav/admin/objects/{}/delete", id);
    state
        .blocking_db(move |db| db.delete_carddav_object(id))
        .await;
    Redirect::to("/carddav").into_response()
}

// ── CardDAV Protocol: Authentication ──

fn unauthorized_carddav() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"CardDAV\", charset=\"UTF-8\"",
        )
        .header(header::CONTENT_TYPE, "text/plain")
        .header(header::CONTENT_LENGTH, "0")
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn authenticate_carddav_account(state: &AppState, headers: &HeaderMap) -> Option<Account> {
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
        warn!("[carddav] account is inactive: {:?}", account.username);
        return None;
    }

    if crate::auth::verify_password(&password, &account.password_hash) {
        Some(account)
    } else {
        warn!("[carddav] bad password for account: {:?}", account.username);
        None
    }
}

// ── CardDAV Protocol: Path Parsing ──

enum CardDavResource {
    Principal,
    AddressBookHomeSet,
    AddressBook(String),
    AddressBookObject(String, String),
}

fn account_email(account: &Account) -> String {
    format!(
        "{}@{}",
        account.username,
        account.domain_name.as_deref().unwrap_or("")
    )
}

fn parse_carddav_resource(path: &str, email: &str) -> CardDavResource {
    let base = format!("/carddav/{}", email.to_lowercase());
    let rest = if path.to_lowercase().starts_with(&format!("{}/", base)) {
        &path[base.len() + 1..]
    } else if path.to_lowercase() == base || path.to_lowercase() == format!("{}/", base) {
        return CardDavResource::Principal;
    } else {
        return CardDavResource::Principal;
    };

    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        [] => CardDavResource::Principal,
        ["addressbooks"] => CardDavResource::AddressBookHomeSet,
        ["addressbooks", slug] => CardDavResource::AddressBook(slug.to_string()),
        ["addressbooks", slug, filename] => {
            CardDavResource::AddressBookObject(slug.to_string(), filename.to_string())
        }
        _ => CardDavResource::Principal,
    }
}

// ── CardDAV Protocol: Handler Dispatch ──

pub async fn protocol_handler(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_string();
    let method = parts.method.clone();
    let headers = parts.headers.clone();

    debug!("[carddav] {} {}", method, path);

    let account = match authenticate_carddav_account(&state, &headers).await {
        Some(a) => a,
        None => return unauthorized_carddav(),
    };

    let email = account_email(&account);
    let expected_prefix_slash = format!("/carddav/{}/", email.to_lowercase());
    let expected_exact = format!("/carddav/{}", email.to_lowercase());
    let path_lower = path.to_lowercase();

    if !path_lower.starts_with(&expected_prefix_slash) && path_lower != expected_exact {
        warn!(
            "[carddav] access denied: {} tried to access {}",
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
        "MKCOL" => handle_mkcol(&state, &account, &path, &body_bytes).await,
        "GET" | "HEAD" => handle_get(&state, &account, &path).await,
        "PUT" => handle_put(&state, &account, &path, &body_bytes).await,
        "DELETE" => handle_delete(&state, &account, &path).await,
        "PROPPATCH" => handle_proppatch(&state, &account, &path).await,
        _ => {
            warn!("[carddav] method {} not allowed on {}", method, path);
            StatusCode::METHOD_NOT_ALLOWED.into_response()
        }
    }
}

// ── CardDAV Protocol: Method Handlers ──

fn handle_options() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            "Allow",
            "OPTIONS, GET, HEAD, PUT, DELETE, PROPFIND, REPORT, MKCOL, PROPPATCH",
        )
        .header("DAV", "1, 2, 3, addressbook")
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
    let resource = parse_carddav_resource(path, &email);

    match resource {
        CardDavResource::Principal => xml_multistatus(propfind_principal_xml(path, &email)),
        CardDavResource::AddressBookHomeSet => {
            let account_id = account.id;
            let addressbooks = state
                .blocking_db(move |db| db.list_carddav_addressbooks_for_account(account_id))
                .await;
            xml_multistatus(propfind_addressbook_home_xml(path, &email, &addressbooks))
        }
        CardDavResource::AddressBook(slug) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let addressbook = state
                .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
                .await;
            match addressbook {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(ab) => {
                    if depth == "1" || depth == "infinity" {
                        let ab_id = ab.id;
                        let objects = state
                            .blocking_db(move |db| db.list_carddav_objects(ab_id))
                            .await;
                        xml_multistatus(propfind_addressbook_with_objects_xml(
                            path, &email, &ab, &objects,
                        ))
                    } else {
                        xml_multistatus(propfind_addressbook_xml(path, &ab))
                    }
                }
            }
        }
        CardDavResource::AddressBookObject(slug, filename) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let addressbook = state
                .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
                .await;
            match addressbook {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(ab) => {
                    let ab_id = ab.id;
                    let filename2 = filename.clone();
                    let object = state
                        .blocking_db(move |db| {
                            db.get_carddav_object_by_filename(ab_id, &filename2)
                        })
                        .await;
                    match object {
                        None => StatusCode::NOT_FOUND.into_response(),
                        Some(obj) => xml_multistatus(propfind_object_xml(path, &obj)),
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
    let resource = parse_carddav_resource(path, &email);

    let (account_id, slug_opt) = match &resource {
        CardDavResource::AddressBook(slug) => (account.id, Some(slug.clone())),
        CardDavResource::AddressBookHomeSet => (account.id, None),
        _ => {
            return xml_multistatus(String::new());
        }
    };

    let report_body = std::str::from_utf8(body).unwrap_or("");
    let is_multiget = report_body.contains("addressbook-multiget");

    if let Some(slug) = slug_opt {
        let slug2 = slug.clone();
        let addressbook = state
            .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
            .await;
        match addressbook {
            None => return StatusCode::NOT_FOUND.into_response(),
            Some(ab) => {
                let ab_id = ab.id;
                let objects = state
                    .blocking_db(move |db| db.list_carddav_objects(ab_id))
                    .await;

                if is_multiget {
                    let requested_filenames =
                        extract_hrefs_from_multiget(report_body, &email, &slug);
                    let filtered: Vec<&CardDavObject> = if requested_filenames.is_empty() {
                        objects.iter().collect()
                    } else {
                        objects
                            .iter()
                            .filter(|o| requested_filenames.contains(&o.filename))
                            .collect()
                    };
                    xml_multistatus(report_objects_xml(&filtered, &email, &slug))
                } else {
                    let all: Vec<&CardDavObject> = objects.iter().collect();
                    xml_multistatus(report_objects_xml(&all, &email, &slug))
                }
            }
        }
    } else {
        let addressbooks = state
            .blocking_db(move |db| db.list_carddav_addressbooks_for_account(account_id))
            .await;
        xml_multistatus(propfind_addressbook_home_xml(path, &email, &addressbooks))
    }
}

async fn handle_mkcol(
    state: &AppState,
    account: &Account,
    path: &str,
    body: &[u8],
) -> Response {
    let email = account_email(account);
    let resource = parse_carddav_resource(path, &email);

    let slug = match resource {
        CardDavResource::AddressBook(s) => s,
        _ => {
            return StatusCode::CONFLICT.into_response();
        }
    };

    let display_name = extract_displayname_from_xml(body).unwrap_or_else(|| slug.clone());
    let description = extract_description_from_xml(body).unwrap_or_default();

    let account_id = account.id;
    let slug2 = slug.clone();
    let result = state
        .blocking_db(move |db| {
            db.create_carddav_addressbook(account_id, &slug2, &display_name, &description)
        })
        .await;

    match result {
        Ok(_) => {
            info!("[carddav] MKCOL created slug={} for {}", slug, email);
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
                error!("[carddav] MKCOL failed: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

async fn handle_get(state: &AppState, account: &Account, path: &str) -> Response {
    let email = account_email(account);
    let resource = parse_carddav_resource(path, &email);

    match resource {
        CardDavResource::AddressBookObject(slug, filename) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let addressbook = state
                .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
                .await;
            match addressbook {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(ab) => {
                    let ab_id = ab.id;
                    let filename2 = filename.clone();
                    let object = state
                        .blocking_db(move |db| {
                            db.get_carddav_object_by_filename(ab_id, &filename2)
                        })
                        .await;
                    match object {
                        None => StatusCode::NOT_FOUND.into_response(),
                        Some(obj) => Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/vcard; charset=utf-8")
                            .header("ETag", format!("\"{}\"", obj.etag))
                            .body(axum::body::Body::from(obj.data))
                            .unwrap(),
                    }
                }
            }
        }
        CardDavResource::AddressBook(slug) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let addressbook = state
                .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
                .await;
            match addressbook {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(_) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(axum::body::Body::from(format!(
                        "<html><body><h1>CardDAV Address Book: {}</h1></body></html>",
                        xml_escape(&slug)
                    )))
                    .unwrap(),
            }
        }
        _ => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(axum::body::Body::from(format!(
                "<html><body><h1>CardDAV: {}</h1></body></html>",
                xml_escape(&email)
            )))
            .unwrap(),
    }
}

async fn handle_put(
    state: &AppState,
    account: &Account,
    path: &str,
    body: &[u8],
) -> Response {
    let email = account_email(account);
    let resource = parse_carddav_resource(path, &email);

    let (slug, filename) = match resource {
        CardDavResource::AddressBookObject(s, f) => (s, f),
        _ => return StatusCode::CONFLICT.into_response(),
    };

    let vcard_data = match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let uid = extract_uid_from_vcard(&vcard_data)
        .unwrap_or_else(|| filename.trim_end_matches(".vcf").to_string());
    let etag = compute_etag(&vcard_data);

    let account_id = account.id;
    let slug2 = slug.clone();
    let addressbook = state
        .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
        .await;

    match addressbook {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(ab) => {
            let ab_id = ab.id;
            let uid2 = uid.clone();
            let filename2 = filename.clone();
            let etag2 = etag.clone();
            let result = state
                .blocking_db(move |db| {
                    let r = db.create_or_update_carddav_object(
                        ab_id, &uid2, &filename2, &etag2, &vcard_data,
                    );
                    if r.is_ok() {
                        db.update_carddav_addressbook_ctag(ab_id);
                    }
                    r
                })
                .await;

            match result {
                Ok(_) => {
                    info!("[carddav] PUT {} for {}", filename, email);
                    Response::builder()
                        .status(StatusCode::CREATED)
                        .header("ETag", format!("\"{}\"", etag))
                        .header(header::CONTENT_LENGTH, "0")
                        .body(axum::body::Body::empty())
                        .unwrap()
                }
                Err(e) => {
                    error!("[carddav] PUT failed: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
    }
}

async fn handle_delete(state: &AppState, account: &Account, path: &str) -> Response {
    let email = account_email(account);
    let resource = parse_carddav_resource(path, &email);

    match resource {
        CardDavResource::AddressBookObject(slug, filename) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let addressbook = state
                .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
                .await;
            match addressbook {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(ab) => {
                    let ab_id = ab.id;
                    let filename2 = filename.clone();
                    state
                        .blocking_db(move |db| {
                            db.delete_carddav_object_by_filename(ab_id, &filename2);
                            db.update_carddav_addressbook_ctag(ab_id);
                        })
                        .await;
                    info!("[carddav] DELETE {} for {}", filename, email);
                    StatusCode::NO_CONTENT.into_response()
                }
            }
        }
        CardDavResource::AddressBook(slug) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let addressbook = state
                .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
                .await;
            match addressbook {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(ab) => {
                    let ab_id = ab.id;
                    state
                        .blocking_db(move |db| db.delete_carddav_addressbook(ab_id))
                        .await;
                    warn!("[carddav] DELETE address book {} for {}", slug, email);
                    StatusCode::NO_CONTENT.into_response()
                }
            }
        }
        _ => StatusCode::FORBIDDEN.into_response(),
    }
}

async fn handle_proppatch(state: &AppState, account: &Account, path: &str) -> Response {
    let email = account_email(account);
    let resource = parse_carddav_resource(path, &email);
    let href = path.to_string();

    match resource {
        CardDavResource::AddressBook(slug) => {
            let account_id = account.id;
            let slug2 = slug.clone();
            let addressbook = state
                .blocking_db(move |db| db.get_carddav_addressbook_by_slug(account_id, &slug2))
                .await;
            if addressbook.is_none() {
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
        .header("DAV", "1, 2, 3, addressbook")
        .body(axum::body::Body::from(inner))
        .unwrap()
}

fn propfind_principal_xml(_path: &str, email: &str) -> String {
    let href = format!("/carddav/{}/", email);
    let home = format!("/carddav/{}/addressbooks/", email);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav">
  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/><D:principal/></D:resourcetype>
        <D:displayname>{email}</D:displayname>
        <D:principal-URL><D:href>{href}</D:href></D:principal-URL>
        <A:addressbook-home-set><D:href>{home}</D:href></A:addressbook-home-set>
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

fn propfind_addressbook_home_xml(
    _path: &str,
    email: &str,
    addressbooks: &[CardDavAddressBook],
) -> String {
    let home_href = format!("/carddav/{}/addressbooks/", email);
    let mut ab_responses = String::new();
    for ab in addressbooks {
        let ab_href = format!("/carddav/{}/addressbooks/{}/", email, ab.slug);
        ab_responses.push_str(&propfind_addressbook_entry_xml(&ab_href, ab));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav" xmlns:CS="http://calendarserver.org/ns/">
  <D:response>
    <D:href>{home_href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:displayname>Address Books</D:displayname>
        <D:current-user-principal><D:href>/carddav/{email}/</D:href></D:current-user-principal>
        <A:addressbook-home-set><D:href>{home_href}</D:href></A:addressbook-home-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
{ab_responses}</D:multistatus>"#,
        home_href = xml_escape(&home_href),
        email = xml_escape(email),
        ab_responses = ab_responses,
    )
}

fn propfind_addressbook_entry_xml(href: &str, ab: &CardDavAddressBook) -> String {
    format!(
        r#"  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/><A:addressbook/></D:resourcetype>
        <D:displayname>{name}</D:displayname>
        <CS:getctag>{ctag}</CS:getctag>
        <D:getetag>{ctag}</D:getetag>
        <A:addressbook-description>{desc}</A:addressbook-description>
        <A:supported-address-data>
          <A:address-data-type content-type="text/vcard" version="3.0"/>
          <A:address-data-type content-type="text/vcard" version="4.0"/>
        </A:supported-address-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
"#,
        href = xml_escape(href),
        name = xml_escape(&ab.display_name),
        ctag = xml_escape(&ab.ctag),
        desc = xml_escape(&ab.description),
    )
}

fn propfind_addressbook_xml(path: &str, ab: &CardDavAddressBook) -> String {
    let href = path.to_string();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav" xmlns:CS="http://calendarserver.org/ns/">
{}</D:multistatus>"#,
        propfind_addressbook_entry_xml(&href, ab)
    )
}

fn propfind_addressbook_with_objects_xml(
    path: &str,
    email: &str,
    ab: &CardDavAddressBook,
    objects: &[CardDavObject],
) -> String {
    let ab_href = path.to_string();
    let mut object_entries = String::new();
    for obj in objects {
        let obj_href = format!(
            "/carddav/{}/addressbooks/{}/{}",
            email, ab.slug, obj.filename
        );
        object_entries.push_str(&propfind_object_entry_xml(&obj_href, obj));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav" xmlns:CS="http://calendarserver.org/ns/">
{}{}
</D:multistatus>"#,
        propfind_addressbook_entry_xml(&ab_href, ab),
        object_entries
    )
}

fn propfind_object_entry_xml(href: &str, obj: &CardDavObject) -> String {
    format!(
        r#"  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getetag>"{etag}"</D:getetag>
        <D:getcontenttype>text/vcard; charset=utf-8</D:getcontenttype>
        <A:address-data>{data}</A:address-data>
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

fn propfind_object_xml(path: &str, obj: &CardDavObject) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav">
{}</D:multistatus>"#,
        propfind_object_entry_xml(path, obj)
    )
}

fn report_objects_xml(objects: &[&CardDavObject], email: &str, slug: &str) -> String {
    let mut entries = String::new();
    for obj in objects {
        let href = format!(
            "/carddav/{}/addressbooks/{}/{}",
            email, slug, obj.filename
        );
        entries.push_str(&propfind_object_entry_xml(&href, obj));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav">
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
        "addressbook".to_string()
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

fn extract_uid_from_vcard(vcard: &str) -> Option<String> {
    for line in vcard.lines() {
        if let Some(uid) = line.strip_prefix("UID:") {
            return Some(uid.trim().to_string());
        }
    }
    None
}

fn extract_displayname_from_xml(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let tag = "displayname>";
    let start = text.find(tag)? + tag.len();
    let end = text[start..].find('<')?;
    let name = text[start..start + end].trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn extract_description_from_xml(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let tag = "addressbook-description>";
    let start = text.find(tag)? + tag.len();
    let end = text[start..].find('<')?;
    Some(text[start..start + end].trim().to_string())
}

fn extract_hrefs_from_multiget(body: &str, email: &str, slug: &str) -> Vec<String> {
    let prefix = format!("/carddav/{}/addressbooks/{}/", email, slug);
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
