use std::collections::HashMap;
use std::ops::Index;

use rsyesql::{indexmap, parse as parse_sql};

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

            let mut group = parse_sql(text).unwrap();
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

    #[inline]
    pub fn get(&self, group: &str, name: &str) -> &str {
        &self[group][name]
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
    queries: indexmap::IndexMap<String, String>,
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

impl QueriesMap {
    pub fn iter(&self) -> QueriesMapIter {
        QueriesMapIter {
            iter: self.queries.iter(),
        }
    }
}

pub struct QueriesMapIter<'a> {
    iter: indexmap::map::Iter<'a, String, String>,
}

impl<'a> Iterator for QueriesMapIter<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<(&'a str, &'a str)> {
        self.iter.next().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}
