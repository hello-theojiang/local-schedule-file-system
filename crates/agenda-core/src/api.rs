/// Point d'entrée unique : `Api::call(méthode, json) -> json`.
#[derive(Default)]
pub struct Api;

impl Api {
    pub fn new() -> Self {
        Api
    }

    pub fn call(&self, method: &str, _params: &str) -> String {
        match method {
            "version" => format!("{{\"ok\":true,\"result\":\"{}\"}}", crate::VERSION),
            _ => format!("{{\"ok\":false,\"error\":\"méthode inconnue : {method}\"}}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version() {
        assert!(Api::new().call("version", "{}").contains(crate::VERSION));
    }
}
