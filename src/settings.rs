use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Settings {
    #[serde(default = "default_server_domain")]
    pub server_domain: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub generate_tls_cert: bool,
    #[serde(default)]
    pub tls_cert: Option<FileOrValue>,
    #[serde(default)]
    pub tls_key: Option<FileOrValue>,
    pub webhook: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum FileOrValue {
    Value { value: String },
    File { file: String },
}

const fn default_smtp_port() -> u16 {
    25
}

fn default_server_domain() -> String {
    "smtp2http.local".to_string()
}
