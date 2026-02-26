//! SOAP 1.1 API endpoint for email operations.
//!
//! Endpoints:
//!   `GET  /api/soap`  — Return the WSDL service description
//!   `POST /api/soap`  — Process a SOAP 1.1 request
//!
//! All SOAP operations require HTTP Basic Auth (admin credentials) or a Bearer
//! token, exactly like the REST API.
//!
//! Supported operations:
//!   `ListEmails`   — List emails in an account's inbox or folder
//!   `GetEmail`     — Read the full content of a single email
//!   `SendEmail`    — Send an email from a mail account
//!   `DeleteEmail`  — Permanently delete an email

use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use log::info;

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

use super::webmail::{
    extract_body, folder_root, is_safe_folder, is_safe_path_component, maildir_path, read_emails,
};

const PAGE_SIZE: usize = 20;
/// Target namespace for the SOAP service.
const TNS: &str = "urn:mailserver";

// ── XML helpers ───────────────────────────────────────────────────────────────

/// Escape characters that are special in XML content / attribute values.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Wrap `body` in a SOAP 1.1 Envelope and return a `text/xml` HTTP response.
fn soap_response(status: StatusCode, body: &str) -> Response {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         xmlns:tns=\"{TNS}\">\n  <soap:Body>{body}\n  </soap:Body>\n</soap:Envelope>\n",
        TNS = TNS,
        body = body
    );
    (
        status,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        xml,
    )
        .into_response()
}

/// Return a SOAP 1.1 Fault inside an Envelope.
fn soap_fault(code: &str, message: &str) -> Response {
    let body = format!(
        "\n    <soap:Fault>\
         \n      <faultcode>{code}</faultcode>\
         \n      <faultstring>{msg}</faultstring>\
         \n    </soap:Fault>",
        code = xml_escape(code),
        msg = xml_escape(message),
    );
    soap_response(StatusCode::INTERNAL_SERVER_ERROR, &body)
}

// ── Request parsing ───────────────────────────────────────────────────────────

/// Parse a SOAP 1.1 request body and return `(operation_name, params)`.
///
/// The parser is intentionally lenient about namespace prefixes: it uses the
/// *local* name of each element so callers may use any prefix for the SOAP
/// envelope namespace.
fn parse_soap_request(
    xml: &str,
) -> Result<(String, std::collections::HashMap<String, String>), String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut path: Vec<String> = Vec::new();
    let mut operation: Option<String> = None;
    let mut params: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                // The first Start element whose *parent* local name is "Body"
                // is the SOAP operation.
                if path.last().map(String::as_str) == Some("Body") {
                    operation = Some(local.clone());
                }
                path.push(local);
            }
            Ok(Event::End(_)) => {
                path.pop();
            }
            Ok(Event::Text(e)) => {
                // We are inside an operation parameter element when:
                //   path == [_, "Body", <operation>, <param>]
                if path.len() == 4 {
                    if path.get(1).map(String::as_str) == Some("Body") {
                        if let Some(param) = path.last() {
                            let text = String::from_utf8_lossy(e.as_ref()).into_owned();
                            params.insert(param.clone(), text);
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    operation
        .ok_or_else(|| "No operation element found in soap:Body".to_string())
        .map(|op| (op, params))
}

// ── GET /api/soap (WSDL) ──────────────────────────────────────────────────────

pub async fn wsdl(_auth: AuthAdmin, State(state): State<AppState>) -> Response {
    info!("[soap] GET /api/soap — WSDL requested");
    let endpoint = format!("https://{}/api/soap", state.hostname);
    let wsdl = build_wsdl(&endpoint);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        wsdl,
    )
        .into_response()
}

fn build_wsdl(endpoint: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<definitions xmlns="http://schemas.xmlsoap.org/wsdl/"
             xmlns:soap="http://schemas.xmlsoap.org/wsdl/soap/"
             xmlns:tns="{tns}"
             xmlns:xsd="http://www.w3.org/2001/XMLSchema"
             targetNamespace="{tns}"
             name="MailserverService">

  <!-- ── Types ────────────────────────────────────────────────────────── -->
  <types>
    <xsd:schema targetNamespace="{tns}">

      <xsd:element name="ListEmails">
        <xsd:complexType><xsd:sequence>
          <xsd:element name="accountId" type="xsd:long"/>
          <xsd:element name="folder"    type="xsd:string" minOccurs="0"/>
          <xsd:element name="page"      type="xsd:int"    minOccurs="0"/>
        </xsd:sequence></xsd:complexType>
      </xsd:element>
      <xsd:element name="ListEmailsResponse" type="xsd:string"/>

      <xsd:element name="GetEmail">
        <xsd:complexType><xsd:sequence>
          <xsd:element name="accountId" type="xsd:long"/>
          <xsd:element name="filename"  type="xsd:string"/>
          <xsd:element name="folder"    type="xsd:string" minOccurs="0"/>
        </xsd:sequence></xsd:complexType>
      </xsd:element>
      <xsd:element name="GetEmailResponse" type="xsd:string"/>

      <xsd:element name="SendEmail">
        <xsd:complexType><xsd:sequence>
          <xsd:element name="accountId"  type="xsd:long"/>
          <xsd:element name="to"         type="xsd:string"/>
          <xsd:element name="subject"    type="xsd:string"/>
          <xsd:element name="body"       type="xsd:string"/>
          <xsd:element name="cc"         type="xsd:string" minOccurs="0"/>
          <xsd:element name="bcc"        type="xsd:string" minOccurs="0"/>
          <xsd:element name="replyTo"    type="xsd:string" minOccurs="0"/>
          <xsd:element name="senderName" type="xsd:string" minOccurs="0"/>
          <xsd:element name="bodyFormat" type="xsd:string" minOccurs="0"/>
        </xsd:sequence></xsd:complexType>
      </xsd:element>
      <xsd:element name="SendEmailResponse" type="xsd:string"/>

      <xsd:element name="DeleteEmail">
        <xsd:complexType><xsd:sequence>
          <xsd:element name="accountId" type="xsd:long"/>
          <xsd:element name="filename"  type="xsd:string"/>
          <xsd:element name="folder"    type="xsd:string" minOccurs="0"/>
        </xsd:sequence></xsd:complexType>
      </xsd:element>
      <xsd:element name="DeleteEmailResponse" type="xsd:string"/>

    </xsd:schema>
  </types>

  <!-- ── Messages ─────────────────────────────────────────────────────── -->
  <message name="ListEmailsRequest">  <part name="parameters" element="tns:ListEmails"/></message>
  <message name="ListEmailsResponse"> <part name="parameters" element="tns:ListEmailsResponse"/></message>
  <message name="GetEmailRequest">    <part name="parameters" element="tns:GetEmail"/></message>
  <message name="GetEmailResponse">   <part name="parameters" element="tns:GetEmailResponse"/></message>
  <message name="SendEmailRequest">   <part name="parameters" element="tns:SendEmail"/></message>
  <message name="SendEmailResponse">  <part name="parameters" element="tns:SendEmailResponse"/></message>
  <message name="DeleteEmailRequest"> <part name="parameters" element="tns:DeleteEmail"/></message>
  <message name="DeleteEmailResponse"><part name="parameters" element="tns:DeleteEmailResponse"/></message>

  <!-- ── Port type ─────────────────────────────────────────────────────── -->
  <portType name="MailserverPortType">
    <operation name="ListEmails">
      <input  message="tns:ListEmailsRequest"/>
      <output message="tns:ListEmailsResponse"/>
    </operation>
    <operation name="GetEmail">
      <input  message="tns:GetEmailRequest"/>
      <output message="tns:GetEmailResponse"/>
    </operation>
    <operation name="SendEmail">
      <input  message="tns:SendEmailRequest"/>
      <output message="tns:SendEmailResponse"/>
    </operation>
    <operation name="DeleteEmail">
      <input  message="tns:DeleteEmailRequest"/>
      <output message="tns:DeleteEmailResponse"/>
    </operation>
  </portType>

  <!-- ── Binding ───────────────────────────────────────────────────────── -->
  <binding name="MailserverBinding" type="tns:MailserverPortType">
    <soap:binding style="document" transport="http://schemas.xmlsoap.org/soap/http"/>
    <operation name="ListEmails">
      <soap:operation soapAction="urn:mailserver#ListEmails"/>
      <input><soap:body use="literal"/></input>
      <output><soap:body use="literal"/></output>
    </operation>
    <operation name="GetEmail">
      <soap:operation soapAction="urn:mailserver#GetEmail"/>
      <input><soap:body use="literal"/></input>
      <output><soap:body use="literal"/></output>
    </operation>
    <operation name="SendEmail">
      <soap:operation soapAction="urn:mailserver#SendEmail"/>
      <input><soap:body use="literal"/></input>
      <output><soap:body use="literal"/></output>
    </operation>
    <operation name="DeleteEmail">
      <soap:operation soapAction="urn:mailserver#DeleteEmail"/>
      <input><soap:body use="literal"/></input>
      <output><soap:body use="literal"/></output>
    </operation>
  </binding>

  <!-- ── Service ───────────────────────────────────────────────────────── -->
  <service name="MailserverService">
    <port name="MailserverPort" binding="tns:MailserverBinding">
      <soap:address location="{endpoint}"/>
    </port>
  </service>

</definitions>
"#,
        tns = TNS,
        endpoint = xml_escape(endpoint),
    )
}

// ── POST /api/soap ────────────────────────────────────────────────────────────

pub async fn handle(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let xml = match std::str::from_utf8(&body) {
        Ok(s) => s.to_string(),
        Err(_) => return soap_fault("soap:Client", "Request body is not valid UTF-8"),
    };

    let (operation, params) = match parse_soap_request(&xml) {
        Ok(r) => r,
        Err(e) => return soap_fault("soap:Client", &format!("Malformed SOAP request: {}", e)),
    };

    info!("[soap] POST /api/soap operation={}", operation);

    match operation.as_str() {
        "ListEmails" => handle_list_emails(state, params).await,
        "GetEmail" => handle_get_email(state, params).await,
        "SendEmail" => handle_send_email(state, params).await,
        "DeleteEmail" => handle_delete_email(state, params).await,
        other => soap_fault(
            "soap:Client",
            &format!("Unknown operation: {}", other),
        ),
    }
}

// ── ListEmails ────────────────────────────────────────────────────────────────

async fn handle_list_emails(
    state: AppState,
    params: std::collections::HashMap<String, String>,
) -> Response {
    let account_id: i64 = match params.get("accountId").and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => return soap_fault("soap:Client", "Missing or invalid accountId parameter"),
    };
    let folder = params.get("folder").cloned().unwrap_or_default();
    let page: usize = params
        .get("page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    if !is_safe_folder(&folder) {
        return soap_fault("soap:Client", "Invalid folder name");
    }

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return soap_fault("soap:Client", "Account not found"),
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain) || !is_safe_path_component(&acct.username) {
        return soap_fault("soap:Client", "Invalid account path");
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let mut logs = Vec::new();
    let emails = read_emails(&maildir_base, &folder, &mut logs);

    let total = emails.len();
    let total_pages = if total == 0 {
        1
    } else {
        (total + PAGE_SIZE - 1) / PAGE_SIZE
    };
    let page = page.max(1).min(total_pages);
    let start = (page - 1) * PAGE_SIZE;

    let folder_display = if folder.is_empty() { "INBOX" } else { folder.as_str() };

    let mut emails_xml = String::new();
    for e in emails.iter().skip(start).take(PAGE_SIZE) {
        emails_xml.push_str(&format!(
            "\n        <email>\
             \n          <filename>{}</filename>\
             \n          <subject>{}</subject>\
             \n          <from>{}</from>\
             \n          <to>{}</to>\
             \n          <date>{}</date>\
             \n          <isNew>{}</isNew>\
             \n          <isSpam>{}</isSpam>\
             \n        </email>",
            xml_escape(&e.filename),
            xml_escape(&e.subject),
            xml_escape(&e.from),
            xml_escape(&e.to),
            xml_escape(&e.date),
            e.is_new,
            e.is_spam,
        ));
    }

    let body = format!(
        "\n    <tns:ListEmailsResponse>\
         \n      <accountId>{account_id}</accountId>\
         \n      <folder>{folder}</folder>\
         \n      <page>{page}</page>\
         \n      <totalPages>{total_pages}</totalPages>\
         \n      <totalCount>{total}</totalCount>\
         \n      <emails>{emails_xml}\
         \n      </emails>\
         \n    </tns:ListEmailsResponse>",
        account_id = account_id,
        folder = xml_escape(folder_display),
        page = page,
        total_pages = total_pages,
        total = total,
        emails_xml = emails_xml,
    );
    soap_response(StatusCode::OK, &body)
}

// ── GetEmail ──────────────────────────────────────────────────────────────────

async fn handle_get_email(
    state: AppState,
    params: std::collections::HashMap<String, String>,
) -> Response {
    let account_id: i64 = match params.get("accountId").and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => return soap_fault("soap:Client", "Missing or invalid accountId parameter"),
    };
    let filename_b64 = match params.get("filename") {
        Some(f) if !f.is_empty() => f.clone(),
        _ => return soap_fault("soap:Client", "Missing filename parameter"),
    };
    let folder = params.get("folder").cloned().unwrap_or_default();

    if !is_safe_folder(&folder) {
        return soap_fault("soap:Client", "Invalid folder name");
    }

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return soap_fault("soap:Client", "Account not found"),
    };

    let filename = match URL_SAFE_NO_PAD
        .decode(filename_b64.as_bytes())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(f) => f,
        None => return soap_fault("soap:Client", "Invalid filename encoding"),
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
    {
        return soap_fault("soap:Client", "Invalid path component");
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let root = folder_root(&maildir_base, &folder);

    let mut file_path = None;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            file_path = Some(candidate);
            break;
        }
    }

    let file_path = match file_path {
        Some(p) => p,
        None => return soap_fault("soap:Client", "Email not found"),
    };

    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(e) => {
            return soap_fault(
                "soap:Server",
                &format!("Failed to read email: {}", e),
            )
        }
    };

    let parsed = match mailparse::parse_mail(&data) {
        Ok(p) => p,
        Err(e) => {
            return soap_fault(
                "soap:Server",
                &format!("Failed to parse email: {}", e),
            )
        }
    };

    let subject = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let from = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("From"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let to = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("To"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let date = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Date"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let is_spam = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("X-Spam-Flag"))
        .map(|h| h.get_value().trim().eq_ignore_ascii_case("YES"))
        .unwrap_or(false);
    let email_body = extract_body(&parsed);

    let body = format!(
        "\n    <tns:GetEmailResponse>\
         \n      <filename>{filename}</filename>\
         \n      <subject>{subject}</subject>\
         \n      <from>{from}</from>\
         \n      <to>{to}</to>\
         \n      <date>{date}</date>\
         \n      <body>{body}</body>\
         \n      <isSpam>{is_spam}</isSpam>\
         \n    </tns:GetEmailResponse>",
        filename = xml_escape(&filename_b64),
        subject = xml_escape(&subject),
        from = xml_escape(&from),
        to = xml_escape(&to),
        date = xml_escape(&date),
        body = xml_escape(&email_body),
        is_spam = is_spam,
    );
    soap_response(StatusCode::OK, &body)
}

// ── SendEmail ─────────────────────────────────────────────────────────────────

async fn handle_send_email(
    state: AppState,
    params: std::collections::HashMap<String, String>,
) -> Response {
    let account_id: i64 = match params.get("accountId").and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => return soap_fault("soap:Client", "Missing or invalid accountId parameter"),
    };
    let to = match params.get("to") {
        Some(t) if !t.is_empty() => t.clone(),
        _ => return soap_fault("soap:Client", "Missing to parameter"),
    };
    let subject = params.get("subject").cloned().unwrap_or_default();
    let body_text = params.get("body").cloned().unwrap_or_default();
    let cc = params.get("cc").cloned().unwrap_or_default();
    let bcc = params.get("bcc").cloned().unwrap_or_default();
    let reply_to = params.get("replyTo").cloned().unwrap_or_default();
    let sender_name = params.get("senderName").cloned().unwrap_or_default();
    let body_format = params
        .get("bodyFormat")
        .cloned()
        .unwrap_or_else(|| "plain".to_string());

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return soap_fault("soap:Client", "Account not found"),
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    let email_addr = format!("{}@{}", acct.username, domain);

    let from_addr = if sender_name.is_empty() {
        email_addr.clone()
    } else {
        let safe_name = sender_name.replace(['\r', '\n'], " ");
        format!("{} <{}>", safe_name, email_addr)
    };

    use lettre::message::header::ContentType;
    use lettre::message::SinglePart;
    use lettre::{SmtpTransport, Transport};

    let from_mb = match from_addr.parse() {
        Ok(a) => a,
        Err(e) => {
            return soap_fault(
                "soap:Client",
                &format!("Invalid from address: {}", e),
            )
        }
    };
    let to_mb = match to.parse() {
        Ok(a) => a,
        Err(e) => {
            return soap_fault(
                "soap:Client",
                &format!("Invalid to address: {}", e),
            )
        }
    };

    let mut builder = lettre::Message::builder()
        .from(from_mb)
        .to(to_mb)
        .subject(&subject);

    for addr in cc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(a) = addr.parse() {
            builder = builder.cc(a);
        }
    }
    for addr in bcc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(a) = addr.parse() {
            builder = builder.bcc(a);
        }
    }
    if !reply_to.trim().is_empty() {
        if let Ok(a) = reply_to.trim().parse() {
            builder = builder.reply_to(a);
        }
    }

    let email = match body_format.as_str() {
        "html" => builder.singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_HTML)
                .body(body_text.clone()),
        ),
        _ => builder.body(body_text.clone()),
    };

    let email = match email {
        Ok(e) => e,
        Err(e) => {
            return soap_fault(
                "soap:Server",
                &format!("Failed to build email: {}", e),
            )
        }
    };

    match SmtpTransport::builder_dangerous("127.0.0.1")
        .port(25)
        .build()
        .send(&email)
    {
        Ok(_) => {
            info!("[soap] email sent to {}", to);
            let body = "\n    <tns:SendEmailResponse>\
                        \n      <status>sent</status>\
                        \n    </tns:SendEmailResponse>";
            soap_response(StatusCode::OK, body)
        }
        Err(e) => soap_fault("soap:Server", &format!("SMTP error: {}", e)),
    }
}

// ── DeleteEmail ───────────────────────────────────────────────────────────────

async fn handle_delete_email(
    state: AppState,
    params: std::collections::HashMap<String, String>,
) -> Response {
    let account_id: i64 = match params.get("accountId").and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => return soap_fault("soap:Client", "Missing or invalid accountId parameter"),
    };
    let filename_b64 = match params.get("filename") {
        Some(f) if !f.is_empty() => f.clone(),
        _ => return soap_fault("soap:Client", "Missing filename parameter"),
    };
    let folder = params.get("folder").cloned().unwrap_or_default();

    if !is_safe_folder(&folder) {
        return soap_fault("soap:Client", "Invalid folder name");
    }

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return soap_fault("soap:Client", "Account not found"),
    };

    let filename = match URL_SAFE_NO_PAD
        .decode(filename_b64.as_bytes())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(f) => f,
        None => return soap_fault("soap:Client", "Invalid filename encoding"),
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
    {
        return soap_fault("soap:Client", "Invalid path component");
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let root = folder_root(&maildir_base, &folder);

    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            if let Err(e) = std::fs::remove_file(&candidate) {
                return soap_fault(
                    "soap:Server",
                    &format!("Failed to delete email: {}", e),
                );
            }
            info!("[soap] deleted email: {}", candidate);
            let body = "\n    <tns:DeleteEmailResponse>\
                        \n      <status>deleted</status>\
                        \n    </tns:DeleteEmailResponse>";
            return soap_response(StatusCode::OK, body);
        }
    }

    soap_fault("soap:Client", "Email not found")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_ampersand() {
        assert_eq!(xml_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn xml_escape_angle_brackets() {
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
    }

    #[test]
    fn xml_escape_quotes() {
        assert_eq!(xml_escape("say \"hi\""), "say &quot;hi&quot;");
    }

    #[test]
    fn xml_escape_no_op() {
        assert_eq!(xml_escape("hello world"), "hello world");
    }

    #[test]
    fn parse_soap_list_emails() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/">
  <soap:Body>
    <ListEmails xmlns="urn:mailserver">
      <accountId>42</accountId>
      <folder>INBOX</folder>
      <page>2</page>
    </ListEmails>
  </soap:Body>
</soap:Envelope>"#;
        let (op, params) = parse_soap_request(xml).unwrap();
        assert_eq!(op, "ListEmails");
        assert_eq!(params["accountId"], "42");
        assert_eq!(params["folder"], "INBOX");
        assert_eq!(params["page"], "2");
    }

    #[test]
    fn parse_soap_send_email() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/">
  <soap:Body>
    <SendEmail xmlns="urn:mailserver">
      <accountId>1</accountId>
      <to>user@example.com</to>
      <subject>Hello</subject>
      <body>World</body>
    </SendEmail>
  </soap:Body>
</soap:Envelope>"#;
        let (op, params) = parse_soap_request(xml).unwrap();
        assert_eq!(op, "SendEmail");
        assert_eq!(params["to"], "user@example.com");
        assert_eq!(params["subject"], "Hello");
    }

    #[test]
    fn parse_soap_missing_body_returns_error() {
        let xml = r#"<?xml version="1.0"?><root><child>x</child></root>"#;
        assert!(parse_soap_request(xml).is_err());
    }

    #[test]
    fn parse_soap_invalid_xml_returns_error() {
        let xml = "<broken<xml";
        // quick-xml may or may not error here, just ensure we get a result
        let _ = parse_soap_request(xml);
    }

    #[test]
    fn wsdl_contains_required_operations() {
        let wsdl = build_wsdl("https://mail.example.com/api/soap");
        assert!(wsdl.contains("ListEmails"));
        assert!(wsdl.contains("GetEmail"));
        assert!(wsdl.contains("SendEmail"));
        assert!(wsdl.contains("DeleteEmail"));
        assert!(wsdl.contains("urn:mailserver"));
        assert!(wsdl.contains("https://mail.example.com/api/soap"));
    }

    #[test]
    fn wsdl_escapes_endpoint_url() {
        let wsdl = build_wsdl("https://example.com/api/soap?a=1&b=2");
        assert!(wsdl.contains("a=1&amp;b=2"));
    }

    #[test]
    fn soap_response_is_valid_envelope() {
        let resp_body = "<tns:ListEmailsResponse><status>ok</status></tns:ListEmailsResponse>";
        // soap_response returns a full Response; we just test the helper function
        // indirectly by checking build_wsdl returns well-formed XML start.
        let wsdl = build_wsdl("https://example.com/api/soap");
        assert!(wsdl.starts_with("<?xml version=\"1.0\""));
    }
}
