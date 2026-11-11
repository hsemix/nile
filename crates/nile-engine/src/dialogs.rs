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

/// How a native question box should look beyond the plain Yes/No one (`<question cancel task>`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageStyle {
    /// A third button, Cancel (the answer is then `Cancel`, not `No`).
    pub cancel: bool,
    /// A task dialog (Windows): the title bar's close button and Escape work and answer like Cancel (like No without `cancel`).
    pub task: bool,
}

impl MessageStyle {
    /// From the space separated words a script passes (`"cancel task"`).
    pub fn parse(words: &str) -> Self {
        let has = |w: &str| words.split_whitespace().any(|x| x == w);
        MessageStyle { cancel: has("cancel"), task: has("task") }
    }
}

/// What the user chose in a message box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageAnswer {
    Yes,
    No,
    Cancel,
}

impl MessageAnswer {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageAnswer::Yes => "yes",
            MessageAnswer::No => "no",
            MessageAnswer::Cancel => "cancel",
        }
    }
}

pub trait Dialogs {
    /// Paths chosen in an open dialog (empty when cancelled). `dir` is where to start.
    fn open_files(&self, multiple: bool, filters: &[FileFilter], caption: &str, dir: &str) -> Vec<String>;
    /// The path chosen in a save dialog, `extension` being the default one.
    fn save_file(&self, filters: &[FileFilter], extension: &str, caption: &str, path: &str) -> Option<String>;
    fn pick_folder(&self, caption: &str, dir: &str) -> Option<String>;
    /// A modal message box; `true` for OK / Yes. Only `Question` has a second button.
    fn message(&self, kind: MessageKind, caption: &str, text: &str) -> bool;
    /// A message box with a [`MessageStyle`]. The default ignores the style and uses [`Dialogs::message`], so embedders
    /// that only implement `message` keep working (a Cancel button needs a host that overrides this).
    fn message_with(&self, kind: MessageKind, caption: &str, text: &str, _style: MessageStyle) -> MessageAnswer {
        if self.message(kind, caption, text) { MessageAnswer::Yes } else { MessageAnswer::No }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_styles_come_from_words() {
        assert_eq!(MessageStyle::parse(""), MessageStyle::default());
        assert_eq!(MessageStyle::parse("task cancel"), MessageStyle { cancel: true, task: true });
        assert_eq!(MessageStyle::parse("cancelled"), MessageStyle::default());
    }

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
