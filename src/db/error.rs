quick_error! {
    #[derive(Debug)]
    pub enum DataBaseError {
        InvalidPostgreSQLVersion(actual: String, required: String) {
            display(r#"Invalid PostgreSQL version: "{}", required: "{}""#, actual, required)
        }
        InvalidSchemaItem(name: String, actual: String, expected: String) {
            display(r#"Invalid {}: found "{}", expected "{}""#, name, actual, expected)
        }
    }
}
