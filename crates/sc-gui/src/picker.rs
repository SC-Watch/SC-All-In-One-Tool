//! File and folder dialogs, opened on a background thread so the window keeps
//! drawing and answering the desktop while one is open. Waiting for the
//! dialog on the window's own thread made KDE (and Windows) report the app as
//! not responding.

use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// What the picked path is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    ImportJoyToKey,
    AddProgram,
    /// Set the program of the "Launch with" item at this index.
    BrowseProgram(usize),
    AddInstall,
}

pub enum Kind {
    /// A file, optionally limited to (filter name, extensions).
    File(Option<(&'static str, &'static [&'static str])>),
    Folder,
}

pub struct Pending {
    pub purpose: Purpose,
    rx: Receiver<Option<PathBuf>>,
}

pub fn open(ctx: &egui::Context, purpose: Purpose, title: &str, kind: Kind) -> Pending {
    let (tx, rx) = mpsc::channel();
    let title = title.to_string();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new().set_title(&title);
        let picked = match kind {
            Kind::File(filter) => {
                if let Some((name, extensions)) = filter {
                    dialog = dialog.add_filter(name, extensions);
                }
                dialog.pick_file()
            }
            Kind::Folder => dialog.pick_folder(),
        };
        let _ = tx.send(picked);
        ctx.request_repaint();
    });
    Pending { purpose, rx }
}

impl Pending {
    /// `Some(result)` once the dialog has closed; the result is `None` when
    /// nothing was chosen.
    pub fn poll(&self) -> Option<Option<PathBuf>> {
        match self.rx.try_recv() {
            Ok(picked) => Some(picked),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}
