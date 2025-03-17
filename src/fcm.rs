use std::{fmt::Display, path::Path};

use http::{header, Method, StatusCode};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::oauth::{Credential, GoogleOAuth2};

mod sealed {
    use super::*;

    #[derive(Debug, Serialize)]
    pub struct Body<'a, D>
    where
        D: Serialize,
    {
        pub message: InnerBody<'a, D>,
    }

    #[derive(Debug, Serialize)]
    pub struct InnerBody<'a, D>
    where
        D: Serialize,
    {
        pub token: &'a str,
        pub notification: &'a Message,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub data: Option<&'a D>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub apns: Option<InnerApnsOptions>,
    }

    #[derive(Debug, Serialize)]
    pub struct InnerApnsOptions {
        pub headers: ApnsHeaders,
        pub payload: ApnsPayload,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct ApnsHeaders {
        /// u8.to_string()
        pub apns_priority: String,
    }

    #[derive(Debug, Serialize)]
    pub struct ApnsPayload {
        pub aps: Aps,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Aps {
        pub mutable_content: u8,
        pub content_available: u8,
    }
}

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
        apns_options: Option<&ApnsOptions>,
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
        let body = sealed::Body {
            message: sealed::InnerBody {
                token: registration_token,
                notification: message,
                apns: apns_options.map(|x| x.to_inner()),
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

#[derive(Debug, Clone, Default)]
pub struct ApnsOptions {
    pub mutable_content: Option<bool>,
    pub content_available: Option<bool>,
    pub priority: Option<ApnsPriority>,
}

impl ApnsOptions {
    fn to_inner(&self) -> sealed::InnerApnsOptions {
        // headers

        let apns_priority = match self.priority.unwrap_or(ApnsPriority::High) {
            ApnsPriority::High => 10,
            ApnsPriority::Normal => 5,
            ApnsPriority::Low => 1,
        }
        .to_string();

        // payload

        let mutable_content = if self.mutable_content.unwrap_or(false) {
            1
        } else {
            0
        };
        let content_available = if self.content_available.unwrap_or(false) {
            1
        } else {
            0
        };

        sealed::InnerApnsOptions {
            headers: sealed::ApnsHeaders { apns_priority },
            payload: sealed::ApnsPayload {
                aps: sealed::Aps {
                    mutable_content,
                    content_available,
                },
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ApnsPriority {
    High,
    Normal,
    Low,
}

#[cfg(test)]
mod tests {
    use super::{ApnsOptions, ApnsPriority, FirebaseCloudMessaging, Message};

    #[tokio::test]
    #[ignore]
    async fn test_send() {
        let fcm = FirebaseCloudMessaging::from_credential_path("./firebase.credential.json");

        let registration_tokens = [];

        for registration_token in registration_tokens {
            let message = Message::new(
                "좋아하실만한 작품이 올라왔어요 (테스트)",
                "저주 때문에 MP가 부족해요!!",
            );

            #[derive(Debug, serde::Serialize)]
            struct Data {
                thumbnail: &'static str,
                book_id: &'static str,
            }

            let data = Some(Data {
                thumbnail: "https://file.madome.app/image/library/3277177/thumbnail",
                book_id: "3277177",
            });

            let res = fcm
                .send(
                    registration_token,
                    &message,
                    Some(&ApnsOptions {
                        mutable_content: Some(true),
                        content_available: Some(true),
                        priority: Some(ApnsPriority::High),
                    }),
                    data.as_ref(),
                )
                .await;

            println!("{res:?}");

            res.unwrap();
        }
    }
}
