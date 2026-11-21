//! Clipboard backup/restore ordering for Windows selection capture, independent of native I/O.

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ClipboardPollOutcome {
    /// The clipboard changed and its text was read successfully.
    Captured(String),
    /// The clipboard sequence never moved: the target app did not copy anything.
    Unchanged,
    /// The clipboard changed, but its text could not be read before the deadline.
    Unreadable,
}

pub(super) trait ClipboardCapture {
    type Error;

    /// `None` means the text format is absent; access failures must return `Err`.
    fn backup_text(&mut self) -> Result<Option<String>, Self::Error>;
    fn copy_selection(&mut self) -> (bool, ClipboardPollOutcome);
    fn restore_text(&mut self, text: &str);
}

pub(super) fn capture_with_restore<C: ClipboardCapture>(
    clipboard: &mut C,
) -> Result<(bool, ClipboardPollOutcome), C::Error> {
    // An unreadable backup must never be treated as an empty clipboard: copying now could
    // destroy text that we cannot restore after the target app's Ctrl+C eventually succeeds.
    let original = clipboard.backup_text()?;
    let (post_ok, outcome) = clipboard.copy_selection();

    // Equal text does not imply an unchanged clipboard: a browser or Word may also have
    // added HTML/RTF. Restore after every observed change, including unreadable copies.
    // If the sequence never moved, avoid the unnecessary Clipboard History write.
    if !matches!(&outcome, ClipboardPollOutcome::Unchanged) {
        clipboard.restore_text(original.as_deref().unwrap_or(""));
    }

    Ok((post_ok, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ReadError {
        Occupied,
        Unknown,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ClipboardContents {
        text: String,
        rich_text: bool,
    }

    impl ClipboardContents {
        fn plain(text: &str) -> Self {
            Self {
                text: text.into(),
                rich_text: false,
            }
        }
    }

    struct FakeClipboard {
        backup_error: Option<ReadError>,
        has_text: bool,
        contents: ClipboardContents,
        copy_result: Option<(bool, ClipboardPollOutcome)>,
        copies: usize,
        restores: usize,
    }

    impl FakeClipboard {
        fn new(original: &str, outcome: ClipboardPollOutcome) -> Self {
            Self {
                backup_error: None,
                has_text: true,
                contents: ClipboardContents::plain(original),
                copy_result: Some((true, outcome)),
                copies: 0,
                restores: 0,
            }
        }
    }

    impl ClipboardCapture for FakeClipboard {
        type Error = ReadError;

        fn backup_text(&mut self) -> Result<Option<String>, ReadError> {
            if let Some(error) = self.backup_error {
                return Err(error);
            }
            Ok(self.has_text.then(|| self.contents.text.clone()))
        }

        fn copy_selection(&mut self) -> (bool, ClipboardPollOutcome) {
            self.copies += 1;
            let result = self.copy_result.take().expect("copy requested only once");
            match &result.1 {
                ClipboardPollOutcome::Captured(text) => {
                    self.contents = ClipboardContents {
                        text: text.clone(),
                        rich_text: true,
                    };
                }
                ClipboardPollOutcome::Unreadable => {
                    self.contents = ClipboardContents::plain("");
                    self.has_text = false;
                }
                ClipboardPollOutcome::Unchanged => {}
            }
            result
        }

        fn restore_text(&mut self, text: &str) {
            self.restores += 1;
            self.has_text = true;
            self.contents = ClipboardContents::plain(text);
        }
    }

    #[test]
    fn backup_failure_never_copies_or_clears_original_text() {
        for error in [ReadError::Occupied, ReadError::Unknown] {
            let mut clipboard = FakeClipboard::new(
                "original",
                ClipboardPollOutcome::Captured("selected".into()),
            );
            clipboard.backup_error = Some(error);

            assert_eq!(capture_with_restore(&mut clipboard), Err(error));
            assert_eq!(clipboard.contents, ClipboardContents::plain("original"));
            assert_eq!(clipboard.copies, 0);
            assert_eq!(clipboard.restores, 0);
        }
    }

    #[test]
    fn equal_text_from_rich_selection_restores_original_plain_clipboard() {
        let mut clipboard =
            FakeClipboard::new("same", ClipboardPollOutcome::Captured("same".into()));

        assert_eq!(
            capture_with_restore(&mut clipboard),
            Ok((true, ClipboardPollOutcome::Captured("same".into())))
        );
        assert_eq!(clipboard.contents, ClipboardContents::plain("same"));
        assert_eq!(clipboard.restores, 1);
    }

    #[test]
    fn captured_selection_is_returned_after_restoring_original_text() {
        let mut clipboard = FakeClipboard::new(
            "original",
            ClipboardPollOutcome::Captured("selected".into()),
        );

        assert_eq!(
            capture_with_restore(&mut clipboard),
            Ok((true, ClipboardPollOutcome::Captured("selected".into())))
        );
        assert_eq!(clipboard.contents, ClipboardContents::plain("original"));
        assert_eq!(clipboard.restores, 1);
    }

    #[test]
    fn unchanged_clipboard_is_not_written_again() {
        let mut clipboard = FakeClipboard::new("original", ClipboardPollOutcome::Unchanged);
        clipboard.contents.rich_text = true;

        assert_eq!(
            capture_with_restore(&mut clipboard),
            Ok((true, ClipboardPollOutcome::Unchanged))
        );
        assert_eq!(clipboard.contents.text, "original");
        assert!(clipboard.contents.rich_text);
        assert_eq!(clipboard.restores, 0);
    }

    #[test]
    fn changed_but_unreadable_clipboard_still_restores_backup() {
        let mut clipboard = FakeClipboard::new("original", ClipboardPollOutcome::Unreadable);

        assert_eq!(
            capture_with_restore(&mut clipboard),
            Ok((true, ClipboardPollOutcome::Unreadable))
        );
        assert_eq!(clipboard.contents, ClipboardContents::plain("original"));
        assert_eq!(clipboard.restores, 1);
    }

    #[test]
    fn absent_text_backup_clears_the_selection_copy() {
        let mut clipboard =
            FakeClipboard::new("", ClipboardPollOutcome::Captured("selected".into()));
        clipboard.has_text = false;

        assert!(capture_with_restore(&mut clipboard).is_ok());
        assert_eq!(clipboard.contents, ClipboardContents::plain(""));
        assert_eq!(clipboard.restores, 1);
    }

    #[test]
    fn failed_shortcut_still_restores_an_observed_clipboard_change() {
        let mut clipboard = FakeClipboard::new("original", ClipboardPollOutcome::Unreadable);
        clipboard.copy_result.as_mut().unwrap().0 = false;

        assert_eq!(
            capture_with_restore(&mut clipboard),
            Ok((false, ClipboardPollOutcome::Unreadable))
        );
        assert_eq!(clipboard.contents, ClipboardContents::plain("original"));
    }
}
