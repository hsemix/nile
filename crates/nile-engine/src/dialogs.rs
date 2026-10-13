//! Native dialogs scripts can ask for (`Window.this.selectFile`, `selectFolder`, and the message
//! boxes of `Window.this.modal(<alert>...</alert>)`). The engine has no windowing, so the embedder
//! implements [`Dialogs`] and hands it to `Page::set_dialogs`; without one, scripts get `null`.

/// One entry of a file dialog's type list: `HTML File (*.htm,*.html)|*.html;*.htm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFilter {
    pub label: String,
    /// Extensions without the dot; empty means every file.
    pub extensions: Vec<String>,
}

/// Sciter's filter text: `label|ext1;ext2|label|ext1` where an extension is `*.html` (or `*.*`).
pub fn parse_filter(filter: &str) -> Vec<FileFilter> {
    let parts: Vec<&str> = filter.split('|').collect();
    parts
        .chunks(2)
        .filter(|c| c.len() == 2 && !c[0].trim().is_empty())
        .map(|c| FileFilter {
            label: c[0].trim().to_string(),
            extensions: c[1]
                .split(';')
                .map(|e| e.trim().trim_start_matches("*.").trim_start_matches('.').to_string())
                .filter(|e| !e.is_empty() && e != "*")
                .collect(),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Alert,
    Error,
    Question,
}

pub trait Dialogs {
    /// Paths chosen in an open dialog (empty when cancelled). `dir` is where to start.
    fn open_files(&self, multiple: bool, filters: &[FileFilter], caption: &str, dir: &str) -> Vec<String>;
    /// The path chosen in a save dialog, `extension` being the default one.
    fn save_file(&self, filters: &[FileFilter], extension: &str, caption: &str, path: &str) -> Option<String>;
    fn pick_folder(&self, caption: &str, dir: &str) -> Option<String>;
    /// A modal message box; `true` for OK / Yes. Only `Question` has a second button.
    fn message(&self, kind: MessageKind, caption: &str, text: &str) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_follow_sciters_syntax() {
        let f = parse_filter("HTML File (*.htm,*.html)|*.html;*.htm|All Files (*.*)|*.*");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0], FileFilter { label: "HTML File (*.htm,*.html)".into(), extensions: vec!["html".into(), "htm".into()] });
        assert!(f[1].extensions.is_empty());
        assert!(parse_filter("").is_empty());
        assert!(parse_filter("dangling").is_empty());
    }
}
