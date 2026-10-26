use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Settings {
    #[serde(default = "default_server_domain")]
    pub server_domain: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    pub tls_cert: FileOrValue,
    pub tls_key: FileOrValue,
    pub webhook: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum FileOrValue {
    Value { value: String },
    File { path: String },
}

const fn default_smtp_port() -> u16 {
    25
}

fn default_server_domain() -> String {
    "smtps2http.local".to_string()
}
