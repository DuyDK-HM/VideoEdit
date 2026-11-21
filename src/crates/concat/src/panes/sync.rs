// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: File > Push project to server, and File > Pull latest project from
//! server. See `concat_host::sync`. A pull lands in `<home>/Concat Sync/<id>`
//! and opens there; pulling again updates it, fetching only what changed.

use std::path::PathBuf;

use concat_host::sync::{self, Server};

use crate::host::{spawn_long, spawn_long_in_project};
use crate::i18n::{t, tf};
use crate::studio::Studio;

/// Saves the open project and pushes it as a new version.
pub fn push_open_project(studio: &mut Studio) {
    let server = match Server::from_env() {
        Ok(server) => server,
        Err(error) => return studio.notify(&error, true),
    };
    let Some(project) = studio
        .session
        .as_ref()
        .map(|session| PathBuf::from(session.path()))
    else {
        return;
    };
    studio.save(false);
    studio.notify(&t("sync.pushing"), false);
    spawn_long_in_project(
        move || sync::push(&server, &project, &mut |_| {}),
        |studio, _, _, result| match result {
            Ok((id, version, uploaded)) => {
                studio.notify(&tf("sync.pushed", &[&id, &version, &uploaded]), false)
            }
            Err(error) => studio.notify(&error, true),
        },
    );
}

/// Pulls the project pushed last and opens it.
pub fn pull_latest(studio: &mut Studio) {
    let server = match Server::from_env() {
        Ok(server) => server,
        Err(error) => return studio.notify(&error, true),
    };
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    studio.notify(&t("sync.pulling"), false);
    spawn_long(
        move || {
            let id = sync::latest(&server)?.ok_or_else(|| t("sync.nothingOnServer"))?;
            sync::pull(&server, &id, &home.join("Concat Sync").join(&id), &mut |_| {})
        },
        |studio, _, _, result| match result {
            Ok(folder) => {
                if let Err(error) = studio.open_project_at(&folder.to_string_lossy()) {
                    studio.notify(&error, true);
                }
            }
            Err(error) => studio.notify(&error, true),
        },
    );
}
