use std::{fmt::Display, path::Path};

use http::{header, Method, StatusCode};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::oauth::{Credential, GoogleOAuth2};

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

    pub async fn send<D>(
        &self,
        registration_token: &str,
        message: &Message,
        data: Option<&D>,
    ) -> crate::Result<SendMessageSuccessResponse>
    where
        D: Serialize,
    {
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );
        let authorization = format!("Bearer {}", self.oauth2.get_or_update_token());
        let body = Body {
            message: InnerBody {
                token: registration_token,
                notification: message,
                data,
            },
        };

        let response = self
            .client
            .request(Method::POST, &url)
            .header(header::AUTHORIZATION, authorization)
            .body(serde_json::to_vec(&body).unwrap())
            .send()
            .await?;

        let status_code = response.status();

        if status_code != StatusCode::OK {
            let err = serde_json::from_slice::<SendMessageErrorResponse>(&response.bytes().await?)
                .map_err(crate::Error::ResponseDeserialize)?;

            return Err(crate::Error::SendMessage(err));
        }

        let res = serde_json::from_slice(&response.bytes().await?)
            .map_err(crate::Error::ResponseDeserialize)?;

        Ok(res)
    }
}

#[derive(Debug, Deserialize)]
pub struct SendMessageSuccessResponse {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageErrorResponse {
    pub error: SendMessageError,
}

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
struct Body<'a, D>
where
    D: Serialize,
{
    message: InnerBody<'a, D>,
}

#[derive(Debug, Serialize)]
struct InnerBody<'a, D>
where
    D: Serialize,
{
    token: &'a str,
    notification: &'a Message,

    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<&'a D>,
}

#[derive(Debug, Serialize, Clone, Default)]
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
    use super::{FirebaseCloudMessaging, Message};

    #[tokio::test]
    #[ignore]
    async fn test_send_to_devices() {
        let fcm = FirebaseCloudMessaging::from_credential_path("./firebase.credential.json");

        let registration_token = "";
        let message = Message::new("title", "body");

        #[derive(Debug, serde::Serialize)]
        struct Data {
            thumbnail: &'static str,
            book_id: &'static str,
        }

        let data = Some(Data {
            thumbnail: "https://file.madome.app/image/library/2699651/thumbnail",
            book_id: "2699651",
        });

        let res = fcm.send(registration_token, &message, data.as_ref()).await;

        println!("{res:?}")
    }
}
