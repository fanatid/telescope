use std::fmt;

#[derive(Debug)]
pub struct CustomError {
    msg: String,
}

impl CustomError {
    pub fn new<S: Into<String>>(msg: S) -> Box<CustomError> {
        Box::new(CustomError { msg: msg.into() })
    }
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for CustomError {}
