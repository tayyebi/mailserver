use serde::Deserialize;

#[derive(Deserialize)]
pub struct DomainForm {
    pub domain: String,
    #[serde(default)]
    pub footer_html: String,
}

#[derive(Deserialize)]
pub struct DomainEditForm {
    pub domain: String,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub footer_html: String,
}

#[derive(Deserialize)]
pub struct AccountForm {
    pub domain_id: i64,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub quota: Option<i64>,
}

#[derive(Deserialize)]
pub struct AccountEditForm {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub quota: Option<i64>,
}

#[derive(Deserialize)]
pub struct AliasForm {
    pub domain_id: i64,
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub tracking_enabled: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

#[derive(Deserialize)]
pub struct AliasEditForm {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub tracking_enabled: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

#[derive(Deserialize)]
pub struct PasswordForm {
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
}

#[derive(Deserialize)]
pub struct TotpEnableForm {
    pub secret: String,
    pub code: String,
}

#[derive(Deserialize)]
pub struct PixelQuery {
    #[serde(default)]
    pub id: String,
}

#[derive(Deserialize)]
pub struct PixelSettingsForm {
    pub pixel_host: String,
    #[serde(default)]
    pub pixel_port: Option<i64>,
}
