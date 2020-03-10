use std::collections::HashMap;
use std::ops::Index;

use super::yesql::parse_sql;

pub type StaticQueries = &'static [(&'static str, &'static str)];

#[derive(Debug)]
pub struct Queries {
    queries: HashMap<String, QueriesMap>,
}

impl Queries {
    pub fn new() -> Queries {
        Queries {
            queries: HashMap::new(),
        }
    }

    pub fn load(&mut self, queries: StaticQueries, schema: &str) {
        for (name, text) in queries.iter() {
            let name = (*name).to_owned();

            let mut group = parse_sql(text);
            for (_, query) in group.iter_mut() {
                *query = query.replace("{SCHEMA}", schema);
            }

            // panic! if entry already exists
            if self.queries.get(&name).is_some() {
                panic!(format!("Duplicate queries for group: {}", name));
            }

            self.queries.insert(name, QueriesMap { queries: group });
        }
    }
}

impl Index<&str> for Queries {
    type Output = QueriesMap;

    #[inline]
    fn index(&self, key: &str) -> &QueriesMap {
        match self.queries.get(key) {
            Some(v) => v,
            None => panic!(r#"no queries for key: "{}""#, key),
        }
    }
}

#[derive(Debug)]
pub struct QueriesMap {
    queries: HashMap<String, String>,
}

impl Index<&str> for QueriesMap {
    type Output = str;

    #[inline]
    fn index(&self, key: &str) -> &str {
        match self.queries.get(key) {
            Some(v) => v.as_str(),
            None => panic!(r#"no query for key: "{}""#, key),
        }
    }
}
