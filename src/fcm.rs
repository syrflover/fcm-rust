use std::{fmt::Display, path::Path};

use http::{header, Method, StatusCode};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    oauth::{Credential, GoogleOAuth2},
    Error,
};

pub struct FirebaseCloudMessaging {
    project_id: String,
    oauth2: GoogleOAuth2,
    client: Client,
}

impl FirebaseCloudMessaging {
    pub fn from_credential_path<P>(p: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self::from_credential(Credential::from_path(p))
    }

    pub fn from_env() -> Self {
        Self::from_credential(Credential::from_env())
    }

    pub fn from_credential(cred: Credential) -> Self {
        Self {
            project_id: cred.project_id.clone(),
            oauth2: GoogleOAuth2::from_credential(cred, "https://fcm.googleapis.com/".to_string()),
            client: Client::new(),
        }
    }

    /* pub fn new(firebase_token: impl Into<String>, project_id: impl Into<String>) -> Self {
        let client = Client::new();

        Self {
            firebase_token: firebase_token.into(),
            project_id: project_id.into(),
            client,
        }
    } */

    const BOUNDARY: &'static str = "fcm_rust_sdk";

    fn add_part(
        project_id: impl AsRef<str>,
        oauth2_token: impl AsRef<str>,
        xs: &mut Vec<String>,
        content: Content,
    ) {
        xs.push(format!("--{}", Self::BOUNDARY));
        xs.push("Content-Type: application/http".to_string());
        xs.push("Content-Transfer-Encoding: binary".to_string());
        xs.push(format!("Authorization: Bearer {}", oauth2_token.as_ref()));
        xs.push("".to_string());
        xs.push(format!(
            "POST /v1/projects/{}/messages:send",
            project_id.as_ref()
        ));
        xs.push("Content-Type: application/json".to_string());
        xs.push("accept: application/json".to_string());
        xs.push("".to_string());
        xs.push(serde_json::to_string_pretty(&content).expect("json serialize"));
    }

    fn add_end_boundary(xs: &mut Vec<String>) {
        xs.push(format!("--{}--\r\n", Self::BOUNDARY));
    }

    /// Reference: https://firebase.google.com/docs/cloud-messaging/send-message#send-messages-to-multiple-devices
    pub async fn send_to_devices(
        &self,
        registration_tokens: impl IntoIterator<Item = impl Into<String>>,
        message: Message,
    ) -> crate::Result<Vec<Result<SendMessageSuccessResponse, SendMessageErrorResponse>>> {
        let mut xs = Vec::new();
        let mut batch_len = 0;

        for registration_token in registration_tokens {
            batch_len += 1;
            let content = Content::new(registration_token.into(), message.clone());

            let oauth2_token = self.oauth2.get_or_update_token();
            Self::add_part(&self.project_id, oauth2_token, &mut xs, content);
        }

        Self::add_end_boundary(&mut xs);

        let body = xs.join("\r\n");

        // println!("{body}");

        // curl --data-binary @batch_request.txt -H 'Content-Type: multipart/mixed; boundary="subrequest_boundary"' https://fcm.googleapis.com/batch
        let req = self
            .client
            .request(Method::POST, "https://fcm.googleapis.com/batch")
            .header(
                header::CONTENT_TYPE,
                format!("multipart/mixed; boundary={}", Self::BOUNDARY),
            )
            .body(body);

        // println!("{req:#?}");

        let res = req.send().await?;

        // println!("{res:#?}");

        let res_content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|x| x.to_str().ok())
            .unwrap_or("");

        let res_boundary = Self::parse_boundary(res_content_type);

        match (res.status(), res_boundary) {
            (StatusCode::OK, Some(res_boundary)) => {
                let res = res.text().await?;

                // println!("{res}");

                let res = Self::parse_batch_response(res.trim(), &res_boundary, batch_len)?;

                Ok(res)
            }

            _ => {
                let res = res.text().await?;

                let error: SendMessageErrorResponse =
                    serde_json::from_str(&res).map_err(Error::ResponseDeserialize)?;

                Err(Error::SendMessage(error))
            }
        }
    }

    fn parse_batch_response(
        x: &str,
        boundary: &str,
        batch_len: usize,
    ) -> crate::Result<Vec<Result<SendMessageSuccessResponse, SendMessageErrorResponse>>> {
        // println!("{x}");
        // println!("{batch_len}");

        x.split(&format!("--{boundary}"))
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .take(batch_len)
            .map(Self::parse_response)
            .collect::<Result<Vec<_>, _>>()
    }

    fn parse_response(
        x: &str,
    ) -> crate::Result<Result<SendMessageSuccessResponse, SendMessageErrorResponse>> {
        let x = x.split("\r\n\r\n").last().unwrap_or_default();
        let xx = x.split("\r\n").next().unwrap_or_default();

        // println!("x = {x:?}");
        // println!("xx = {xx:?}");

        match serde_json::from_str(xx) {
            Ok(r) => Ok(Ok(r)),
            Err(_) => {
                if let Ok(r) = serde_json::from_str(x) {
                    Ok(Ok(r))
                } else {
                    let r = serde_json::from_str(x).map_err(Error::ResponseDeserialize)?;
                    Ok(Err(r))
                }
            }
        }
    }

    fn parse_boundary(x: &str) -> Option<String> {
        let r = x
            .split(';')
            .map(|x| x.trim())
            .find(|x| x.starts_with("boundary="))?
            .replacen("boundary=", "batch_", 1);

        Some(r)
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Deserialize)]
pub struct SendMessageSuccessResponse {
    pub name: String,
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Deserialize)]
pub struct SendMessageErrorResponse {
    pub error: SendMessageError,
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Deserialize)]
pub struct SendMessageError {
    pub code: u16,
    pub message: String,
    pub status: String,
}

impl Display for SendMessageErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error.message)
    }
}

#[derive(Debug, Serialize)]
struct Content {
    message: ContentInner,
}

impl Content {
    pub fn new(registration_token: String, message: Message) -> Self {
        let inner = ContentInner::new(registration_token, message);

        Self { message: inner }
    }
}

#[derive(Debug, Serialize)]
struct ContentInner {
    token: String,
    notification: Message,
}

impl ContentInner {
    pub fn new(registration_token: String, message: Message) -> Self {
        Self {
            token: registration_token,
            notification: message,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Message {
    pub title: String,
    pub body: String,
}

impl Message {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FirebaseCloudMessaging, Message, SendMessageError, SendMessageErrorResponse,
        SendMessageSuccessResponse,
    };

    #[tokio::test]
    #[ignore]
    async fn test_send_to_devices() {
        let fcm = FirebaseCloudMessaging::from_credential_path("./firebase.credential.json");

        let registration_token = "";
        let message = Message::new("title", "body");

        let a = fcm.send_to_devices([registration_token], message).await;

        println!("{a:?}")
    }

    #[test]
    fn test_parse_batch_response() {
        let boundary = "batch_nDhMX4IzFTDLsCJ3kHH7v_44ua-aJT6q";

        let responses = format!(
            r#"--{boundary}
Content-Type: application/http
Content-ID: response-

HTTP/1.1 200 OK
Content-Type: application/json; charset=UTF-8
Vary: Origin
Vary: X-Origin
Vary: Referer

{{
    "name": "projects/35006771263/messages/0:1570471792141125%43c11b7043c11b70"
}}

--{boundary}
Content-Type: application/http
Content-ID: response-

HTTP/1.1 400 BAD REQUEST
Content-Type: application/json; charset=UTF-8
Vary: Origin
Vary: X-Origin
Vary: Referer

{{
    "error": {{
        "code": 400,
        "message": "The registration token is not a valid FCM registration token",
        "status": "INVALID_ARGUMENT"
  }}
}}

--{boundary}
Content-Type: application/http
Content-ID: response-

HTTP/1.1 200 OK
Content-Type: application/json; charset=UTF-8
Vary: Origin
Vary: X-Origin
Vary: Referer

{{
    "name": "projects/35006771263/messages/0:1570471792141696%43c11b7043c11b70"
}}

--{boundary}--"#
        )
        .replace('\n', "\r\n");

        let actual = FirebaseCloudMessaging::parse_batch_response(&responses, boundary, 3).unwrap();

        let expected = vec![
            Ok(SendMessageSuccessResponse {
                name: "projects/35006771263/messages/0:1570471792141125%43c11b7043c11b70"
                    .to_string(),
            }),
            Err(SendMessageErrorResponse {
                error: SendMessageError {
                    code: 400,
                    message: "The registration token is not a valid FCM registration token"
                        .to_string(),
                    status: "INVALID_ARGUMENT".to_string(),
                },
            }),
            Ok(SendMessageSuccessResponse {
                name: "projects/35006771263/messages/0:1570471792141696%43c11b7043c11b70"
                    .to_string(),
            }),
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_parse_response() {
        let success = r#"Content-Type: application/http
Content-ID: response-

HTTP/1.1 200 OK
Content-Type: application/json; charset=UTF-8
Vary: Origin
Vary: X-Origin
Vary: Referer

{
    "name": "projects/35006771263/messages/0:1570471792141696%43c11b7043c11b70"
}"#
        .replace('\n', "\r\n");

        let actual = FirebaseCloudMessaging::parse_response(&success)
            .unwrap()
            .unwrap();

        let expected = SendMessageSuccessResponse {
            name: "projects/35006771263/messages/0:1570471792141696%43c11b7043c11b70".to_string(),
        };

        assert_eq!(actual, expected);

        let error = r#"{boundary}
Content-Type: application/http
Content-ID: response-

HTTP/1.1 400 BAD REQUEST
Content-Type: application/json; charset=UTF-8
Vary: Origin
Vary: X-Origin
Vary: Referer

{
    "error": {
        "code": 400,
        "message": "The registration token is not a valid FCM registration token",
        "status": "INVALID_ARGUMENT"
    }
}"#
        .replace('\n', "\r\n");

        let actual = FirebaseCloudMessaging::parse_response(&error)
            .unwrap()
            .unwrap_err();

        let expected = SendMessageErrorResponse {
            error: SendMessageError {
                code: 400,
                message: "The registration token is not a valid FCM registration token".to_string(),
                status: "INVALID_ARGUMENT".to_string(),
            },
        };

        assert_eq!(actual, expected);
    }
}
