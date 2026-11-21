// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: File > Pack project (.zip) and File > Open packed project.
//! See `concat_host::package` for what goes in.

use std::path::PathBuf;

use concat_host::package;

use crate::host::{self, spawn_long_in_project, spawn_long};
use crate::i18n::{t, tf};
use crate::studio::Studio;

/// Saves the open project, then packs it beside its folder as `<name>.zip`.
pub fn pack_open_project(studio: &mut Studio) {
    let Some(project) = studio
        .session
        .as_ref()
        .map(|session| PathBuf::from(session.path()))
    else {
        return;
    };
    studio.save(false);
    let name = project
        .file_name()
        .map_or("project".to_owned(), |name| name.to_string_lossy().into_owned());
    let out = project.with_file_name(format!("{name}.zip"));
    studio.notify(&t("package.packing"), false);
    spawn_long_in_project(
        move || package::pack(&project, &out).map(|outside| (out, outside)),
        |studio, _, _, result| match result {
            Ok((out, outside)) => studio.notify(
                &tf("package.packed", &[&out.display(), &outside]),
                false,
            ),
            Err(error) => studio.notify(&error, true),
        },
    );
}

/// Asks for a package, unpacks it beside itself and opens it.
pub fn open_package() {
    host::pick_files_then(
        t("package.openMenu"),
        Some((t("package.zip"), &["zip"])),
        |_, _, _, paths| {
            let Some(zip) = paths.into_iter().next() else {
                return;
            };
            spawn_long(
                move || package::unpack(&zip, &package::unpack_folder_for(&zip)),
                |studio, _, _, result| match result {
                    Ok(folder) => {
                        if let Err(error) = studio.open_project_at(&folder.to_string_lossy()) {
                            studio.notify(&error, true);
                        }
                    }
                    Err(error) => studio.notify(&error, true),
                },
            );
        },
    );
}
