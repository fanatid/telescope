use std::fmt;

use crate::AnyError;

#[derive(Debug)]
pub struct CustomError {
    msg: String,
}

impl CustomError {
    #[inline]
    pub fn new<S: Into<String>>(msg: S) -> CustomError {
        CustomError { msg: msg.into() }
    }

    #[inline]
    pub fn new_any<S: Into<String>>(msg: S) -> AnyError {
        Box::new(Self::new(msg)).into()
    }
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for CustomError {}
