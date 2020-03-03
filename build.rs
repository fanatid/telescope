use std::fs;

fn main() {
    generate_cli_name_version();
}

fn generate_cli_name_version() {
    let version = load_version();
    let git_rev = load_git_rev();
    let app_version = format!("{}@{}", version, git_rev);

    fs::write("./src/args.rs-version", app_version)
        .expect("Failed to create CLI file with version");
}

fn load_version() -> String {
    include_str!("./Cargo.toml")
        .parse::<toml::value::Value>()
        .expect("Failed to parse Cargo.toml")
        .get("package")
        .expect("Cargo.toml should have `package` section")
        .get("version")
        .expect("Cargo.toml should have field `version` in `package` section")
        .as_str()
        .expect("Field `version` from `package` section not a String")
        .to_owned()
}

fn load_git_rev() -> String {
    let repo = git2::Repository::open("./").expect("Failed to open git repository");

    let mut rev = repo
        .head()
        .expect("Failed get HEAD")
        .target()
        .expect("Failed to get OID")
        .as_bytes()
        .iter()
        .take(4)
        .fold(String::with_capacity(8), |s, b| s + &format!("{:02x}", b));
    rev.truncate(7);

    let mut status_options = git2::StatusOptions::new();
    status_options.include_ignored(false);
    let statuses = repo
        .statuses(Some(&mut status_options))
        .expect("Failed to get statuses");

    if statuses.is_empty() {
        rev
    } else {
        format!("{}-modified", rev)
    }
}
