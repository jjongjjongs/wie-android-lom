//! What this runtime does differently for one named title.
//!
//! Some titles were written against a handset this runtime does not reproduce
//! exactly, and the difference is not something the title can be talked out of
//! at runtime: it sizes its screens from a panel it assumes, or it draws
//! around a status strip it assumes is there. Each of those is a fact about
//! one title, established by running it, and there is nowhere in the emulated
//! API to put such a fact.
//!
//! They therefore live here, keyed by the platform and the application id its
//! descriptor carries, so that every one of them is in a single place rather
//! than spread through whichever module happened to need it first. The
//! reference emulator keeps the same kind of table (`internal/quirkdb`, keyed
//! by a title hash), which is what suggested collecting ours.
//!
//! A title with no entry gets [`TitleQuirks::default`], which asks for nothing.

/// The platform whose descriptor named an application id.
///
/// Ids are only unique within a platform - they are assigned per carrier - so
/// a lookup that did not say which platform it meant could answer for the
/// wrong title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlePlatform {
    Ktf,
    Lgt,
    Skt,
}

/// Everything known to need doing differently for one title.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TitleQuirks {
    /// The panel the title was drawn for, when that is not the one a host
    /// would pick by default. `None` leaves the host's own default standing.
    ///
    /// A title lays out from what the screen reports, so a panel it was not
    /// written for is one it lays out wrongly. The host has to size its screen
    /// before there is an emulator to ask, so this is read from the archive's
    /// descriptor rather than from a running title.
    pub screen_size: Option<(u32, u32)>,

    /// Whether the title draws its own status strip, so the runtime must leave
    /// room for one above the drawing area rather than handing the title the
    /// whole panel.
    pub expects_annunciator: bool,
}

const fn panel(width: u32, height: u32) -> TitleQuirks {
    TitleQuirks {
        screen_size: Some((width, height)),
        expects_annunciator: false,
    }
}

const fn annunciator() -> TitleQuirks {
    TitleQuirks {
        screen_size: None,
        expects_annunciator: true,
    }
}

/// Every title this runtime knows something about, and what it knows.
///
/// The reasoning behind each entry is at the function that reads it - the
/// screen size at `LgtEmulator::screen_size`, the status strip at
/// `wie_lgt`'s `title_expects_annunciator`.
const QUIRKS: &[(TitlePlatform, &str, TitleQuirks)] = &[
    // 미니게임 히어로즈2 터치: repaints a 240x80 sponsor banner along the
    // bottom of whatever height it is told, so its 320 rows of screen need a
    // 400-row panel underneath them.
    (TitlePlatform::Lgt, "00030F5B", panel(240, 400)),
    // 판타지나이트: without the strip its bottom 24 rows keep a stale band.
    (TitlePlatform::Lgt, "0002787C", annunciator()),
    // 프로야구 2009.
    (TitlePlatform::Lgt, "0002CB6A", annunciator()),
    // 지크2: centres a 296-row picture and then adds the strip itself.
    (TitlePlatform::Lgt, "0002A52B", annunciator()),
    // 알바타이쿤2: every screen it draws lands exactly one strip down.
    (TitlePlatform::Lgt, "0002D4D0", annunciator()),
];

/// What to do differently for the title `aid` on `platform`.
///
/// Returns [`TitleQuirks::default`] - which asks for nothing - for every title
/// without an entry, which is nearly all of them.
pub fn title_quirks(platform: TitlePlatform, aid: &str) -> TitleQuirks {
    for (entry_platform, entry_aid, quirks) in QUIRKS {
        if *entry_platform == platform && entry_aid.eq_ignore_ascii_case(aid) {
            return *quirks;
        }
    }

    TitleQuirks::default()
}

#[cfg(test)]
mod tests {
    use super::{TitlePlatform, TitleQuirks, title_quirks};

    #[test]
    fn a_title_without_an_entry_asks_for_nothing() {
        assert_eq!(title_quirks(TitlePlatform::Lgt, "00025C2B"), TitleQuirks::default());
        assert_eq!(title_quirks(TitlePlatform::Skt, "00030F5B"), TitleQuirks::default());
    }

    /// Ids are assigned per carrier, so an entry must not answer for the same
    /// id on another platform.
    #[test]
    fn an_entry_answers_only_for_its_own_platform() {
        assert_eq!(title_quirks(TitlePlatform::Lgt, "00030F5B").screen_size, Some((240, 400)));
        assert_eq!(title_quirks(TitlePlatform::Ktf, "00030F5B").screen_size, None);
        assert_eq!(title_quirks(TitlePlatform::Skt, "00030F5B").screen_size, None);
    }

    /// Descriptors are not consistent about the case of an id.
    #[test]
    fn an_id_is_matched_whatever_case_it_is_written_in() {
        assert!(title_quirks(TitlePlatform::Lgt, "0002cb6a").expects_annunciator);
        assert!(title_quirks(TitlePlatform::Lgt, "0002CB6A").expects_annunciator);
    }

    /// An id appearing twice for one platform would make the table's answer
    /// depend on which entry came first.
    #[test]
    fn no_title_is_listed_twice() {
        for (index, (platform, aid, _)) in super::QUIRKS.iter().enumerate() {
            for (other_platform, other_aid, _) in &super::QUIRKS[index + 1..] {
                assert!(
                    !(platform == other_platform && aid.eq_ignore_ascii_case(other_aid)),
                    "{aid} is listed twice for {platform:?}"
                );
            }
        }
    }
}
