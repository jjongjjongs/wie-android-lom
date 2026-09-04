//! The handset's own bitmap face, read out of the firmware BIOS.
//!
//! The reference draws every string from one 12-pixel bitmap face. Its font
//! registry (`dfont_register`, walked by `dfont_get`) holds a single entry - the
//! `font_default` module registers name `"default"` at height 12, style 0 - so a
//! title asking `MC_grpGetFont` for any other height matches nothing, gets the
//! default face back, and is drawn with it regardless of the size it asked for.
//! That face is three tables of 1-bit glyphs in the BIOS image, and the module
//! that registers it carries their addresses along with its metrics: 12 rows
//! tall, 9 above the baseline and 3 below, 6 pixels of advance for a byte
//! character and 12 for a wide one.
//!
//! Rasterising an outline font in its place is what made text soft here. The
//! outline is anti-aliased, so every stroke carries a grey fringe the handset
//! never had, and `fonts/neodgm.ttf` is drawn on a 16-unit em - it lands on
//! whole pixels only at 16px and multiples of it, and the sizes WIPI actually
//! asks for (10, 12, 14, 18, 19, 22) all fall between the pixels.
//!
//! Nothing here reads the BIOS itself: the platform hands the image over once,
//! and with no BIOS supplied the outline path stays exactly as it was.

use alloc::{sync::Arc, vec::Vec};

use spin::RwLock;

/// Rows in a glyph, and so the face's height.
pub const GLYPH_HEIGHT: usize = 12;
/// Distance from the top of the glyph box to the baseline.
pub const GLYPH_ASCENT: u32 = 9;
/// Rows below the baseline.
pub const GLYPH_DESCENT: u32 = 3;
/// Advance of a byte (ASCII) character.
pub const ASCII_ADVANCE: u32 = 6;
/// Advance of a wide (KS X 1001) character.
pub const WIDE_ADVANCE: u32 = 12;

/// Glyphs in the ASCII table - one byte per row, so 8 pixels of which 6 are used.
const ASCII_GLYPHS: usize = 128;
const ASCII_STRIDE: usize = GLYPH_HEIGHT;
/// Glyphs in a wide table - two bytes per row, so 16 pixels of which 12 are used.
const WIDE_STRIDE: usize = GLYPH_HEIGHT * 2;
/// KS X 1001 rows are 94 characters each.
const KS_ROW: usize = 94;
/// Hangul occupies rows 0xb0..=0xc8 - the 2350 syllables of the composed set.
const HANGUL_GLYPHS: usize = 2350;
/// The symbol tables cover rows 0xa1..=0xaf, ahead of the Hangul ones.
const SYMBOL_GLYPHS: usize = 15 * KS_ROW;

/// The face the BIOS carries, if the platform supplied one.
static SYSTEM_FONT: RwLock<Option<Arc<BitmapFont>>> = RwLock::new(None);

/// The face installed for this run, or `None` while no BIOS has been supplied.
pub fn system_font() -> Option<Arc<BitmapFont>> {
    SYSTEM_FONT.read().clone()
}

/// Forget the installed face, so a title that supplies no BIOS is drawn with
/// the outline font even when one ran before it in this process.
pub fn clear() {
    *SYSTEM_FONT.write() = None;
}

/// Take the face out of a BIOS image and install it, reporting whether one was
/// found. An image without a recognisable face leaves the outline path in
/// place.
pub fn install_from_bios(image: &[u8]) -> bool {
    match BitmapFont::from_bios(image) {
        Some(font) => {
            *SYSTEM_FONT.write() = Some(Arc::new(font));
            true
        }
        None => false,
    }
}

pub struct BitmapFont {
    /// 128 glyphs, `ASCII_STRIDE` bytes each.
    ascii: Vec<u8>,
    /// The composed Hangul syllables, `WIDE_STRIDE` bytes each.
    hangul: Vec<u8>,
    /// KS X 1001 rows 0xa1..=0xaf, `WIDE_STRIDE` bytes each.
    symbols: Vec<u8>,
}

/// One glyph: its rows, and how far the pen moves past it.
pub struct Glyph<'a> {
    rows: &'a [u8],
    bytes_per_row: usize,
    pub advance: u32,
}

impl Glyph<'_> {
    /// Whether the pixel at `(x, y)` within the glyph box is inked.
    pub fn pixel(&self, x: u32, y: u32) -> bool {
        let byte = x as usize / 8;
        if byte >= self.bytes_per_row || y as usize >= GLYPH_HEIGHT {
            return false;
        }

        self.rows[y as usize * self.bytes_per_row + byte] & (0x80 >> (x % 8)) != 0
    }

    /// Pixels the glyph box spans, which is its advance except for a byte
    /// character, whose box is the byte it is stored in.
    pub fn width(&self) -> u32 {
        self.advance
    }
}

impl BitmapFont {
    /// The glyph for a character, by the EUC-KR code the handset stores it
    /// under. `None` for anything the face has no table for - a hanja, which
    /// these titles do not use, or a character outside KS X 1001.
    pub fn glyph(&self, c: char) -> Option<Glyph<'_>> {
        if let Some(byte) = u8::try_from(u32::from(c)).ok().filter(|_| c.is_ascii()) {
            let at = byte as usize * ASCII_STRIDE;

            return Some(Glyph {
                rows: &self.ascii[at..at + ASCII_STRIDE],
                bytes_per_row: 1,
                advance: ASCII_ADVANCE,
            });
        }

        // Only the 94-character rows of KS X 1001 are in the tables. The
        // encoder also has a code for every other Hangul syllable - Windows-949
        // extends the set, and a syllable it adds carries a low byte below the
        // rows - and those have no glyph here, so they are left undrawn rather
        // than indexed off the front of a table.
        let (high, low) = euc_kr_code(c)?;
        if !(0xa1..=0xfe).contains(&low) {
            return None;
        }

        let column = low as usize - 0xa1;
        let (table, index) = match high {
            0xa1..=0xaf => (&self.symbols, (high as usize - 0xa1) * KS_ROW + column),
            0xb0..=0xc8 => (&self.hangul, (high as usize - 0xb0) * KS_ROW + column),
            _ => return None,
        };

        let at = index * WIDE_STRIDE;
        if at + WIDE_STRIDE > table.len() {
            return None;
        }

        Some(Glyph {
            rows: &table[at..at + WIDE_STRIDE],
            bytes_per_row: 2,
            advance: WIDE_ADVANCE,
        })
    }

    /// Width of a string laid out in this face.
    pub fn string_width(&self, string: &str) -> u32 {
        string.chars().map(|c| self.glyph(c).map_or(0, |g| g.advance)).sum()
    }

    /// Find the face in a BIOS image.
    ///
    /// The tables sit one after another - ASCII, then Hangul, then the symbol
    /// rows - so finding the first locates all three. The ASCII table is found
    /// by what it must look like rather than by an address, so a differently
    /// built image is either recognised or rejected, never misread: 128 glyphs
    /// of 12 rows, blank through the control characters and the space, inked
    /// for the letters and digits, and every row within the 6 pixels the face
    /// advances by - which leaves the last two columns of the byte each row is
    /// stored in clear.
    ///
    /// The zero bytes ahead of the table let that description fit a few offsets
    /// short of it as well, each shifting every glyph by a whole word, so the
    /// match to take is the last of the run: one word further on and the ink of
    /// the first drawn glyph reaches the stretch that has to be blank.
    fn from_bios(image: &[u8]) -> Option<Self> {
        let ascii_len = ASCII_GLYPHS * ASCII_STRIDE;
        let hangul_len = HANGUL_GLYPHS * WIDE_STRIDE;
        let symbols_len = SYMBOL_GLYPHS * WIDE_STRIDE;
        let total = ascii_len + hangul_len + symbols_len;

        let matches = |at: usize| {
            at + total <= image.len()
                && is_ascii_table(&image[at..at + ascii_len])
                && is_wide_table(&image[at + ascii_len..at + ascii_len + hangul_len])
        };

        let mut at = (0..image.len().saturating_sub(total)).step_by(4).find(|&at| matches(at))?;
        while matches(at + 4) {
            at += 4;
        }

        Some(Self {
            ascii: image[at..at + ascii_len].to_vec(),
            hangul: image[at + ascii_len..at + ascii_len + hangul_len].to_vec(),
            symbols: image[at + ascii_len + hangul_len..at + total].to_vec(),
        })
    }
}

/// Whether a run of bytes is the face's ASCII table. See `from_bios`.
fn is_ascii_table(table: &[u8]) -> bool {
    // Nothing is drawn for the control characters or the space.
    if table[..0x21 * ASCII_STRIDE].iter().any(|&b| b != 0) {
        return false;
    }

    // Every row fits the 6 pixels the face advances by, so the last two columns
    // of the byte it is stored in are never inked.
    if table.iter().any(|&b| b & 0x03 != 0) {
        return false;
    }

    // The letters and digits are drawn.
    ['A', 'Z', 'a', 'z', '0', '9', '!']
        .iter()
        .all(|&c| table[c as usize * ASCII_STRIDE..(c as usize + 1) * ASCII_STRIDE].iter().any(|&b| b != 0))
}

/// Whether a run of bytes is one of the face's wide tables: 12-pixel glyphs in
/// 16-pixel rows, so the low nibble of each row's second byte is always clear,
/// and a Hangul table opens on a drawn syllable.
fn is_wide_table(table: &[u8]) -> bool {
    if table[1..WIDE_STRIDE].iter().step_by(2).any(|&b| b & 0x0f != 0) {
        return false;
    }

    table[..WIDE_STRIDE].iter().any(|&b| b != 0)
}

/// The EUC-KR code a character is stored under, for the two-byte range the
/// wide tables are indexed by.
fn euc_kr_code(c: char) -> Option<(u8, u8)> {
    let mut buffer = [0u8; 4];
    let text = c.encode_utf8(&mut buffer);

    let (encoded, _, had_errors) = encoding_rs::EUC_KR.encode(text);
    if had_errors || encoded.len() != 2 {
        return None;
    }

    Some((encoded[0], encoded[1]))
}

#[cfg(test)]
mod tests {
    use alloc::{vec, vec::Vec};

    use super::*;

    /// A face shaped like the one in the BIOS: blank through the control
    /// characters and the space, every ASCII glyph inked within six columns,
    /// and each wide glyph carrying its own index so a lookup can be checked.
    fn fake_bios() -> Vec<u8> {
        let mut image = vec![0u8; 64];

        let mut ascii = vec![0u8; ASCII_GLYPHS * ASCII_STRIDE];
        for c in 0x21..ASCII_GLYPHS {
            for row in 0..ASCII_STRIDE {
                ascii[c * ASCII_STRIDE + row] = ((c + row) as u8) & 0xfc;
            }
            // Every glyph is drawn, whatever the pattern worked out to.
            ascii[c * ASCII_STRIDE] |= 0x80;
        }
        image.extend_from_slice(&ascii);

        for index in 0..HANGUL_GLYPHS + SYMBOL_GLYPHS {
            // Carried as index + 1, so even the first glyph is drawn - a blank
            // one would look like the run of zeros ahead of the table.
            let tag = index + 1;
            for _ in 0..GLYPH_HEIGHT {
                image.push((tag >> 4) as u8);
                image.push(((tag << 4) as u8) & 0xf0);
            }
        }
        image.extend_from_slice(&[0u8; 64]);

        image
    }

    #[test]
    fn finds_the_face_past_the_zeros_ahead_of_it() {
        let font = BitmapFont::from_bios(&fake_bios()).expect("face");

        // Landing a word short would shift every glyph, so the space has to be
        // blank and the glyph after it drawn.
        assert!(font.glyph(' ').unwrap().rows.iter().all(|&b| b == 0));
        assert!(font.glyph('!').unwrap().rows.iter().any(|&b| b != 0));
    }

    #[test]
    fn indexes_hangul_by_its_euc_kr_row() {
        let font = BitmapFont::from_bios(&fake_bios()).expect("face");

        // 가 opens the composed set, and 힝 closes it 2349 glyphs later. A
        // syllable the set leaves out - 힣, which Windows-949 adds - has no
        // glyph rather than an index off the front of the table.
        let index = |c| {
            let glyph = font.glyph(c).unwrap();
            (((glyph.rows[0] as usize) << 4) | (glyph.rows[1] as usize) >> 4) - 1
        };
        assert_eq!(index('가'), 0);
        assert_eq!(index('각'), 1);
        assert_eq!(index('힝'), HANGUL_GLYPHS - 1);
        assert!(font.glyph('힣').is_none());
    }

    #[test]
    fn measures_a_string_by_its_advances() {
        let font = BitmapFont::from_bios(&fake_bios()).expect("face");

        assert_eq!(font.string_width("ab"), 2 * ASCII_ADVANCE);
        assert_eq!(font.string_width("가나"), 2 * WIDE_ADVANCE);
        assert_eq!(font.string_width("가 a"), WIDE_ADVANCE + 2 * ASCII_ADVANCE);
    }
}
