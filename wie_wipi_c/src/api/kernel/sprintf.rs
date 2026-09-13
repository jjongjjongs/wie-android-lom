use alloc::{format, string::String, vec::Vec};

use wie_util::{Result, read_null_terminated_string_bytes};

use crate::context::WIPICContext;

const MAX_WIDTH: usize = 4096;

pub fn sprintf(context: &mut dyn WIPICContext, format: &[u8], args: &[u32]) -> Result<Vec<u8>> {
    self::format(format, args, &mut |ptr| read_null_terminated_string_bytes(context, ptr))
}

/// Formats `format` with `args`, resolving `%s` pointers through `read_string`.
/// Exposed so callers that hold something other than a `WIPICContext` - the LGT
/// stdlib works straight off `ArmCore` - can format with their own reader.
///
/// `read_string` hands back the guest's bytes rather than text, because a
/// string's precision is a count of those: 와일드프론티어 draws a line of script
/// as `%.*s` over a buffer that is not terminated between lines, and the count
/// it passes is how many bytes that line is. Decoding first and counting
/// characters would run one line into the next.
///
/// Conversions carry the flags, width and precision C gives them, because
/// titles build fixed-width records with them: 아니마 writes its billing request
/// as `AM%-6d%10.10s%2.2s`, a twenty byte header it then copies out by length,
/// and a conversion that ignored the width would leave it the wrong size.
///
/// Everything here is bytes, start to finish, and none of it goes through text.
/// The format is the guest's own EUC-KR and so is the result, and a byte that
/// is not a character in either of them has to come out as itself. 창세기전3
/// 에피소드3 builds its script a byte at a time with `sprintk(dest, "%c", b)`,
/// and half of a Korean character is one of those bytes: decoded it is a
/// replacement character, and encoded back it is the eight bytes `&#65533;`,
/// which is six more than the caller counted on and walks its buffer straight
/// through the block behind it. The heap's free list is what was behind it, and
/// the title died in its own allocator a few frames later.
pub fn format(format: &[u8], args: &[u32], read_string: &mut dyn FnMut(u32) -> Result<Vec<u8>>) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(format.len());
    let mut chars = format.iter().copied();
    let mut arg_iter = args.iter();

    while let Some(x) = chars.next() {
        if x != b'%' {
            result.push(x);
            continue;
        }

        let mut spec = alloc::vec![b'%'];
        let mut conversion = Conversion::default();
        // Digits belong to the precision once a `.` has been seen, and to the
        // width before it.
        let mut in_precision = false;
        let mut longs = 0u32;

        loop {
            let Some(c) = chars.next() else {
                // broken format: emit what we have as-is
                result.extend_from_slice(&spec);
                break;
            };
            spec.push(c);

            match c {
                b'%' => {
                    result.push(b'%');
                    break;
                }
                b'd' | b'u' => {
                    // ILP32 ABI: long is one word; only long long occupies two
                    let long = longs >= 2;
                    let raw = if long {
                        next_arg64(&mut arg_iter)
                    } else {
                        next_arg(&mut arg_iter) as u64
                    };

                    let (negative, magnitude) = if c == b'd' {
                        let arg = if long { raw as i64 } else { raw as u32 as i32 as i64 };
                        (arg < 0, arg.unsigned_abs())
                    } else {
                        (false, raw)
                    };

                    conversion.push_number(&mut result, negative, &format!("{magnitude}"));
                    break;
                }
                b's' => {
                    let ptr = next_arg(&mut arg_iter);
                    let value = if ptr == 0 { Vec::from(*b"(null)") } else { read_string(ptr)? };

                    conversion.push_string(&mut result, &value);
                    break;
                }
                b'c' => {
                    let value = next_arg(&mut arg_iter) as u8;

                    conversion.push_string(&mut result, &[value]);
                    break;
                }
                b'x' => {
                    let arg = if longs >= 2 {
                        next_arg64(&mut arg_iter)
                    } else {
                        next_arg(&mut arg_iter) as u64
                    };

                    conversion.push_number(&mut result, false, &format!("{arg:x}"));
                    break;
                }
                b'l' => longs += 1,
                // `*` takes the field from the arguments, ahead of the value it
                // measures. A negative width is C's other way of writing `-`,
                // and a negative precision is no precision at all.
                b'*' => {
                    let field = next_arg(&mut arg_iter) as i32;

                    if in_precision {
                        conversion.precision = (field >= 0).then_some(field as usize);
                    } else if field < 0 {
                        conversion.left = true;
                        conversion.width = Some(field.unsigned_abs() as usize);
                    } else {
                        conversion.width = Some(field as usize);
                    }
                }
                b'-' if conversion.width.is_none() && !in_precision => conversion.left = true,
                b'0' if conversion.width.is_none() && !in_precision => conversion.zero = true,
                b'.' if !in_precision => {
                    in_precision = true;
                    conversion.precision = Some(0);
                }
                b'0'..=b'9' => {
                    let digit = (c - b'0') as usize;
                    let field = if in_precision {
                        &mut conversion.precision
                    } else {
                        &mut conversion.width
                    };

                    *field = Some(field.unwrap_or(0).saturating_mul(10).saturating_add(digit));
                }
                _ => {
                    tracing::warn!("unsupported format specifier: {}", String::from_utf8_lossy(&spec));
                    result.extend_from_slice(&spec);
                    break;
                }
            }
        }
    }

    Ok(result)
}

/// One conversion's flags, width and precision, and how they lay a value out.
#[derive(Default)]
struct Conversion {
    /// `-`: pad on the right instead of the left.
    left: bool,
    /// `0`: pad a number with zeros rather than spaces.
    zero: bool,
    width: Option<usize>,
    precision: Option<usize>,
}

impl Conversion {
    /// Both are guest-controlled, and `core::fmt` panics on a width at or above
    /// 65536, so neither may reach a formatter unclamped.
    fn width(&self) -> usize {
        self.width.unwrap_or(0).min(MAX_WIDTH)
    }

    fn precision(&self) -> Option<usize> {
        self.precision.map(|precision| precision.min(MAX_WIDTH))
    }

    /// A number, whose precision is the fewest digits to print and whose `0`
    /// flag C ignores when a precision is given.
    fn push_number(&self, result: &mut Vec<u8>, negative: bool, digits: &str) {
        let digits = digits.as_bytes();
        let zeros = self.precision().unwrap_or(0).saturating_sub(digits.len());
        let sign: &[u8] = if negative { b"-" } else { b"" };
        let length = sign.len() + zeros + digits.len();
        let padding = self.width().saturating_sub(length);

        if self.left {
            push_body(result, sign, zeros, digits);
            result.extend(core::iter::repeat_n(b' ', padding));
        } else if self.zero && self.precision.is_none() {
            result.extend_from_slice(sign);
            result.extend(core::iter::repeat_n(b'0', padding));
            push_body(result, b"", zeros, digits);
        } else {
            result.extend(core::iter::repeat_n(b' ', padding));
            push_body(result, sign, zeros, digits);
        }
    }

    /// A string, whose precision is the most bytes to print and whose width is
    /// counted in them too, as C counts both.
    ///
    /// The bytes go through untouched. They are the guest's own EUC-KR and the
    /// result goes back to the guest, so there is nothing for a decode to do
    /// here but lose - see [`format`]. Cutting at a precision that falls inside
    /// a character is what C does and what the reference would have drawn, and
    /// the half character stays half a character rather than becoming anything
    /// longer.
    fn push_string(&self, result: &mut Vec<u8>, value: &[u8]) {
        let taken = self.precision().unwrap_or(value.len()).min(value.len());
        let padding = self.width().saturating_sub(taken);

        if !self.left {
            result.extend(core::iter::repeat_n(b' ', padding));
        }

        result.extend_from_slice(&value[..taken]);

        if self.left {
            result.extend(core::iter::repeat_n(b' ', padding));
        }
    }
}

fn push_body(result: &mut Vec<u8>, sign: &[u8], zeros: usize, digits: &[u8]) {
    result.extend_from_slice(sign);
    result.extend(core::iter::repeat_n(b'0', zeros));
    result.extend_from_slice(digits);
}

fn next_arg<'a>(arg_iter: &mut impl Iterator<Item = &'a u32>) -> u32 {
    arg_iter.next().copied().unwrap_or_else(|| {
        tracing::warn!("printf: more format specifiers than arguments");
        0
    })
}

// long arguments occupy two consecutive words, low word first
fn next_arg64<'a>(arg_iter: &mut impl Iterator<Item = &'a u32>) -> u64 {
    let low = next_arg(arg_iter) as u64;
    let high = next_arg(arg_iter) as u64;

    (high << 32) | low
}

#[cfg(test)]
mod test {
    use alloc::{string::String, vec::Vec};

    use wie_util::Result;

    /// The formatter's own result is bytes; these cases are all ASCII, so read
    /// them back as text to keep them legible.
    fn format(format_string: &str, args: &[u32]) -> Result<String> {
        let bytes = super::format(format_string.as_bytes(), args, &mut |_| Ok(Vec::from(*b"stub")))?;

        Ok(String::from_utf8(bytes).unwrap())
    }

    /// The same for a case that reads its own strings.
    fn format_with(format_string: &str, args: &[u32], read: &mut dyn FnMut(u32) -> Result<Vec<u8>>) -> Result<Vec<u8>> {
        super::format(format_string.as_bytes(), args, read)
    }

    #[test]
    fn test_unsigned() -> Result<()> {
        assert_eq!(format("%u", &[0xffff_ffff])?, "4294967295");

        Ok(())
    }

    #[test]
    fn test_unknown_specifier_passthrough() -> Result<()> {
        assert_eq!(format("a%qb", &[])?, "a%qb");

        Ok(())
    }

    #[test]
    fn test_more_specifiers_than_args() -> Result<()> {
        assert_eq!(format("%d %d %d %d %d", &[1, 2, 3, 4])?, "1 2 3 4 0");

        Ok(())
    }

    #[test]
    fn test_width_and_zero_flag() -> Result<()> {
        assert_eq!(format("%02d", &[1])?, "01");
        assert_eq!(format("%10d", &[42])?, "        42");
        assert_eq!(format("%d", &[0xffff_ffff])?, "-1");

        Ok(())
    }

    #[test]
    fn test_long_specifiers() -> Result<()> {
        // ILP32: long is one word, so %ld must not shift later arguments
        assert_eq!(format("%ld", &[0xffff_ffff])?, "-1");
        assert_eq!(format("%lu", &[0xffff_ffff])?, "4294967295");
        assert_eq!(format("%ld %d", &[1, 42])?, "1 42");

        // long long arguments are two words, low first
        assert_eq!(format("%lld", &[0xffff_ffff, 0xffff_ffff])?, "-1");
        assert_eq!(format("%llu", &[0xffff_ffff, 0xffff_ffff])?, "18446744073709551615");
        assert_eq!(format("%llx", &[0x9abc_def0, 0x1234_5678])?, "123456789abcdef0");
        assert_eq!(format("%lld %d", &[1, 0, 42])?, "1 42");

        Ok(())
    }

    #[test]
    fn test_hex_width() -> Result<()> {
        assert_eq!(format("%08x", &[0xbeef])?, "0000beef");
        assert_eq!(format("%8x", &[0xbeef])?, "    beef");

        Ok(())
    }

    #[test]
    fn test_huge_width_is_clamped() -> Result<()> {
        // core::fmt panics on width >= 65536; must not propagate guest width unclamped
        assert_eq!(format("%65536d", &[1])?.len(), 4096);
        assert_eq!(format("%99999999999999999999d", &[1])?.len(), 4096);

        Ok(())
    }

    #[test]
    fn test_string_and_null() -> Result<()> {
        assert_eq!(format("%s!", &[1])?, "stub!");
        assert_eq!(format("%s", &[0])?, "(null)");

        Ok(())
    }

    #[test]
    fn test_left_justify() -> Result<()> {
        assert_eq!(format("%-6d|", &[42])?, "42    |");
        assert_eq!(format("%-6d|", &[0xffff_ffff])?, "-1    |");
        assert_eq!(format("%-6s|", &[1])?, "stub  |");
        assert_eq!(format("%-6x|", &[0xbeef])?, "beef  |");

        // A width the value already fills is not padded either way round.
        assert_eq!(format("%-2d|", &[42])?, "42|");

        Ok(())
    }

    #[test]
    fn test_precision() -> Result<()> {
        // On a number it is the fewest digits, and the sign sits outside them.
        assert_eq!(format("%.3d", &[7])?, "007");
        assert_eq!(format("%.3d", &[0xffff_fff9])?, "-007");
        assert_eq!(format("%.1d", &[1234])?, "1234");

        // C ignores the zero flag when a precision is given, and pads with
        // spaces to the width instead.
        assert_eq!(format("%08.3d|", &[7])?, "     007|");

        // On a string it is the most characters.
        assert_eq!(format("%.2s|", &[1])?, "st|");
        assert_eq!(format("%.9s|", &[1])?, "stub|");
        assert_eq!(format("%10.10s|", &[1])?, "      stub|");
        assert_eq!(format("%-10.2s|", &[1])?, "st        |");

        Ok(())
    }

    #[test]
    fn test_the_fixed_width_record_anima_builds() -> Result<()> {
        // 아니마 (0003266D) builds its billing header at 0x3d954 as
        // `sprintk(dest, "AM%-6d%10.10s%2.2s", length, subscriber, "10")` and
        // copies out exactly twenty bytes of it. The length is the whole
        // record's, header included - `0x3d910` computes it as the payload plus
        // twenty and hands the same value to the send - and the subscriber is
        // the ten digits `0xf03c` copies in.
        let mut read = |ptr: u32| {
            Ok(match ptr {
                1 => Vec::from(*b"1911112222"),
                _ => Vec::from(*b"10"),
            })
        };
        let header = format_with("AM%-6d%10.10s%2.2s", &[38, 1, 2], &mut read)?;

        // Which is what the twenty bytes have to come to. A conversion that
        // dropped the width would have made it eighteen, and the record it
        // fronts is measured by the number inside it.
        assert_eq!(header, b"AM38    191111222210");
        assert_eq!(header.len(), 20);

        Ok(())
    }

    #[test]
    fn test_star_takes_the_field_from_the_arguments() -> Result<()> {
        // Ahead of the value it measures, in argument order.
        assert_eq!(format("%*d|", &[6, 42])?, "    42|");
        assert_eq!(format("%.*s|", &[2, 1])?, "st|");
        assert_eq!(format("%*.*s|", &[6, 2, 1])?, "    st|");

        // A negative width is C's other way of writing `-`, and a negative
        // precision is no precision at all.
        assert_eq!(format("%*d|", &[-6i32 as u32, 42])?, "42    |");
        assert_eq!(format("%.*s|", &[-1i32 as u32, 1])?, "stub|");

        // And the star's own argument is not left for the value.
        assert_eq!(format("%.*s %d", &[2, 1, 7])?, "st 7");

        Ok(())
    }

    #[test]
    fn test_a_string_is_measured_in_the_bytes_it_came_as() -> Result<()> {
        // 와일드프론티어 draws one line of a script buffer as `%.*s`, and the
        // count is that line's length in bytes - the buffer is not terminated
        // between lines, so a character count would run into the next one.
        // 프론티어호에 탔던 건 is twenty EUC-KR bytes, twelve of them the first
        // six characters - which a character count would read as twenty.
        let line: Vec<u8> = alloc::vec![
            0xc7, 0xc1, 0xb7, 0xd0, 0xc6, 0xbc, 0xbe, 0xee, 0xc8, 0xa3, 0xbf, 0xa1, 0x20, 0xc5, 0xc0, 0xb4, 0xf8, 0x20, 0xb0, 0xc7
        ];
        assert_eq!(line.len(), 20);

        let mut read = |_| Ok(line.clone());
        assert_eq!(format_with("%.*s", &[12, 1], &mut read)?, line[..12]);
        assert_eq!(format_with("%.*s", &[20, 1], &mut read)?, line[..]);

        // Past its end is the whole of it, not a read past it.
        assert_eq!(format_with("%.*s", &[99, 1], &mut read)?, line[..]);

        Ok(())
    }

    #[test]
    fn test_the_dialogue_line_wild_frontier_draws() -> Result<()> {
        // 와일드프론티어's speech format at 0x4a9d8, `\x07` and a digit being its
        // own colour escape: the speaker's name, then a line of the script
        // buffer measured by the count that comes with it.
        let mut read = |ptr: u32| {
            Ok(match ptr {
                1 => Vec::from(*b"KRISNOAH"),
                _ => Vec::from(*b"who are you? and more script behind it"),
            })
        };
        let line = format_with("\u{7}2[%s]\u{7}0%.*s", &[1, 16, 2], &mut read)?;

        // Nothing of the format is left in it, and the line stops where its
        // count says rather than running on into the next one.
        assert_eq!(line, b"\x072[KRISNOAH]\x070who are you? and");
        assert!(!line.contains(&b'%'));

        Ok(())
    }

    #[test]
    fn a_percent_c_is_the_byte_it_was_given() -> Result<()> {
        // 창세기전3 에피소드3 builds its script one byte at a time, and half of
        // a Korean character is one of those bytes. It has to come out as
        // itself: as text it is a replacement character, and encoded back it
        // would be the eight bytes `&#65533;` - six more than the caller
        // counted, into the block behind its buffer.
        for byte in [0x00u8, 0x20, 0x41, 0x7f, 0x80, 0xa1, 0xbb, 0xc7, 0xfe, 0xff] {
            let out = super::format(b"%c", &[byte as u32], &mut |_| Ok(Vec::new()))?;

            assert_eq!(out, [byte], "%c of {byte:#04x}");
        }

        Ok(())
    }

    #[test]
    fn a_percent_s_is_the_bytes_it_was_given() -> Result<()> {
        // Whole or half a character, and a byte no encoding claims: the string
        // goes back to the guest, so it goes through as it came.
        let value: Vec<u8> = alloc::vec![0xc7, 0xd1, 0xb1, 0xdb, 0xbb, 0xff, 0x80];
        let mut read = |_| Ok(value.clone());

        assert_eq!(format_with("%s", &[1], &mut read)?, value);
        // Cut inside a character, the half that is left stays one byte.
        assert_eq!(format_with("%.3s", &[1], &mut read)?, value[..3]);

        Ok(())
    }

    /// The format's own bytes are the guest's too, and Korean in it is not
    /// there to be decoded and put back.
    #[test]
    fn the_format_carries_its_own_bytes_through() -> Result<()> {
        // 한글 in EUC-KR, around a conversion.
        let format: Vec<u8> = alloc::vec![0xc7, 0xd1, 0xb1, 0xdb, b'%', b'd', 0xbb, 0xf3];
        let out = super::format(&format, &[7], &mut |_| Ok(Vec::new()))?;

        assert_eq!(out, [0xc7, 0xd1, 0xb1, 0xdb, b'7', 0xbb, 0xf3]);

        Ok(())
    }

    #[test]
    fn test_precision_is_clamped_like_width() -> Result<()> {
        // Guest-controlled, and `core::fmt` is not what pads here, but a
        // precision that allocated unclamped would be a way to exhaust memory.
        assert_eq!(format("%.65536d", &[1])?.len(), 4096);
        assert_eq!(format("%.99999999999999999999s", &[1])?, "stub");

        Ok(())
    }
}
