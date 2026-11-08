//! `I attach the file …`: reading the file, the multipart body it makes, and
//! the check the host runs before the first request.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use reqwest::multipart::{Form, Part};

use crate::state::Recipe;

/// One attached file, read into memory when the step ran so a replayed
/// request resends the content it first sent.
#[derive(Clone)]
pub struct FilePart {
    pub name: String,
    pub file_name: String,
    pub mime: String,
    pub bytes: Arc<[u8]>,
}

/// A step's path against the directory of the file the step is written in;
/// an absolute path is taken as is.
fn resolve(file: &str, source_dir: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(source_dir).join(path)
    }
}

/// Slashes forward on every platform, as the host writes a path.
fn shown(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Reads the file now, so a missing one fails at the step and a replay sends
/// what the step saw. The part's file name is the path's last segment and its
/// type comes from the extension (`application/octet-stream` when unknown).
pub fn read(file: &str, part: &str, source_dir: &str) -> Result<FilePart, String> {
    let path = resolve(file, source_dir);
    let bytes = std::fs::read(&path).map_err(|e| {
        format!(
            "I attach the file {file:?}: cannot read {}: {e}",
            shown(&path)
        )
    })?;
    Ok(FilePart {
        name: part.to_string(),
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        mime: mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string(),
        bytes: bytes.into(),
    })
}

/// A literal path must be a file before the first request. A path holding a
/// `<<variable>>` can only be read once it is interpolated, so the step
/// reports it then.
pub fn validate(file: &str, source_dir: &str) -> Result<(), String> {
    if file.contains("<<") {
        return Ok(());
    }
    let path = resolve(file, source_dir);
    if path.is_file() {
        Ok(())
    } else {
        Err(format!(
            "I attach the file {file:?}: no such file ({})",
            shown(&path)
        ))
    }
}

/// A fresh `Form` per attempt: a form is consumed by the request that carries
/// it, so a replay rebuilds it from the recipe.
pub fn form(recipe: &Recipe) -> Result<Form, String> {
    let mut form = Form::new();
    for (name, value) in recipe.form.iter().flatten() {
        form = form.text(name.clone(), value.clone());
    }
    for file in &recipe.files {
        let part = Part::bytes(file.bytes.to_vec())
            .file_name(file.file_name.clone())
            .mime_str(&file.mime)
            .map_err(|e| format!("invalid content type {:?}: {e}", file.mime))?;
        form = form.part(file.name.clone(), part);
    }
    Ok(form)
}

/// What the failure dump shows of a multipart body: files first, as name, file
/// name, type and size, never the bytes; then the text parts as `name=value`.
/// The dump keeps the first 600 characters of a body, and the file lines are
/// the ones that matter.
pub fn describe(recipe: &Recipe) -> String {
    recipe
        .files
        .iter()
        .map(|f| {
            format!(
                "{}: {} ({}, {} bytes)",
                f.name,
                f.file_name,
                f.mime,
                f.bytes.len()
            )
        })
        .chain(
            recipe
                .form
                .iter()
                .flatten()
                .map(|(name, value)| format!("{name}={}", cut(value, 100))),
        )
        .collect::<Vec<_>>()
        .join("\n    ")
}

fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;
