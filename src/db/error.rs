quick_error! {
    #[derive(Debug)]
    pub enum DBError {
        InvalidVersion(actual: String, required: String) {
            display(r#"Invalid PostgreSQL version: "{}", required: "{}""#, actual, required)
        }
    }
}
