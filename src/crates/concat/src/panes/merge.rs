// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: File > Merge selected files. The media picked in the bin, in the
//! natural order of their names (`001_a`, `002_b`, ... `010_c`), become one
//! video: joined without a re-encode when they all match
//! (`concat_host::merge`), the result imported into the bin; otherwise laid
//! one after another at the end of the timeline, for the export to make
//! one file of, scaled and padded to the frame.

use std::path::PathBuf;

use concat_host::{media, merge};
use concat_project::Command;

use crate::host::spawn_long_in_project;
use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::panes::media_bin::MediaMsg;
use crate::studio::Studio;

/// What the worker found.
enum Merged {
    /// Joined into this file.
    File(PathBuf),
    /// The files differ; they go on the timeline in this order.
    Timeline(Vec<String>),
}

/// Merges the media selected in the bin.
pub fn merge_selected(studio: &mut Studio) {
    let Some(project) = studio
        .session
        .as_ref()
        .map(|session| PathBuf::from(session.path()))
    else {
        return;
    };
    let mut picked: Vec<(String, PathBuf)> = studio
        .project()
        .media
        .iter()
        .filter(|item| studio.media.selected.contains(&item.id))
        .map(|item| (item.id.clone(), PathBuf::from(&item.path)))
        .collect();
    if picked.len() < 2 {
        studio.notify(&t("merge.pickTwo"), true);
        return;
    }
    picked.sort_by(|(_, a), (_, b)| {
        let name = |path: &PathBuf| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        merge::natural_cmp(&name(a), &name(b))
    });
    studio.notify(&tf("merge.started", &[&picked.len()]), false);
    spawn_long_in_project(
        move || -> Result<Merged, String> {
            let summaries = picked
                .iter()
                .map(|(_, path)| media::probe(&path.to_string_lossy()))
                .collect::<Result<Vec<_>, _>>()?;
            if !merge::can_copy(&summaries) {
                return Ok(Merged::Timeline(picked.into_iter().map(|(id, _)| id).collect()));
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            let extension = picked[0]
                .1
                .extension()
                .map_or("mp4".to_owned(), |e| e.to_string_lossy().to_ascii_lowercase());
            let target = project.join("merged").join(format!("merged-{stamp}.{extension}"));
            let files: Vec<PathBuf> = picked.into_iter().map(|(_, path)| path).collect();
            merge::concat_copy(&files, &target)?;
            Ok(Merged::File(target))
        },
        |studio, _, _, result| match result {
            Ok(Merged::File(path)) => {
                studio.notify(&t("merge.joined"), false);
                studio.handle(Msg::Media(MediaMsg::Import(vec![path])));
            }
            Ok(Merged::Timeline(ids)) => {
                let count = ids.len();
                let start = f64::from(studio.duration());
                studio.apply(Command::AddClipsInSequence {
                    media_ids: ids,
                    track_id: None,
                    start,
                    ripple: false,
                });
                studio.notify(&tf("merge.onTimeline", &[&count]), false);
            }
            Err(error) => studio.notify(&error, true),
        },
    );
}
