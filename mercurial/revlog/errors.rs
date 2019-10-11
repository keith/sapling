/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure_ext::{
    bail_msg, ensure_msg, format_err, prelude::*, Error, Fail, Result, ResultExt,
};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Bundle2Decode: {}", _0)]
    Bundle2Decode(String),
    #[fail(display = "Revlog: {}", _0)]
    Revlog(String),
    #[fail(display = "Repo: {}", _0)]
    Repo(String),
    #[fail(display = "Path: {}", _0)]
    Path(String),
    #[fail(display = "Unknown requirement: {}", _0)]
    UnknownReq(String),
    #[fail(display = "invalid Thrift structure '{}': {}", _0, _1)]
    InvalidThrift(String, String),
}
