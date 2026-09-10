//! Presentation state only: changing pages never owns or cancels a Core session.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(super) enum Page {
    #[default]
    Start,
    Dictation,
    Qa,
    Selection,
    Agent,
    Services,
    Models,
    Remote,
    History,
    Settings,
}

impl Page {
    pub const ALL: [Self; 10] = [
        Self::Start,
        Self::Dictation,
        Self::Qa,
        Self::Selection,
        Self::Agent,
        Self::Services,
        Self::Models,
        Self::Remote,
        Self::History,
        Self::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "开始",
            Self::Dictation => "听写",
            Self::Qa => "问答",
            Self::Selection => "选区润色",
            Self::Agent => "Less Computer",
            Self::Services => "AI 服务",
            Self::Models => "本地模型",
            Self::Remote => "手机输入",
            Self::History => "历史",
            Self::Settings => "环境与设置",
        }
    }
}

#[derive(Default)]
pub(super) struct Navigation {
    pub page: Page,
    unread: [bool; Page::ALL.len()],
}

impl Navigation {
    pub fn open(&mut self, page: Page) {
        self.page = page;
        self.unread[page as usize] = false;
    }

    pub fn notify(&mut self, page: Page) {
        if self.page != page {
            self.unread[page as usize] = true;
        }
    }

    pub fn has_update(&self, page: Page) -> bool {
        self.unread[page as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_work_keeps_its_notice_until_its_own_page_is_opened() {
        let mut navigation = Navigation::default();
        navigation.notify(Page::Qa);
        navigation.notify(Page::Selection);
        navigation.notify(Page::Agent);
        navigation.open(Page::Settings);
        for page in [Page::Qa, Page::Selection, Page::Agent] {
            assert!(navigation.has_update(page));
        }
        navigation.open(Page::Qa);
        assert!(!navigation.has_update(Page::Qa));
        assert!(navigation.has_update(Page::Selection));
        assert!(navigation.has_update(Page::Agent));
    }

    #[test]
    fn reading_a_page_does_not_create_an_unread_notice() {
        let mut navigation = Navigation::default();
        navigation.open(Page::Agent);
        navigation.notify(Page::Agent);
        assert!(!navigation.has_update(Page::Agent));
        navigation.open(Page::Start);
        navigation.notify(Page::Agent);
        assert!(navigation.has_update(Page::Agent));
        assert_eq!(navigation.page, Page::Start);
    }

    #[test]
    fn every_destination_can_be_opened_without_clearing_other_destinations() {
        let mut navigation = Navigation::default();
        for page in Page::ALL {
            assert!(!page.label().is_empty());
            navigation.notify(page);
        }
        for (index, page) in Page::ALL.into_iter().enumerate() {
            navigation.open(page);
            assert_eq!(navigation.page, page);
            assert!(!navigation.has_update(page));
            for remaining in &Page::ALL[index + 1..] {
                assert!(navigation.has_update(*remaining));
            }
        }
    }
}
