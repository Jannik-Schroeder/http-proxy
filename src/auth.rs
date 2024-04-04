use std::env;
use std::collections::HashMap;

#[derive(Clone)]
pub struct Auth {
    credentials: HashMap<String, String>,
}

impl Auth {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let username = env::var("PROXY_USER")?;
        let password = env::var("PROXY_PASS")?;

        let mut credentials = HashMap::new();
        credentials.insert(username, password);

        Ok(Auth { credentials })
    }

    pub fn validate(&self, received_username: &str, received_password: &str) -> Result<bool, ()> {
        println!("received_username: {}", received_username);
        if let Some(expected_password) = self.credentials.get(received_username) {
            if received_password == expected_password {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }
}
