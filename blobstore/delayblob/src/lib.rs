/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::fmt;
use std::iter::{repeat, Map, Repeat};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::future::lazy;
use failure_ext::Error;
use futures_ext::{BoxFuture, FutureExt};
use tokio::prelude::*;
use tokio::timer::Delay;

use blobstore::Blobstore;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

/// A blobstore that imposes a delay on all its operations, where the delay is generated by a
/// passed in function or closure
pub struct DelayBlob<F>
where
    F: FnMut(()) -> Duration + 'static + Send + Sync,
{
    blobstore: Box<dyn Blobstore>,
    delay: Mutex<Map<Repeat<()>, F>>,
    get_roundtrips: usize,
    put_roundtrips: usize,
    is_present_roundtrips: usize,
    assert_present_roundtrips: usize,
}

impl<F> DelayBlob<F>
where
    F: FnMut(()) -> Duration + 'static + Send + Sync,
{
    pub fn new(
        blobstore: Box<dyn Blobstore>,
        delay_gen: F,
        get_roundtrips: usize,
        put_roundtrips: usize,
        is_present_roundtrips: usize,
        assert_present_roundtrips: usize,
    ) -> Self {
        Self {
            blobstore,
            delay: Mutex::new(repeat(()).map(delay_gen)),
            get_roundtrips,
            put_roundtrips,
            is_present_roundtrips,
            assert_present_roundtrips,
        }
    }

    fn sleep(&self, roundtrips: usize) -> impl Future<Item = (), Error = Error> + 'static {
        let mut locked_delay = self.delay.lock().expect("lock poisoned");
        let delay = locked_delay.by_ref().take(roundtrips).sum();
        lazy(move || Delay::new(Instant::now() + delay)).map_err(Error::from)
    }
}

impl<F> Blobstore for DelayBlob<F>
where
    F: FnMut(()) -> Duration + 'static + Send + Sync,
{
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let sleep = self.sleep(self.get_roundtrips);
        let get = self.blobstore.get(ctx, key);
        sleep.and_then(move |_| get).boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let sleep = self.sleep(self.put_roundtrips);
        let put = self.blobstore.put(ctx, key, value);
        sleep.and_then(move |_| put).boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        let sleep = self.sleep(self.is_present_roundtrips);
        let is_present = self.blobstore.is_present(ctx, key);
        sleep.and_then(move |_| is_present).boxify()
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        let sleep = self.sleep(self.assert_present_roundtrips);
        let assert_present = self.blobstore.assert_present(ctx, key);
        sleep.and_then(move |_| assert_present).boxify()
    }
}

impl<F> fmt::Debug for DelayBlob<F>
where
    F: FnMut(()) -> Duration + 'static + Send + Sync,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DelayBlob")
            .field("blobstore", &self.blobstore)
            .field("get_roundtrips", &self.get_roundtrips)
            .field("put_roundtrips", &self.put_roundtrips)
            .field("is_present_roundtrips", &self.is_present_roundtrips)
            .field("assert_present_roundtrips", &self.assert_present_roundtrips)
            .finish()
    }
}
