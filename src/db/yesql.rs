use std::collections::HashMap;

use regex::{Regex, RegexBuilder};

#[derive(Debug, PartialEq)]
enum LineType {
    Tag,
    Query,
}

pub fn parse_sql(data: &str) -> HashMap<String, String> {
    let mut queries = HashMap::new();

    let mut last_type: Option<LineType> = None;
    let mut last_tag: Option<&str> = None;

    let re_multi_line_comments = RegexBuilder::new(r#"(/\*.*?\*/)"#)
        .multi_line(true)
        .dot_matches_new_line(true)
        .build()
        .unwrap();
    let re_tag = Regex::new(r#"^\s*--\s*name\s*:\s*(.*)\s*$"#).unwrap();

    re_multi_line_comments
        .replace_all(data, "")
        .lines()
        .filter(|x| !x.is_empty())
        .for_each(|line| {
            let (ty, value) = match re_tag.captures(line) {
                Some(caps) => (LineType::Tag, caps.get(1).unwrap().as_str()),
                None => (LineType::Query, line.trim()),
            };

            match ty {
                LineType::Tag => {
                    if last_type.is_some() && last_type.as_ref().unwrap() == &LineType::Tag {
                        panic!(r#"Tag "{}" overwritten"#, value)
                    }

                    last_tag = Some(value);
                }
                LineType::Query => {
                    if last_tag.is_none() {
                        panic!(r#"Query without tag: "{}""#, value);
                    }

                    queries
                        .entry(last_tag.unwrap().to_owned())
                        .and_modify(|x| {
                            *x = format!("{} {}", *x, value);
                        })
                        .or_insert_with(|| value.to_owned());
                }
            };

            last_type = Some(ty);
        });

    queries
}
