//! The Store's pages. Two of them are in the sidebar, the apps Rift suggests and the apps that are
//! installed; the other two are where the field and a press take you, what a search found and one
//! app on its own.

/// One page of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    /// The apps Rift suggests, in their groups. The front page.
    Apps,
    /// What the words in the field found on the remotes.
    Found,
    /// One app: what it is, how big it is, and what it asks for.
    App,
    /// The apps that are installed.
    Installed,
}

impl Page {
    /// Every page, in the order the words are listed.
    pub const ALL: [Self; 4] = [Self::Apps, Self::Found, Self::App, Self::Installed];

    /// The pages the sidebar lists, in order.
    pub const SIDE: [Self; 2] = [Self::Apps, Self::Installed];

    /// The page's name in the sidebar and over the page. The page of one app is headed by the
    /// app's own name instead.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Apps => "Apps",
            Self::Found => "Search",
            Self::App => "App",
            Self::Installed => "Installed",
        }
    }

    /// The word the control socket takes and `--state` prints.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Apps => "apps",
            Self::Found => "found",
            Self::App => "app",
            Self::Installed => "installed",
        }
    }

    /// The page a word names.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        let word = word.trim();
        Self::ALL
            .into_iter()
            .find(|page| page.word().eq_ignore_ascii_case(word))
    }

    /// The symbolic icon of the page's row in the sidebar, from the Adwaita theme.
    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Apps => "system-software-install-symbolic",
            Self::Found => "system-search-symbolic",
            Self::App => "application-x-executable-symbolic",
            Self::Installed => "object-select-symbolic",
        }
    }

    /// Whether the page is one of the two the sidebar lists.
    #[must_use]
    pub fn in_sidebar(self) -> bool {
        Self::SIDE.contains(&self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_reads_back_from_its_word() {
        for page in Page::ALL {
            assert_eq!(Page::from_word(page.word()), Some(page));
            assert_eq!(
                Page::from_word(&page.word().to_ascii_uppercase()),
                Some(page)
            );
            assert!(!page.label().is_empty() && !page.icon().is_empty());
        }
        assert_eq!(Page::from_word("welcome"), None);
        assert_eq!(Page::from_word(""), None);
    }

    #[test]
    fn the_sidebar_lists_the_apps_and_what_is_installed() {
        assert!(Page::Apps.in_sidebar() && Page::Installed.in_sidebar());
        assert!(!Page::Found.in_sidebar() && !Page::App.in_sidebar());
    }
}
