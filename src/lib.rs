mod auth;
mod error;
mod fcm;

pub use auth::GoogleOAuth2;
pub use error::Error;
pub use fcm::{
    FirebaseCloudMessaging, Message, SendMessageError, SendMessageErrorResponse,
    SendMessageSuccessResponse,
};

pub type Result<T> = std::result::Result<T, Error>;
