use crate::fcm::SendMessageErrorResponse;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Response Deserialize: {0}")]
    ResponseDeserialize(serde_json::Error),

    #[error("Reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Send Message: {0}")]
    SendMessage(SendMessageErrorResponse),
}
