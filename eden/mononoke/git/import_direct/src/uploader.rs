/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::{save_bonsai_changesets, BlobRepo};
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use filestore::{self, StoreRequest};
use futures::stream::{self, Stream};
use futures_stats::TimedTryFutureExt;
use git_hash::ObjectId;
use import_tools::{
    CommitMetadata, GitImportLfs, GitUploader, HGGIT_COMMIT_ID_EXTRA, HGGIT_MARKER_EXTRA,
    HGGIT_MARKER_VALUE,
};
use mononoke_types::{
    hash, BonsaiChangeset, BonsaiChangesetMut, ChangesetId, FileChange, FileType, MPath,
};
use slog::{debug, info};
use sorted_vector_map::SortedVectorMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct DirectUploader {
    inner: Arc<BlobRepo>,
}

impl DirectUploader {
    pub fn new(repo: BlobRepo) -> Self {
        Self {
            inner: Arc::new(repo),
        }
    }
}

#[async_trait]
impl GitUploader for DirectUploader {
    type Change = FileChange;
    type IntermediateChangeset = BonsaiChangeset;

    fn deleted() -> Self::Change {
        FileChange::Deletion
    }

    async fn upload_file(
        &self,
        ctx: &CoreContext,
        lfs: &GitImportLfs,
        path: &MPath,
        ty: FileType,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<Self::Change, Error> {
        let meta_ret = if let Some(lfs_meta) = lfs.is_lfs_file(&git_bytes, oid) {
            let blobstore = self.inner.blobstore();
            let filestore_config = self.inner.filestore_config();
            cloned!(ctx, lfs, blobstore, path);
            Ok(lfs
                .with(
                    ctx,
                    lfs_meta,
                    move |ctx, lfs_meta, req, bstream| async move {
                        info!(
                            ctx.logger(),
                            "Uploading LFS {} sha256:{} size:{}",
                            path,
                            lfs_meta.sha256.to_brief(),
                            lfs_meta.size,
                        );
                        filestore::store(&blobstore, filestore_config, &ctx, &req, bstream).await
                    },
                )
                .await?)
        } else {
            let (req, bstream) = git_store_request(ctx, oid, git_bytes)?;
            Ok(filestore::store(
                self.inner.blobstore(),
                self.inner.filestore_config(),
                ctx,
                &req,
                bstream,
            )
            .await?)
        };
        meta_ret.map(|meta| FileChange::tracked(meta.content_id, ty, meta.total_size, None))
    }

    async fn generate_changeset(
        &self,
        _ctx: &CoreContext,
        bonsai_parents: Vec<ChangesetId>,
        metadata: CommitMetadata,
        changes: SortedVectorMap<MPath, Self::Change>,
        _dry_run: bool,
    ) -> Result<(Self::IntermediateChangeset, ChangesetId), Error> {
        let bcs = generate_bonsai_changeset(metadata, bonsai_parents, changes)?;
        let bcs_id = bcs.get_changeset_id();
        Ok((bcs, bcs_id))
    }

    async fn save_changesets_bulk(
        &self,
        ctx: &CoreContext,
        dry_run: bool,
        changesets: Vec<(Self::IntermediateChangeset, hash::GitSha1)>,
    ) -> Result<(), Error> {
        let oid_to_bcsid = changesets
            .iter()
            .map(|(bcs, git_sha1)| BonsaiGitMappingEntry::new(*git_sha1, bcs.get_changeset_id()))
            .collect::<Vec<BonsaiGitMappingEntry>>();
        let vbcs = changesets.into_iter().map(|x| x.0).collect();

        // We know that the commits are in order (this is guaranteed by the Walk), so we
        // can insert them as-is, one by one, without extra dependency / ordering checks.
        let (stats, ()) = save_bonsai_changesets(vbcs, ctx.clone(), &self.inner)
            .try_timed()
            .await?;
        debug!(
            ctx.logger(),
            "save_bonsai_changesets for {} commits in {:?}",
            oid_to_bcsid.len(),
            stats.completion_time
        );

        if !dry_run {
            self.inner
                .bonsai_git_mapping()
                .bulk_add(ctx, &oid_to_bcsid)
                .await?;
        }

        Ok(())
    }
}

fn git_store_request(
    ctx: &CoreContext,
    git_id: ObjectId,
    git_bytes: Bytes,
) -> Result<(StoreRequest, impl Stream<Item = Result<Bytes, Error>>), Error> {
    let size = git_bytes.len().try_into()?;
    let git_sha1 =
        hash::RichGitSha1::from_bytes(Bytes::copy_from_slice(git_id.as_bytes()), "blob", size)?;
    let req = StoreRequest::with_git_sha1(size, git_sha1);
    debug!(
        ctx.logger(),
        "Uploading git-blob:{} size:{}",
        git_sha1.sha1().to_brief(),
        size
    );
    Ok((req, stream::once(async move { Ok(git_bytes) })))
}

fn generate_bonsai_changeset(
    metadata: CommitMetadata,
    parents: Vec<ChangesetId>,
    file_changes: SortedVectorMap<MPath, FileChange>,
) -> Result<BonsaiChangeset, Error> {
    let CommitMetadata {
        oid,
        message,
        author,
        author_date,
        committer,
        committer_date,
        ..
    } = metadata;

    let mut extra = SortedVectorMap::new();
    extra.insert(
        HGGIT_COMMIT_ID_EXTRA.to_string(),
        oid.to_string().into_bytes(),
    );
    extra.insert(HGGIT_MARKER_EXTRA.to_string(), HGGIT_MARKER_VALUE.to_vec());

    // TODO: Should we have further extras?
    BonsaiChangesetMut {
        parents,
        author,
        author_date,
        committer: Some(committer),
        committer_date: Some(committer_date),
        message,
        extra,
        file_changes,
        is_snapshot: false,
    }
    .freeze()
}