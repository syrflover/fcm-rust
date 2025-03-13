//! Google OAuth2 Token Manager
//!
//! Reference: https://developers.google.com/identity/protocols/oauth2/service-account#jwt-auth

use std::{
    env,
    fmt::Debug,
    fs::File,
    io::BufReader,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{Algorithm, EncodingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
pub struct Credential {
    // pub(crate) r#type: String,
    pub(crate) project_id: String,
    pub(crate) private_key_id: String,
    pub(crate) private_key: String,
    pub(crate) client_email: String,
    // pub(crate) client_id: String,
    // pub(crate) auth_uri: String,
    // pub(crate) token_uri: String,
    // pub(crate) auth_provider_x509_cert_url: String,
    // pub(crate) client_x509_cert_url: String,
}

impl Credential {
    pub fn from_path<P>(p: P) -> Self
    where
        P: AsRef<Path>,
    {
        let file = File::open(p).expect("failed `File::open` to credential file");
        let buf_reader = BufReader::new(file);
        let cred: Credential =
            serde_json::from_reader(buf_reader).expect("failed deserialize from credential file");

        cred
    }

    pub fn from_env() -> Self {
        let project_id = env::var("FIREBASE_PROJECT_ID").expect("please set FIREBASE_PROJECT_ID");
        let private_key_id =
            env::var("FIREBASE_PRIVATE_KEY_ID").expect("please set FIREBASE_PRIVATE_KEY_ID");
        let private_key =
            env::var("FIREBASE_PRIVATE_KEY").expect("please set FIREBASE_PRIVATE_KEY");
        let client_email =
            env::var("FIREBASE_CLIENT_EMAIL").expect("please set FIREBASE_CLIENT_EMAIL");

        Self {
            project_id,
            private_key_id,
            private_key,
            client_email,
        }
    }
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("firebase credential")
    }
}

pub struct Header {
    alg: Algorithm,
    // typ: String,
    /// `private_key_id` from `credential.json`
    kid: String,
}
impl Header {
    pub fn new(private_key_id: String) -> Self {
        Self {
            alg: Algorithm::RS256,
            // typ: "JWT".to_string(),
            kid: private_key_id,
        }
    }
}

impl From<Header> for jsonwebtoken::Header {
    fn from(header: Header) -> Self {
        Self {
            alg: header.alg,
            kid: Some(header.kid),
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Payload {
    /// `client_email` from `credential.json`
    sub: String,
    /// `client_email` from `credential.json`
    iss: String,
    /// `https://fcm.googleapis.com/`
    aud: String,
    iat: u64,
    /// `iat` + `3600`
    exp: u64,
}

fn now() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    now.as_secs()
}

impl Payload {
    pub fn new(client_email: String, service_endpoint: String) -> Self {
        let iat = now();
        let exp = iat + 3600;

        Self {
            sub: client_email.clone(),
            iss: client_email,
            aud: service_endpoint,
            iat,
            exp,
        }
    }
}

pub struct GoogleOAuth2 {
    /// `private_key_id` from `credential.json`
    private_key_id: String,
    /// `private_key` from `credential.json`
    private_key: String,
    /// `client_email` from `credential.json`
    client_email: String,
    /// e.g. `https://fcm.googleapis.com/`
    service_endpoint: String,

    oauth2_token: RwLock<Option<String>>,
}

impl GoogleOAuth2 {
    pub fn from_credential_path<P>(p: P, service_endpoint: impl Into<String>) -> Self
    where
        P: AsRef<Path>,
    {
        Self::from_credential(Credential::from_path(p), service_endpoint)
    }

    pub fn from_env(service_endpoint: impl Into<String>) -> Self {
        Self::from_credential(Credential::from_env(), service_endpoint)
    }

    pub fn from_credential(cred: Credential, service_endpoint: impl Into<String>) -> Self {
        let this = Self {
            client_email: cred.client_email,
            private_key_id: cred.private_key_id,
            private_key: cred.private_key,
            service_endpoint: service_endpoint.into(),
            oauth2_token: Default::default(),
        };

        this.update_token();

        this
    }

    pub fn get_token(&self) -> Option<String> {
        let oauth2_token = self.oauth2_token.read();

        match oauth2_token.clone() {
            Some(oauth2_token) if Self::check(&oauth2_token) => Some(oauth2_token),
            _ => None,
        }
    }

    pub fn update_token(&self) -> String {
        let header = Header::new(self.private_key_id.clone());
        let payload = Payload::new(self.client_email.clone(), self.service_endpoint.clone());

        let oauth2_token = Self::encode(header, payload, self.private_key.as_bytes());

        let mut oauth2_token_holder = self.oauth2_token.write();
        oauth2_token_holder.replace(oauth2_token.clone());

        oauth2_token
    }

    pub fn get_or_update_token(&self) -> String {
        match self.get_token() {
            Some(oauth2_token) => oauth2_token,
            None => self.update_token(),
        }
    }

    fn encode(header: Header, payload: Payload, key: &[u8]) -> String {
        // let header = Header::new(self.private_key_id.clone()).into();
        // let payload = Payload::new(self.client_email.clone(), self.service_endpoint.clone());
        let key =
            EncodingKey::from_rsa_pem(key).expect("can't parse `EncodingKey` from private key");

        jsonwebtoken::encode(&header.into(), &payload, &key).unwrap()
    }

    fn decode_payload(oauth2_token: &str) -> Option<Payload> {
        let p = oauth2_token.split('.').nth(1)?;
        let buf = URL_SAFE_NO_PAD.decode(p).ok()?;
        serde_json::from_slice(&buf).ok()
    }

    fn check(oauth2_token: &str) -> bool {
        matches! {
            Self::decode_payload(oauth2_token),
                Some(payload) if now() - payload.iat <= 3420
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::GoogleOAuth2;

    #[tokio::test]
    async fn test() {
        let oauth2 = GoogleOAuth2::from_credential_path(
            "./firebase.credential.json",
            "https://fcm.googleapis.com/",
        );

        let a = oauth2.get_token().unwrap();

        let b = oauth2.get_or_update_token();

        assert_eq!(a, b);

        let c = oauth2.get_token().unwrap();

        assert_eq!(c, a);

        std::thread::sleep(Duration::from_secs(1));

        let d = oauth2.update_token();

        assert_ne!(d, a);

        let e = oauth2.get_or_update_token();

        assert_eq!(e, d);
    }
}
