use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub mod api;
mod build_info;
pub mod const_val;
pub mod dyn_setting;
pub mod echo;
pub mod invite_code;
pub mod mfa;
pub mod permission;
pub mod resource;
pub mod session;
pub mod token;
pub mod users;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    Added,
    Removed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiffOwned<T> {
    pub added: Vec<T>,
    pub removed: Vec<T>,
}

impl<T> DiffOwned<T> {
    pub fn new(added: Vec<T>, removed: Vec<T>) -> Self {
        Self { added, removed }
    }

    pub fn as_ref(&self) -> DiffRef<'_, T> {
        self.into()
    }
}

#[derive(Debug)]
pub struct DiffRef<'a, T> {
    pub added: &'a [T],
    pub removed: &'a [T],
}

impl<'a, T> DiffRef<'a, T> {
    pub fn new(added: &'a [T], removed: &'a [T]) -> Self {
        Self { added, removed }
    }

    pub fn iter(&self) -> impl Iterator<Item = DiffItemRef<'a, T>> {
        self.added
            .iter()
            .map(|x| DiffItemRef {
                kind: Change::Added,
                value: x,
            })
            .chain(self.removed.iter().map(|x| DiffItemRef {
                kind: Change::Removed,
                value: x,
            }))
    }
}

#[derive(Debug)]
pub struct DiffItemRef<'a, T> {
    pub kind: Change,
    pub value: &'a T,
}

impl<'a, T> From<&'a DiffOwned<T>> for DiffRef<'a, T> {
    fn from(d: &'a DiffOwned<T>) -> Self {
        DiffRef {
            added: &d.added,
            removed: &d.removed,
        }
    }
}
