use crate::config::ConfigFile;

pub struct CliSettings {
    pub file: ConfigFile,
    pub config_path: std::path::PathBuf,
}

impl CliSettings {
    pub fn gateway_url(&self) -> String {
        self.file.gateway_http_url()
    }

    pub fn api_key(&self) -> Option<String> {
        self.file.gateway.api_key.clone()
    }

    pub fn admin_token(&self) -> Option<String> {
        self.file.gateway.admin_token.clone()
    }
}
