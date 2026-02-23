use serde::Deserialize;

#[derive(Deserialize)]
pub struct DomainForm {
    pub domain: String,
    #[serde(default)]
    pub footer_html: String,
    #[serde(default)]
    pub bimi_svg: String,
    #[serde(default)]
    pub unsubscribe_enabled: Option<String>,
}

#[derive(Deserialize)]
pub struct DomainEditForm {
    pub domain: String,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub footer_html: String,
    #[serde(default)]
    pub bimi_svg: String,
    #[serde(default)]
    pub unsubscribe_enabled: Option<String>,
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
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub tracking_enabled: Option<String>,
}

#[derive(Deserialize)]
pub struct AliasEditForm {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub tracking_enabled: Option<String>,
}

#[derive(Deserialize)]
pub struct ForwardingForm {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub keep_copy: Option<String>,
}

#[derive(Deserialize)]
pub struct ForwardingEditForm {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub keep_copy: Option<String>,
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

#[derive(Deserialize)]
pub struct Fail2banSettingForm {
    pub max_attempts: i32,
    pub ban_duration_minutes: i32,
    pub find_time_minutes: i32,
    #[serde(default)]
    pub enabled: Option<String>,
}

#[derive(Deserialize)]
pub struct Fail2banBanForm {
    pub ip_address: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub duration_minutes: Option<i32>,
    #[serde(default)]
    pub permanent: Option<String>,
}

#[derive(Deserialize)]
pub struct Fail2banListForm {
    pub ip_address: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
pub struct Fail2banGlobalToggleForm {
    #[serde(default)]
    pub enabled: Option<String>,
}

#[derive(Deserialize)]
pub struct SpamblToggleForm {
    pub id: i64,
    #[serde(default)]
    pub enabled: Option<String>,
}

#[derive(Deserialize)]
pub struct UnsubscribeQuery {
    #[serde(default)]
    pub token: String,
}

#[derive(Deserialize)]
pub struct WebhookSettingsForm {
    #[serde(default)]
    pub webhook_url: String,
}

#[derive(Deserialize)]
pub struct FeatureToggleForm {
    #[serde(default)]
    pub filter_enabled: Option<String>,
    #[serde(default)]
    pub milter_enabled: Option<String>,
    #[serde(default)]
    pub unsubscribe_enabled: Option<String>,
}

#[derive(Deserialize)]
pub struct RelayForm {
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub port: Option<i32>,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Deserialize)]
pub struct RelayEditForm {
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub port: Option<i32>,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub active: Option<String>,
}

#[derive(Deserialize)]
pub struct RelayAssignmentForm {
    pub assignment_type: String,
    pub pattern: String,
}
