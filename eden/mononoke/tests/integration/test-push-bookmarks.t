# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export ONLY_FAST_FORWARD_BOOKMARK="master_bookmark"
  $ export ONLY_FAST_FORWARD_BOOKMARK_REGEX="ffonly.*"
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull

start mononoke

  $ start_and_wait_for_mononoke_server
Push with bookmark
  $ cd repo-push
  $ enableextension remotenames
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hgmn push --to withbook --create
  pushing rev 11f53bbd855a to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark withbook
  searching for changes
  exporting bookmark withbook

Pull the bookmark
  $ cd ../repo-pull
  $ enableextension remotenames

  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   0e7ec5675652
     default/withbook          11f53bbd855a

Update the bookmark
  $ cd ../repo-push
  $ echo update > update && hg addremove && hg ci -m update
  adding update
  $ hgmn push --to withbook
  pushing rev 66b9c137712a to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark withbook
  searching for changes
  updating bookmark withbook
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   0e7ec5675652
     default/withbook          66b9c137712a

Try non fastforward moves (backwards and across branches)
  $ cd ../repo-push
  $ hg update -q master_bookmark
  $ echo other_commit > other_commit && hg -q addremove && hg ci -m other_commit
  $ hgmn push
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  updating bookmark master_bookmark
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to master_bookmark
  pushing rev 0e7ec5675652 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Non fast-forward bookmark move from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to 30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473
  remote: 
  remote:   Caused by:
  remote:     Failed to move bookmark
  remote:   Caused by:
  remote:     Non fast-forward bookmark move from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to 30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to move bookmark",
  remote:             source: NonFastForwardMove {
  remote:                 from: ChangesetId(
  remote:                     Blake2(29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5),
  remote:                 ),
  remote:                 to: ChangesetId(
  remote:                     Blake2(30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473),
  remote:                 ),
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 66b9c137712a --to master_bookmark
  pushing rev 66b9c137712a to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Non fast-forward bookmark move from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to b1a2d38c877a990517a50f9bf928770dd7d3b5b9dbef412d7dafd2ccd2ede0fb
  remote: 
  remote:   Caused by:
  remote:     Failed to move bookmark
  remote:   Caused by:
  remote:     Non fast-forward bookmark move from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to b1a2d38c877a990517a50f9bf928770dd7d3b5b9dbef412d7dafd2ccd2ede0fb
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to move bookmark",
  remote:             source: NonFastForwardMove {
  remote:                 from: ChangesetId(
  remote:                     Blake2(29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5),
  remote:                 ),
  remote:                 to: ChangesetId(
  remote:                     Blake2(b1a2d38c877a990517a50f9bf928770dd7d3b5b9dbef412d7dafd2ccd2ede0fb),
  remote:                 ),
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to withbook
  pushing rev 0e7ec5675652 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark withbook
  searching for changes
  no changes found
  updating bookmark withbook
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   a075b5221b92
     default/withbook          0e7ec5675652

Try non fastfoward moves on regex bookmark
  $ hgmn push -r a075b5221b92 --to ffonly_bookmark --create -q
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to ffonly_bookmark
  pushing rev 0e7ec5675652 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark ffonly_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Non fast-forward bookmark move from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to 30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473
  remote: 
  remote:   Caused by:
  remote:     Failed to move bookmark
  remote:   Caused by:
  remote:     Non fast-forward bookmark move from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to 30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to move bookmark",
  remote:             source: NonFastForwardMove {
  remote:                 from: ChangesetId(
  remote:                     Blake2(29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5),
  remote:                 ),
  remote:                 to: ChangesetId(
  remote:                     Blake2(30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473),
  remote:                 ),
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Try to delete master
  $ cd ../repo-push
  $ hgmn push --delete master_bookmark
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Deletion of 'master_bookmark' is prohibited
  remote: 
  remote:   Caused by:
  remote:     Failed to delete bookmark
  remote:   Caused by:
  remote:     Deletion of 'master_bookmark' is prohibited
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to delete bookmark",
  remote:             source: DeletionProhibited {
  remote:                 bookmark: BookmarkName {
  remote:                     bookmark: "master_bookmark",
  remote:                 },
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Delete the bookmark
  $ hgmn push --delete withbook
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  no changes found
  deleting remote bookmark withbook
  [1]
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/ffonly_bookmark   a075b5221b92
     default/master_bookmark   a075b5221b92
