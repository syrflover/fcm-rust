mod error;
mod fcm;
mod oauth;

pub use error::Error;
pub use fcm::{
    FirebaseCloudMessaging, Message, SendMessageError, SendMessageErrorResponse,
    SendMessageSuccessResponse,
};
pub use oauth::{Credential, GoogleOAuth2};

pub type Result<T> = std::result::Result<T, Error>;
