//! The handset's own bitmap faces, read out of the firmware BIOS.
//!
//! The reference draws every string from a bitmap face, never from an outline.
//! Its font module carries a table of seven faces - 10, 12, 14, 16, 20, 22 and
//! 24 pixels tall - and `MC_grpGetFont` picks one of them from the size flag the
//! title passes, so a title asking for the small face gets 10-pixel glyphs that
//! advance 5 pixels for a byte character and 10 for a wide one, not the 12/6 of
//! the default face. Each face is three tables of 1-bit glyphs in the image:
//! ASCII, the 2350 composed Hangul syllables, and the KS X 1001 symbol rows.
//!
//! Rasterising an outline font in their place is what made text soft here. The
//! outline is anti-aliased, so every stroke carries a grey fringe the handset
//! never had, and `fonts/neodgm.ttf` is drawn on a 16-unit em - it lands on
//! whole pixels only at 16px and multiples of it, and the sizes WIPI actually
//! asks for all fall between the pixels.
//!
//! Nothing here reads the BIOS itself: the platform hands the image over once,
//! and with no BIOS supplied the outline path stays exactly as it was.

use alloc::{sync::Arc, vec::Vec};

use spin::RwLock;

/// KS X 1001 rows are 94 characters each.
const KS_ROW: usize = 94;
/// Glyphs in the ASCII table, one for every byte value.
const ASCII_GLYPHS: usize = 128;
/// Hangul occupies rows 0xb0..=0xc8 - the 2350 syllables of the composed set.
const HANGUL_GLYPHS: usize = 2350;
/// The symbol tables cover rows 0xa1..=0xac, ahead of the Hangul ones. The
/// reference's own lookup bounds the row at 0xac, so that is where they end.
const SYMBOL_ROWS: usize = 12;
const SYMBOL_GLYPHS: usize = SYMBOL_ROWS * KS_ROW;

/// Words in one entry of the firmware's face table: the size flag, the face's
/// metrics, and the addresses of its three glyph tables.
const FACE_RECORD_WORDS: usize = 11;
const FACE_RECORD_BYTES: usize = FACE_RECORD_WORDS * 4;
/// Faces the table has to hold to be taken for one. The reference's has seven;
/// two is enough to tell a table from a chance run of plausible words.
const MIN_FACES: usize = 2;

/// The faces the BIOS carries, if the platform supplied one.
static FACES: RwLock<Option<Arc<FaceSet>>> = RwLock::new(None);

/// The face for a `MC_grpGetFont` size flag, or the default face when the flag
/// names none - which is what the reference's own registry lookup falls back to.
pub fn face_for_size(size: u32) -> Option<Arc<BitmapFace>> {
    let faces = FACES.read().clone()?;
    Some(faces.by_size(size))
}

/// The face a font handle names. The handle is the face's pixel height, so a
/// title that carried one through `SetContext` is drawn and measured in the
/// same face it asked for; 0, or a height no face has, keeps the default.
pub fn face_for_height(height: u32) -> Option<Arc<BitmapFace>> {
    let faces = FACES.read().clone()?;
    Some(faces.by_height(height))
}

/// Forget the installed faces, so a title that supplies no BIOS is drawn with
/// the outline font even when one ran before it in this process.
pub fn clear() {
    *FACES.write() = None;
}

/// Take the faces out of a BIOS image and install them, reporting whether any
/// were found. An image without a recognisable face table leaves the outline
/// path in place.
pub fn install_from_bios(image: &[u8]) -> bool {
    match FaceSet::from_bios(image) {
        Some(faces) => {
            *FACES.write() = Some(Arc::new(faces));
            true
        }
        None => false,
    }
}

/// The faces of one family, in the order the firmware's table lists them.
struct FaceSet {
    faces: Vec<Arc<BitmapFace>>,
    /// Index of the face whose size flag is 0 - the one the reference registers
    /// as `"default"` and hands out for a size it does not recognise.
    default: usize,
}

impl FaceSet {
    fn by_size(&self, size: u32) -> Arc<BitmapFace> {
        self.faces
            .iter()
            .find(|face| face.size == size)
            .unwrap_or(&self.faces[self.default])
            .clone()
    }

    fn by_height(&self, height: u32) -> Arc<BitmapFace> {
        self.faces
            .iter()
            .find(|face| face.height == height)
            .unwrap_or(&self.faces[self.default])
            .clone()
    }

    /// Find the face table in a BIOS image.
    ///
    /// Each entry describes itself: the metrics have to add up (ascent and
    /// descent make the height, a row's bytes hold its pixels) and the three
    /// table addresses have to sit exactly the tables' own lengths apart. A run
    /// of entries that all check out, back to back, is the table; anything else
    /// in the image would have to agree with itself over eleven words twice
    /// over to be mistaken for one.
    fn from_bios(image: &[u8]) -> Option<Self> {
        let segments = Segments::of(image)?;

        let mut best: Vec<FaceRecord> = Vec::new();
        let mut at = 0;
        while at + FACE_RECORD_BYTES <= image.len() {
            let Some(first) = FaceRecord::read(image, &segments, at) else {
                at += 4;
                continue;
            };

            let mut run = alloc::vec![first];
            let mut next = at + FACE_RECORD_BYTES;
            while let Some(record) = FaceRecord::read(image, &segments, next) {
                run.push(record);
                next += FACE_RECORD_BYTES;
            }

            if run.len() > best.len() {
                best = run;
            }
            at += 4;
        }

        if best.len() < MIN_FACES {
            return None;
        }

        let faces: Vec<_> = best.iter().map(|record| Arc::new(record.face(image))).collect();
        let default = best.iter().position(|record| record.size == 0).unwrap_or(0);

        Some(Self { faces, default })
    }
}

/// One entry of the firmware's face table, checked against itself.
struct FaceRecord {
    size: u32,
    height: u32,
    ascent: u32,
    descent: u32,
    ascii_advance: u32,
    wide_advance: u32,
    ascii_bpr: usize,
    wide_bpr: usize,
    ascii_at: usize,
    hangul_at: usize,
    symbols_at: usize,
}

impl FaceRecord {
    fn read(image: &[u8], segments: &Segments, at: usize) -> Option<Self> {
        let word = |index: usize| -> Option<u32> {
            let start = at + index * 4;
            Some(u32::from_le_bytes(image.get(start..start + 4)?.try_into().ok()?))
        };

        let size = word(0)?;
        let wide_advance = word(1)?;
        let height = word(2)?;
        let ascii_advance = word(3)?;
        let ascent = word(4)?;
        let descent = word(5)?;
        let ascii_bpr = word(6)? as usize;
        let wide_bpr = word(7)? as usize;

        // The metrics have to describe a face: a row of pixels that fits the
        // bytes it is stored in, and a baseline that splits the height.
        if !(4..=64).contains(&height) || ascent.checked_add(descent) != Some(height) {
            return None;
        }
        if ascii_advance == 0 || ascii_advance > wide_advance || wide_advance > 64 {
            return None;
        }
        if ascii_bpr != ascii_advance.div_ceil(8) as usize || wide_bpr != wide_advance.div_ceil(8) as usize {
            return None;
        }

        // ... and the tables have to sit their own lengths apart, in the order
        // the face stores them.
        let ascii_at = segments.offset(word(8)?)?;
        let hangul_at = segments.offset(word(9)?)?;
        let symbols_at = segments.offset(word(10)?)?;

        let ascii_len = ASCII_GLYPHS * height as usize * ascii_bpr;
        let hangul_len = HANGUL_GLYPHS * height as usize * wide_bpr;
        let symbols_len = SYMBOL_GLYPHS * height as usize * wide_bpr;

        if hangul_at.checked_sub(ascii_at)? != ascii_len || symbols_at.checked_sub(hangul_at)? != hangul_len {
            return None;
        }
        if symbols_at + symbols_len > image.len() {
            return None;
        }

        Some(Self {
            size,
            height,
            ascent,
            descent,
            ascii_advance,
            wide_advance,
            ascii_bpr,
            wide_bpr,
            ascii_at,
            hangul_at,
            symbols_at,
        })
    }

    fn face(&self, image: &[u8]) -> BitmapFace {
        let ascii_len = ASCII_GLYPHS * self.height as usize * self.ascii_bpr;
        let hangul_len = HANGUL_GLYPHS * self.height as usize * self.wide_bpr;
        let symbols_len = SYMBOL_GLYPHS * self.height as usize * self.wide_bpr;

        BitmapFace {
            size: self.size,
            height: self.height,
            ascent: self.ascent,
            descent: self.descent,
            ascii_advance: self.ascii_advance,
            wide_advance: self.wide_advance,
            ascii_bpr: self.ascii_bpr,
            wide_bpr: self.wide_bpr,
            ascii: image[self.ascii_at..self.ascii_at + ascii_len].to_vec(),
            hangul: image[self.hangul_at..self.hangul_at + hangul_len].to_vec(),
            symbols: image[self.symbols_at..self.symbols_at + symbols_len].to_vec(),
        }
    }
}

/// The image's loadable segments, for turning an address the face table names
/// into an offset in the file. The firmware is an ELF shared object and its
/// tables are addressed as it will be mapped, not as it is stored.
struct Segments {
    /// `(vaddr, offset, filesz)` of every `PT_LOAD`.
    load: Vec<(u32, u32, u32)>,
}

impl Segments {
    fn of(image: &[u8]) -> Option<Self> {
        // 32-bit little-endian ELF, which is what the handset's firmware is.
        if image.get(..4)? != b"\x7fELF" || image.get(4)? != &1 || image.get(5)? != &1 {
            return None;
        }

        let half = |at: usize| -> Option<u16> { Some(u16::from_le_bytes(image.get(at..at + 2)?.try_into().ok()?)) };
        let word = |at: usize| -> Option<u32> { Some(u32::from_le_bytes(image.get(at..at + 4)?.try_into().ok()?)) };

        let phoff = word(0x1c)? as usize;
        let phentsize = half(0x2a)? as usize;
        let phnum = half(0x2c)? as usize;

        let mut load = Vec::new();
        for index in 0..phnum {
            let at = phoff + index * phentsize;
            if word(at)? != 1 {
                continue;
            }
            load.push((word(at + 8)?, word(at + 4)?, word(at + 0x10)?));
        }

        (!load.is_empty()).then_some(Self { load })
    }

    fn offset(&self, addr: u32) -> Option<usize> {
        self.load
            .iter()
            .find(|&&(vaddr, _, filesz)| addr >= vaddr && addr - vaddr < filesz)
            .map(|&(vaddr, offset, _)| (offset + (addr - vaddr)) as usize)
    }
}

/// One face: its metrics and its three glyph tables.
pub struct BitmapFace {
    /// The `MC_grpGetFont` size flag this face answers to.
    size: u32,
    /// Rows in a glyph, and so the face's height.
    pub height: u32,
    /// Distance from the top of the glyph box to the baseline.
    pub ascent: u32,
    /// Rows below the baseline.
    pub descent: u32,
    ascii_advance: u32,
    wide_advance: u32,
    ascii_bpr: usize,
    wide_bpr: usize,
    ascii: Vec<u8>,
    hangul: Vec<u8>,
    symbols: Vec<u8>,
}

/// One glyph: its rows, and how far the pen moves past it.
pub struct Glyph<'a> {
    rows: &'a [u8],
    bytes_per_row: usize,
    height: u32,
    pub advance: u32,
}

impl Glyph<'_> {
    /// Whether the pixel at `(x, y)` within the glyph box is inked.
    pub fn pixel(&self, x: u32, y: u32) -> bool {
        let byte = x as usize / 8;
        if byte >= self.bytes_per_row || y >= self.height {
            return false;
        }

        self.rows[y as usize * self.bytes_per_row + byte] & (0x80 >> (x % 8)) != 0
    }

    /// Pixels the glyph box spans, which is what the face advances by.
    pub fn width(&self) -> u32 {
        self.advance
    }
}

impl BitmapFace {
    /// The glyph for a character, by the EUC-KR code the handset stores it
    /// under. `None` for anything the face has no table for - a hanja, which
    /// these titles do not use, or a character outside KS X 1001.
    pub fn glyph(&self, c: char) -> Option<Glyph<'_>> {
        let ascii_stride = self.height as usize * self.ascii_bpr;
        if let Some(byte) = u8::try_from(u32::from(c)).ok().filter(|_| c.is_ascii()) {
            let at = byte as usize * ascii_stride;

            return Some(Glyph {
                rows: &self.ascii[at..at + ascii_stride],
                bytes_per_row: self.ascii_bpr,
                height: self.height,
                advance: self.ascii_advance,
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
            0xa1..=0xac => (&self.symbols, (high as usize - 0xa1) * KS_ROW + column),
            0xb0..=0xc8 => (&self.hangul, (high as usize - 0xb0) * KS_ROW + column),
            _ => return None,
        };

        let wide_stride = self.height as usize * self.wide_bpr;
        let at = index * wide_stride;
        if at + wide_stride > table.len() {
            return None;
        }

        Some(Glyph {
            rows: &table[at..at + wide_stride],
            bytes_per_row: self.wide_bpr,
            height: self.height,
            advance: self.wide_advance,
        })
    }

    /// Width of a string laid out in this face.
    pub fn string_width(&self, string: &str) -> u32 {
        string.chars().map(|c| self.glyph(c).map_or(0, |g| g.advance)).sum()
    }
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

    /// Metrics of the two faces the fixture carries, shaped like the firmware's
    /// own: `(size flag, height, ascent, descent, ascii advance, wide advance)`.
    const FIXTURE_FACES: [(u32, u32, u32, u32, u32, u32); 2] = [(8, 10, 8, 2, 5, 10), (0, 12, 9, 3, 6, 12)];

    /// An image shaped like the BIOS: an ELF header with one loadable segment
    /// mapped at a non-zero address, the glyph tables of two faces, and the
    /// face table naming them by address. Each wide glyph carries its own index
    /// so a lookup can be checked.
    fn fake_bios() -> Vec<u8> {
        const BASE: u32 = 0x1000;

        let mut image = vec![0u8; 0x100];
        image[..4].copy_from_slice(b"\x7fELF");
        image[4] = 1;
        image[5] = 1;
        image[0x1c..0x20].copy_from_slice(&0x40u32.to_le_bytes()); // e_phoff
        image[0x2a..0x2c].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
        image[0x2c..0x2e].copy_from_slice(&1u16.to_le_bytes()); // e_phnum

        // One PT_LOAD covering the whole file, mapped at BASE.
        image[0x40..0x44].copy_from_slice(&1u32.to_le_bytes());
        image[0x44..0x48].copy_from_slice(&0u32.to_le_bytes()); // p_offset
        image[0x48..0x4c].copy_from_slice(&BASE.to_le_bytes()); // p_vaddr
        image[0x50..0x54].copy_from_slice(&0x0100_0000u32.to_le_bytes()); // p_filesz

        let mut records = Vec::new();
        for (size, height, ascent, descent, ascii_advance, wide_advance) in FIXTURE_FACES {
            let ascii_at = image.len();
            for byte in 0..ASCII_GLYPHS {
                for row in 0..height as usize {
                    image.push(if byte >= 0x21 { ((byte + row) as u8) | 0x80 } else { 0 });
                }
            }

            let hangul_at = image.len();
            for index in 0..HANGUL_GLYPHS + SYMBOL_GLYPHS {
                // Carried as index + 1, so even the first glyph is drawn.
                let tag = index + 1;
                for _ in 0..height {
                    image.push((tag >> 8) as u8);
                    image.push(tag as u8);
                }
            }
            let symbols_at = hangul_at + HANGUL_GLYPHS * height as usize * 2;

            for word in [
                size,
                wide_advance,
                height,
                ascii_advance,
                ascent,
                descent,
                1,
                2,
                BASE + ascii_at as u32,
                BASE + hangul_at as u32,
                BASE + symbols_at as u32,
            ] {
                records.extend_from_slice(&word.to_le_bytes());
            }
        }
        image.extend_from_slice(&records);
        image.extend_from_slice(&[0u8; 64]);

        image
    }

    #[test]
    fn reads_every_face_the_table_lists() {
        let faces = FaceSet::from_bios(&fake_bios()).expect("faces");

        assert_eq!(faces.faces.len(), FIXTURE_FACES.len());
        for (size, height, ascent, descent, ascii_advance, wide_advance) in FIXTURE_FACES {
            let face = faces.by_size(size);
            assert_eq!((face.height, face.ascent, face.descent), (height, ascent, descent));
            assert_eq!(face.glyph('a').unwrap().advance, ascii_advance);
            assert_eq!(face.glyph('가').unwrap().advance, wide_advance);
        }
    }

    #[test]
    fn falls_back_to_the_default_face_for_a_size_it_has_no_face_for() {
        let faces = FaceSet::from_bios(&fake_bios()).expect("faces");

        // 0x8000 names no face here, so the size-0 face answers - the same
        // fallback the reference's registry lookup makes.
        assert_eq!(faces.by_size(0x8000).height, 12);
        assert_eq!(faces.by_height(9999).height, 12);
        assert_eq!(faces.by_height(10).height, 10);
    }

    #[test]
    fn indexes_hangul_by_its_euc_kr_row() {
        let faces = FaceSet::from_bios(&fake_bios()).expect("faces");
        let face = faces.by_size(0);

        // 가 opens the composed set, and 힝 closes it 2349 glyphs later. A
        // syllable the set leaves out - 힣, which Windows-949 adds - has no
        // glyph rather than an index off the front of the table.
        let index = |c| {
            let glyph = face.glyph(c).unwrap();
            (((glyph.rows[0] as usize) << 8) | glyph.rows[1] as usize) - 1
        };
        assert_eq!(index('가'), 0);
        assert_eq!(index('각'), 1);
        assert_eq!(index('힝'), HANGUL_GLYPHS - 1);
        assert!(face.glyph('힣').is_none());
    }

    #[test]
    fn measures_a_string_by_the_advances_of_its_face() {
        let faces = FaceSet::from_bios(&fake_bios()).expect("faces");

        assert_eq!(faces.by_size(0).string_width("가 a"), 12 + 6 + 6);
        assert_eq!(faces.by_size(8).string_width("가 a"), 10 + 5 + 5);
    }
}
