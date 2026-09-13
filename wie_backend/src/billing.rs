//! The services these titles reached over a billing socket, answered in process.
//!
//! A handset opened a socket to the carrier or the publisher to authenticate a
//! copy, to sell an item, or - for a title whose online menu went the same way -
//! to log in, and every one of those services has been switched off for years. A
//! title that reaches one and is told nothing usually stops on a screen it never
//! leaves.
//!
//! Twelve protocols turn up across the titles here, and a request is recognised by
//! its own shape rather than by which title sent it. Anything that is not one of
//! them is left unanswered rather than guessed at.
//!
//! Both API surfaces reach these: a WIPI-C title through `MC_netBillSocket`, a
//! WIPI-Java one through `org.kwis.msf.io.URL`'s `BillSocket://`. They answer
//! the same protocols, so the answers live here rather than in either.

use alloc::{format, string::String, vec, vec::Vec};

/// The seven-byte granted frame this protocol's answers are: the `0xffff`
/// marker, the length, the message type, and a zero status.
const GRANTED_FRAME_SIZE: usize = 7;

/// Which end of a billing frame's `u16` header fields comes first.
///
/// The frames carry a `0xffff` marker, a length and a message type, and titles
/// do not agree on how the two `u16`s are laid out: 붉은보석 writes a 19-byte
/// purchase request as `ff ff 13 00 68 00 ...`, little end first, where the
/// same frame reconstructed big-endian would be `ff ff 00 13 00 68 ...`. The
/// marker is a palindrome and says nothing, so the length is what tells them
/// apart - only one reading of it can describe the frame in hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BillFrameOrder {
    Big,
    Little,
}

impl BillFrameOrder {
    pub fn read(self, bytes: [u8; 2]) -> u16 {
        match self {
            Self::Big => u16::from_be_bytes(bytes),
            Self::Little => u16::from_le_bytes(bytes),
        }
    }

    pub fn write(self, value: u16) -> [u8; 2] {
        match self {
            Self::Big => value.to_be_bytes(),
            Self::Little => value.to_le_bytes(),
        }
    }
}

/// A billing frame's header, read in whichever order its own length makes sense
/// in.
pub struct BillFrame {
    pub order: BillFrameOrder,
    pub message_type: u16,
}

impl BillFrame {
    /// `None` for anything that is not one of these frames: too short to carry
    /// a header, no `0xffff` marker, or a length that describes no frame this
    /// could be under either reading.
    ///
    /// A native client may hand `MC_netSocketWrite` only part of the frame it
    /// built - 붉은보석 declares nineteen bytes and has been seen writing ten -
    /// so a length longer than the slice is accepted. It has to be at least the
    /// six a header takes, and where neither reading is exact the shorter one
    /// wins, being the one that could still be this frame.
    pub fn parse(request: &[u8]) -> Option<Self> {
        if request.len() < 6 || request[0] != 0xff || request[1] != 0xff {
            return None;
        }

        let length = [request[2], request[3]];
        let mut best: Option<(BillFrameOrder, usize)> = None;

        for order in [BillFrameOrder::Big, BillFrameOrder::Little] {
            let declared = order.read(length) as usize;

            if declared < 6 || declared < request.len() {
                continue;
            }

            if best.is_none_or(|(_, shortest)| declared < shortest) {
                best = Some((order, declared));
            }
        }

        let (order, _) = best?;

        Some(Self {
            order,
            message_type: order.read([request[4], request[5]]),
        })
    }
}

/// A billing frame as a trace line: its header fields read out, then the bytes.
///
/// These frames all start `ffff`, a `u16` length and a `u16` type, so naming
/// those three is what makes a capture readable without counting nibbles. Read
/// the way [`BillFrame`] reads them, and named with the order it settled on, so
/// a trace shows what the code acted on rather than one guess at it. Anything
/// this cannot read as a frame is shown as bytes alone. Capped, because a trace
/// is for reading.
pub fn bill_frame_trace(frame: &[u8]) -> String {
    // Enough for a whole request. 제노니아1's is 72 bytes and a 64-byte cap cut
    // off the end of it, which is the half that says what the title asked for.
    const SHOWN: usize = 256;

    let bytes: Vec<String> = frame.iter().take(SHOWN).map(|byte| format!("{byte:02x}")).collect();
    let bytes = format!("{}{}", bytes.join(" "), if frame.len() > SHOWN { " ..." } else { "" });

    let Some(parsed) = BillFrame::parse(frame) else {
        return format!("{} bytes [{bytes}]", frame.len());
    };

    format!(
        "{:?}-endian type {:#06x} len {} of {} bytes [{bytes}]",
        parsed.order,
        parsed.message_type,
        parsed.order.read([frame[2], frame[3]]),
        frame.len(),
    )
}
/// The answer to the pipe-delimited cash request NHN's titles send.
///
/// 데몬헌터 (`0002B5EB`) opens a billing socket to `222.237.78.175` and writes an
/// ASCII record rather than a framed message:
///
/// ```text
/// CASH|0|demon|05590091|00029B60004|500|2034517541
/// ```
///
/// - the transaction, the game's own code and account, the item code, its price
///   in won, and a token. The item codes and prices are a table in the title's
///   own `binary.mod`, `00029B60001|100|` through `0002B640007|2900|`.
///
/// What it does with the answer is a chain of string compares at `0x21004`:
/// equal to `SASH` takes the branch that shows 결제가 완료되었습니다, and the
/// two failures it knows by name are `SFL|MOVER` (monthly purchase limit) and
/// `SFL|PNUM` (staff accounts). Anything else - including the nothing a
/// switched-off gateway returns - falls through to 네트워크 장애가
/// 발생했습니다, which is the notice the title cannot get past.
///
/// The answer carries its own length ahead of it. The title's receive is a
/// two-step state machine at `0x20f76`: state 6 recvs exactly two bytes, reads
/// them as a `u16` and passes that through `MC_utilHtons` - so the field is
/// big-endian on the wire - and state 7 recvs exactly that many bytes and
/// compares them. The length counts the body alone; the two it was read from
/// are already consumed.
///
/// Answered `SASH` bare, the title read `SA` as its length, made 0x5341 of it
/// and waited for 21313 bytes that were never coming - which the trace caught
/// as `MC_utilHtons(0x4153)` on the very next line.
///
/// So the answer is `00 04` then `SASH`, and nothing after it: the compare is
/// an equality against a string built to the length just read.
///
/// `None` for anything that is not one of these records, which is not something
/// to answer with a guess.
pub fn lgt_local_cash_response(request: &[u8]) -> Option<Vec<u8>> {
    const REQUEST: &[u8] = b"CASH|";
    const GRANTED: &[u8] = b"SASH";

    if !request.starts_with(REQUEST) {
        return None;
    }

    let mut response = Vec::with_capacity(2 + GRANTED.len());
    response.extend_from_slice(&(GRANTED.len() as u16).to_be_bytes());
    response.extend_from_slice(GRANTED);

    Some(response)
}
/// What GAMEVIL's server answers one of its titles' purchases with.
///
/// 제노니아1 (`00027BAA`) opens a billing socket to `218.145.70.36:31206` and
/// writes a 72-byte record; 제노니아2 (`0002C004`) and 3 (`0002FE78`) write a
/// 93-byte one to the same place. They are the same record with a tail added:
///
/// ```text
/// [0..2]   u16 LE - the record's own length
/// [2..4]   u16 LE - the command
/// [4..16]  the subscriber number
/// [16..56] the item, EUC-KR
/// [56..60] u32 LE - the price in won
/// [60..72] the item code, the title's own aid and an index
/// [72..]   2 and 3 add a flag and the handset model
/// ```
///
/// The reply is settled by GAMEVIL's own later Android port of 제노니아1, which
/// carries C++ symbols for the protocol these titles speak - the same server
/// address and the same `00027BAA00n` item codes are strings inside it.
///
/// `tagNetHeader` is four bytes: `GetLength` reads a `u16` at `[0]`, `GetCMD` a
/// `u16` at `[2]`, and `CGsNetCore::GetRecvPacketHeaderSize` returns 4.
/// `CMvNet::OnRecvDone` then skips the header, reads one **signed byte** as the
/// status and calls `OnError(cmd, status)` when it is below `-1`, and otherwise
/// switches on the command over a fixed list - `0x101`, `0x103`, ... `0x701`,
/// `0x805` - dropping anything not on it without a word.
///
/// Every command a title sends is even and the answer to it is that command plus
/// one: `0x0700` is `CS_BUY_ITEM` and `0x0701` reaches `API_ZN_SC_BUY_ITEM`,
/// which reads nothing out of the body and calls a single callback. 제노니아1
/// buys with `0x0700`, 2 and 3 with `0x0400`, so each is answered with its own
/// command plus one.
///
/// Which accounts for every sweep run at 제노니아1 before the port settled it.
/// `0x107`, `0x103` and `0x101` are real commands, so they were dispatched - to
/// handlers with nothing to say. `0x0700` and `0x0007`, answered as though the
/// command were the request's own, are not commands at all and were dropped. And
/// every reply whose status came out negative drew a red message, because the
/// status is read before the command is looked at.
///
/// `None` for anything that is not one of these records: the declared length has
/// to be the record in hand, it has to be long enough to carry a purchase, and
/// the command has to be one a title sends rather than one it is sent.
pub fn lgt_local_gamevil_packet_response(request: &[u8]) -> Option<Vec<u8>> {
    /// Through the item code, which is the shortest of these records seen.
    const SHORTEST_PURCHASE: usize = 72;
    /// Not negative, so `OnRecvDone` reaches the command instead of `OnError`.
    const GRANTED: u8 = 0;
    /// Header, status, and room behind it. The buy handler reads nothing there,
    /// but a reply that carries a little cannot come up short.
    const LENGTH: usize = 0x20;

    if request.len() < SHORTEST_PURCHASE || u16::from_le_bytes([request[0], request[1]]) as usize != request.len() {
        return None;
    }

    // A title's own commands are the even ones; the odd are what it is answered
    // with. Answering an odd command would be answering an answer.
    let command = u16::from_le_bytes([request[2], request[3]]);
    if command == 0 || command % 2 != 0 {
        return None;
    }

    let mut response = vec![0u8; LENGTH];
    response[0..2].copy_from_slice(&(LENGTH as u16).to_le_bytes());
    response[2..4].copy_from_slice(&(command + 1).to_le_bytes());
    response[4] = GRANTED;

    Some(response)
}
/// The rows 이노티아연대기's 캐쉬템 구매 screen is answered with.
///
/// A row is a name, how many one purchase grants, and its price. The name is
/// what carries the item: the handler matches it against the title's own table
/// of 545 item names and keeps the index that matched, and the index is what
/// the purchase then hands to the routine that puts an item in the bag. So
/// these are the title's own names, byte for byte as `inotia.bar` spells them,
/// and each one is unique in that table. A name it does not know would leave
/// the row's index at -1 and buy nothing.
///
/// Which of the 545 the service sold, and for how much, went with the service.
/// These are the ones that read as a cash shop's rather than a town shop's -
/// the blessed scrolls, the coupons, the keys and the styles - priced in the
/// hundreds of won those went for.
const INOTIA_SHOP_ROWS: [(&[u8], u8, u32); 12] = [
    // 축복받은 부활주문서
    (b"\xc3\xe0\xba\xb9\xb9\xde\xc0\xba \xba\xce\xc8\xb0\xc1\xd6\xb9\xae\xbc\xad", 1, 500),
    // 축복받은 용사의 인장
    (b"\xc3\xe0\xba\xb9\xb9\xde\xc0\xba \xbf\xeb\xbb\xe7\xc0\xc7 \xc0\xce\xc0\xe5", 1, 500),
    // 부활의 기도문
    (b"\xba\xce\xc8\xb0\xc0\xc7 \xb1\xe2\xb5\xb5\xb9\xae", 1, 300),
    // 창고확장 쿠폰(3칸)
    (b"\xc3\xa2\xb0\xed\xc8\xae\xc0\xe5 \xc4\xed\xc6\xf9(3\xc4\xad)", 1, 1000),
    // 스킬 초기화
    (b"\xbd\xba\xc5\xb3 \xc3\xca\xb1\xe2\xc8\xad", 1, 1000),
    // 자원 교환권
    (b"\xc0\xda\xbf\xf8 \xb1\xb3\xc8\xaf\xb1\xc7", 1, 500),
    // 행운의 열쇠
    (b"\xc7\xe0\xbf\xee\xc0\xc7 \xbf\xad\xbc\xe8", 1, 300),
    // 신비의 열쇠
    (b"\xbd\xc5\xba\xf1\xc0\xc7 \xbf\xad\xbc\xe8", 1, 500),
    // 흑기사의 투구
    (b"\xc8\xe6\xb1\xe2\xbb\xe7\xc0\xc7 \xc5\xf5\xb1\xb8", 1, 1000),
    // 레게 스타일
    (b"\xb7\xb9\xb0\xd4 \xbd\xba\xc5\xb8\xc0\xcf", 1, 800),
    // 번개 스타일
    (b"\xb9\xf8\xb0\xb3 \xbd\xba\xc5\xb8\xc0\xcf", 1, 800),
    // 스텔스 가면
    (b"\xbd\xba\xc5\xda\xbd\xba \xb0\xa1\xb8\xe9", 1, 800),
];

/// What answers the subscriber records 이노티아연대기 shops with.
///
/// 이노티아연대기 (`0001E718`) reads `PHONENUMBER`, takes a server out of its
/// own `etc.dat` - `어드벤쳐` at `211.115.66.232`, whose fourth port is 19017 -
/// and opens `MC_netBillSocket` to it the moment 캐쉬템 구매 is entered. The 16
/// bytes it writes there are the whole of the first request:
///
/// ```text
/// 00 10  1e  0b  30 31 30 35 35 39 33 30 39 30 36  00
/// ```
///
/// ```text
/// [0..2]  u16 BE - the record's own length, its own two bytes counted
/// [2]     the command
/// [3]     how many digits of subscriber number follow
/// [4..]   the subscriber number, and the page of the catalogue being asked for
/// ```
///
/// The title is compiled ahead of time, so what it does with the answer is ARM
/// rather than bytecode, and the framing is not the request's. The reader at
/// `0x3aff0` takes exactly two bytes, reads them big-endian through `0x7784`
/// (`(b[0] << 8) | b[1]`), keeps them as the head of the message and asks
/// `0x3af04` for `length - 2` more. So a reply is one length-prefixed record and
/// the prefix counts itself, the same way the request's does.
///
/// `0x38580` then dispatches it: the read cursor is seeked past the length, one
/// byte is taken as the command and one more as the status, and the command
/// indexes the table at `0x4be74`. Every handler there opens by reading that
/// status, and **1** is the only value any of them treat as success - `0x385de`,
/// the plainest of them, answers 0 with error 0x45, 2 with 0x4c and 3 with 0xdd.
///
/// `0x1e` is the catalogue, and its handler at `0x39540` reads:
///
/// ```text
/// [u8 pages][u8 page]  [u8 rows]  then rows x  [u8 name length][name][u8 count][u32 BE price]
/// ```
///
/// The first two are what the screen draws as `page + 1`/`pages` and what its
/// left and right arrows step through - `0x315d8` asks for `page - 1` while the
/// page is above zero, `0x315ec` for `page + 1` while it is below `pages - 1` -
/// so the page a reply declares is the page it was asked for, and one page
/// holding the whole catalogue is one to page through.
///
/// Each row's name is matched against the title's own item table, and the count
/// is a quantity: above one, `0x39628` appends `(N)` to the displayed name.
/// Choosing a row writes the matched index and that quantity aside (`0x3ec5a`)
/// and sends `0x1f`, whose reply is a status and nothing else - the handler at
/// `0x396b2` reads no further and puts the item in the bag itself.
///
/// `None` for anything that is not one of these two records: the declared length
/// has to be the record in hand, the command has to be one of the shop's, and
/// the fields behind it have to account for the rest of the record exactly.
pub fn lgt_local_subscriber_record_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length and the command, which is what the reader frames on.
    const HEADER: usize = 3;
    /// The one status every handler in the table reads as success.
    const GRANTED_STATUS: u8 = 1;
    /// The command the shop asks its catalogue for.
    const CATALOGUE_COMMAND: u8 = 0x1e;
    /// The command a chosen row is bought with.
    const PURCHASE_COMMAND: u8 = 0x1f;
    /// One page holds every row, so there is one page to step through.
    const PAGES: u8 = 1;

    if request.len() < HEADER + 2 || u16::from_be_bytes([request[0], request[1]]) as usize != request.len() {
        return None;
    }

    // Both records open with the subscriber number, length-prefixed.
    let digits = request[3] as usize;
    let subscriber = request.get(4..4 + digits)?;
    if digits == 0 || !subscriber.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let rest = &request[4 + digits..];

    let body = match request[2] {
        // The page asked for is the last byte, and there is nothing behind it.
        CATALOGUE_COMMAND => {
            let [page] = *rest else { return None };
            if page >= PAGES {
                return None;
            }

            let mut body = vec![PAGES, page, INOTIA_SHOP_ROWS.len() as u8];
            for (name, count, price) in INOTIA_SHOP_ROWS {
                body.push(name.len() as u8);
                body.extend_from_slice(name);
                body.push(count);
                body.extend_from_slice(&price.to_be_bytes());
            }

            body
        }
        // The row's own name, length-prefixed as the subscriber number was,
        // and the quantity behind it.
        PURCHASE_COMMAND => {
            let name_length = *rest.first()? as usize;
            if rest.len() != name_length + 2 {
                return None;
            }

            Vec::new()
        }
        _ => return None,
    };

    let length = HEADER + 1 + body.len();
    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(&(length as u16).to_be_bytes());
    response.push(request[2]);
    response.push(GRANTED_STATUS);
    response.extend_from_slice(&body);

    Some(response)
}

/// The rows 이노티아연대기2's 캐쉬템 구매 screen is answered with, by the tab
/// each belongs to.
///
/// A row is an item, how many one purchase grants, and its price. The item is
/// the index the title's own table gives it - the shop draws a row by handing
/// that index to `0x71e8`, which reads the name out of `game.dat`'s 970-entry
/// item table (block 12, stride 17) and resolves it through `memorytext.dat` -
/// so these are the title's own items rather than names invented here.
///
/// The tabs are `game.dat`'s block 80, seven rows whose codes are 1 to 7:
/// 싱글 플레이용, 강화, 조합용, 공성전용, one the text table leaves unnamed,
/// 용병 and 이벤트. Which items the service sold under each, and for how much,
/// went with the service; these are the ones from the title's own table that
/// belong under the tab they are filed here.
/// One row of a shop tab: the item, how many one purchase grants, its price.
type InotiaShopRow = (u32, u8, u32);

const INOTIA_2_SHOP_TABS: [(u8, &[InotiaShopRow]); 7] = [
    // 싱글 플레이용
    (
        1,
        &[
            (7, 5, 500),    // 회복약(대)
            (11, 5, 500),   // 마나물약(대)
            (17, 3, 500),   // 원기회복의 물약
            (347, 1, 500),  // 부활주문서
            (647, 1, 1000), // 축복받은 부활주문서
            (969, 1, 1000), // 빠른 성장의 물약
            (4, 1, 2000),   // 무한의 가방
        ],
    ),
    // 강화
    (
        2,
        &[
            (23, 1, 1000),  // 무기강화 주문서
            (24, 1, 1000),  // 방어구강화 주문서
            (27, 3, 500),   // 상급 에테르
            (28, 1, 1000),  // 최상급 에테르
            (29, 1, 2000),  // 혼돈의 에테르
            (935, 1, 3000), // 강화세트
        ],
    ),
    // 조합용
    (
        3,
        &[
            (32, 1, 1000),  // 조합대전집
            (364, 5, 500),  // 마법의 양피지
            (361, 5, 500),  // 마력의 결정
            (362, 3, 1000), // 고동치는 결정
            (369, 3, 1000), // 오리하르콘 조각
            (932, 1, 2000), // 오색 수정
        ],
    ),
    // 공성전용
    (
        4,
        &[
            (931, 1, 1000), // 공성전 명령서
            (936, 1, 3000), // 공성전세트
            (871, 5, 500),  // 확성기
            (872, 3, 1000), // 길드 확성기
        ],
    ),
    // The tab the title's own text table leaves unnamed.
    (
        5,
        &[
            (873, 1, 1000), // 창고 쿠폰
            (874, 1, 2000), // 길드 창고 쿠폰
            (870, 1, 500),  // 채팅 이용권
            (868, 1, 500),  // 광산 출입증
            (869, 1, 2000), // 광산 개발권
        ],
    ),
    // 용병
    (
        6,
        &[
            (30, 1, 1000),  // 용사의 인장
            (31, 1, 500),   // 견습용 용사의 인장
            (943, 1, 1000), // 초월의 영약(힘)
            (944, 1, 1000), // 초월의 영약(민첩)
            (945, 1, 1000), // 초월의 영약(체력)
            (946, 1, 1000), // 초월의 영약(지능)
            (947, 1, 1000), // 초월의 영약(정신)
        ],
    ),
    // 이벤트
    (
        7,
        &[
            (933, 1, 3000), // 카오스세트
            (934, 1, 5000), // 에픽카오스세트
            (880, 1, 500),  // 부활의 기도문
            (949, 1, 1000), // 신수의 물약
            (968, 3, 500),  // 신성한 가루
            (879, 5, 300),  // 장난감 폭탄
        ],
    ),
];

/// What answers the command records 이노티아연대기2 opens a session with.
///
/// 이노티아연대기2 (`0002BA13`) connects to `211.115.66.232:20009` - the same
/// host the first game's shop used, on a different port - the moment its shop
/// is entered, and sits on 처리 중 until something answers. `0x116a4` is that
/// connect, and `0x10edc` is what it runs once the socket is up: the 18 bytes
/// the capture shows going out are the whole of the first request.
///
/// ```text
/// 00 10  00 00  00 02  30 31 30 34 36 31 31 39 32 36 39 00
/// ```
///
/// ```text
/// [0..2]  u16 BE - the body's length, which does NOT count these two bytes
/// [2..4]  u16 BE - the command
/// [4..6]  u16 BE - the tag the session carries from step to step
/// [6..]   the subscriber number, twelve bytes with its own NUL
/// ```
///
/// The length is written last, over the two bytes the writer reserved: `0x10b64`
/// sets the length to the cursor **minus two** and rewinds to write it there. A
/// reply is framed the same way - `0x10f48` reads two bytes, `0x5346c` reads them
/// through `MC_utilNtohs`, and `0x112f4` reads exactly that many more.
///
/// `0x11920` then dispatches the body: `0x5346c` again for the command, which is
/// compared against the command last sent (`0x10c88` keeps it) to dismiss the
/// 처리 중 dialog, and then switched on. Each handler reads its own fields off
/// the same cursor, big-endian throughout, with a string being a `u16` length
/// and that many bytes.
///
/// Three commands make up what the shop asks for, and each handler is what
/// shapes its answer:
///
/// - `0x0000`, the hello `0x10edc` sends with the subscriber number, whose
///   handler at `0x1184c` reads a `u16` code, a `u8` and a string. The `u8` has
///   to be nonzero or the connection is dropped; the code becomes the tag the
///   next request carries; and the string is a message to show the player, so an
///   empty one is what lets `0x118c8` run the next step instead of stopping on a
///   dialog.
/// - `0x014a`, whose handler at `0x1180c` reads a `u16` it discards and a `u8`
///   that has to be exactly 1. That one hands the session to the screen that
///   opened it. The title sends this one itself only when nothing else has
///   claimed the step behind the hello.
/// - `0x010f`, the shop's own list, which `0x32abc` sends as three bytes - the
///   tab's code, a first row and how many rows are wanted. Its command is not
///   one the network layer knows, so `0x11940` reads the tag and hands the rest
///   to the screen at `0x32fe0`, whose `0x3304a` reads two bytes it discards and
///   then a `u8` row count. Each row behind that is:
///
/// ```text
/// [u32 item][u8 length][a string the reader drops][u8 count][u32 price][u8 length][description]
/// ```
///
/// The item is an index into the title's own table, which is what the row is
/// drawn from: `0x331f6` reads its icon out of that table, `0x71e8` reads its
/// name, and above a count of one `0x33238` renders the two as `%s(%d)`. So the
/// rows carry items the title already knows rather than anything named here.
/// - `0x010e`, the buy, which `0x32af2` sends as the subscriber number, a fixed
///   `9`, the row's name as the title spells it and a `u16` quantity. Its
///   handler at `0x3301c` reads one byte and does not look at it: this protocol
///   reports a refusal by answering `0x0000` with a message instead, so a reply
///   under the command that was asked is the grant. The screen then sends
///   `0x0112` behind it, in the same shape, and reads nothing back at all.
///
/// The tag is the server's to choose, so it comes back as it was sent - the
/// title picked `2` for the hello itself, and echoing keeps the session on it.
///
/// `None` for anything that is not one of these records: the length has to
/// describe the body in hand, the command has to be one of these three, and what
/// follows has to be the payload that command carries.
pub fn lgt_local_command_tag_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length field, which the length it holds does not count.
    const LENGTH_FIELD: usize = 2;
    /// A command and the tag behind it.
    const BODY_HEADER: usize = 4;
    /// The subscriber number, written as a fixed twelve bytes.
    const SUBSCRIBER: usize = 12;
    /// A category, a first row, and how many rows are wanted.
    const LIST_ARGUMENTS: usize = 3;
    /// The status both handshake handlers read as success.
    const GRANTED_STATUS: u8 = 1;
    /// The command a session opens with.
    const HELLO_COMMAND: u16 = 0x0000;
    /// The command that follows once the hello is granted.
    const SESSION_COMMAND: u16 = 0x014a;
    /// The command the shop asks its list for.
    const LIST_COMMAND: u16 = 0x010f;
    /// The command a chosen row is bought with.
    const BUY_COMMAND: u16 = 0x010e;
    /// The command the screen sends once the buy is granted.
    const COMMIT_COMMAND: u16 = 0x0112;

    if request.len() < LENGTH_FIELD + BODY_HEADER {
        return None;
    }
    if u16::from_be_bytes([request[0], request[1]]) as usize != request.len() - LENGTH_FIELD {
        return None;
    }

    let command = u16::from_be_bytes([request[2], request[3]]);
    let tag = [request[4], request[5]];
    let payload = &request[LENGTH_FIELD + BODY_HEADER..];

    // Every request but the list opens with the subscriber number and the NUL
    // it is written with.
    let subscriber_first = |field: &[u8]| match field.iter().position(|&byte| byte == 0) {
        Some(0) | None => false,
        Some(digits) => field[..digits].iter().all(u8::is_ascii_digit),
    };

    // The handshake steps carry the subscriber number and nothing else; the
    // list carries its three arguments; a purchase carries the subscriber
    // number, a byte, the row's name and the quantity.
    let carries_subscriber = || payload.len() == SUBSCRIBER && subscriber_first(payload);
    let buys_a_row = || {
        let Some((subscriber, rest)) = payload.split_at_checked(SUBSCRIBER) else {
            return false;
        };
        let Some(&name_length) = rest.get(1) else {
            return false;
        };
        subscriber_first(subscriber) && rest.len() == 2 + name_length as usize + 2
    };

    let mut body = Vec::from(command.to_be_bytes());
    match command {
        // The code the next request carries, the status, and an empty message.
        HELLO_COMMAND if carries_subscriber() => {
            body.extend_from_slice(&tag);
            body.push(GRANTED_STATUS);
            body.extend_from_slice(&0u16.to_be_bytes());
        }
        // A word the handler reads past, and the status.
        SESSION_COMMAND if carries_subscriber() => {
            body.extend_from_slice(&tag);
            body.push(GRANTED_STATUS);
        }
        // Two bytes the screen reads past, then the tab's rows from the one
        // asked for, as many as were asked for.
        LIST_COMMAND if payload.len() == LIST_ARGUMENTS => {
            let (_, rows) = INOTIA_2_SHOP_TABS.iter().find(|(tab, _)| *tab == payload[0])?;
            let rows = rows.get(payload[1] as usize..).unwrap_or_default();
            let rows = &rows[..rows.len().min(payload[2] as usize)];

            body.extend_from_slice(&tag);
            body.extend_from_slice(&[0, 0]);
            body.push(rows.len() as u8);
            for (item, count, price) in rows {
                body.extend_from_slice(&item.to_be_bytes());
                // The string the reader drops, and the description, both empty:
                // the row is drawn from the title's own item table either way.
                body.push(0);
                body.push(*count);
                body.extend_from_slice(&price.to_be_bytes());
                body.push(0);
            }
        }
        // Nothing the buy handler at `0x3301c` reads past its own byte, and
        // nothing at all for the commit behind it - the screen has already
        // moved on to its own message by then.
        BUY_COMMAND | COMMIT_COMMAND if buys_a_row() => {
            body.extend_from_slice(&tag);
            body.push(GRANTED_STATUS);
        }
        _ => return None,
    }

    let mut response = Vec::with_capacity(LENGTH_FIELD + body.len());
    response.extend_from_slice(&(body.len() as u16).to_be_bytes());
    response.extend_from_slice(&body);

    Some(response)
}

/// The rows 라그나로크 바이올렛's 럭셔리샵 is answered with.
///
/// A row is an item, how many one purchase grants, and its price. The item is
/// the index the title's own table gives it: `0x1aff0` reads its icon out of
/// the record table at `0x1407fc0`, stride `0x34`, and `0x1b07c` reads its name
/// out of the fixed 17-byte names at `0x140ed70` - 540 of them, both tables in
/// the module's own `.data`. So these are the title's own items rather than
/// names invented here, and the last twenty of that table are what a shop
/// called 럭셔리샵 sold: the bags, the springs, the pet eggs and hats, the
/// wallpapers and the pouches.
///
/// What each went for went with the service; these are priced in the round
/// hundreds a cash shop's were.
const RAGNAROK_VIOLET_SHOP_ROWS: [(u32, u32, u32); 20] = [
    (521, 1, 1000), // 여행자 가방
    (524, 1, 2000), // 튼튼한 가방
    (522, 1, 500),  // 마력의 샘물
    (523, 1, 500),  // 신비의 샘물
    (520, 1, 1000), // 비너스의 눈물
    (525, 1, 1500), // 오리하르콘
    (526, 1, 1000), // 대장장이의 손
    (527, 1, 2000), // 까만 펫 알
    (528, 1, 2000), // 녹색 펫 알
    (529, 1, 2000), // 푸른 펫 알
    (533, 1, 500),  // 밥그릇
    (534, 1, 800),  // 모형칼 펫모자
    (535, 1, 800),  // 밀짚 펫모자
    (536, 1, 800),  // 천사하트 펫모자
    (537, 1, 800),  // 바람개비 펫모자
    (530, 1, 500),  // 태양 벽지
    (531, 1, 500),  // 해변 벽지
    (532, 1, 500),  // 노을 벽지
    (538, 1, 1000), // 재료주머니
    (539, 1, 1500), // 카드북
];

/// What answers the fixed blocks 라그나로크 바이올렛 opens its shop with.
///
/// 라그나로크 바이올렛 (`000256A7`) opens a billing socket to port 9000 when its
/// shop is entered and writes one 1024-byte block, zero-padded past what it
/// says:
///
/// ```text
/// 00 00 00 66  00 00 00 c9  00 00 00 33  00 00 00 00  00 00 00 04
/// 00 00 00 0a "1046119269"  00 00 00 08 "Emulator"  00 00 00 09 "ver 1.0.2"
/// 00 00 00 04  00 00 00 02  00 ... 00
/// ```
///
/// ```text
/// [0..4]   u32 BE - the screen, one of 0x65..=0x75
/// [4..8]   u32 BE - the step
/// [8..12]  u32 BE - how much of the block past [16] is meant
/// [12..16] u32 BE - carried back and forth, and part of what marks a repeat
/// [16..20] u32 BE - the step's own word
/// [20..]   the step's body, strings written as a u32 length and that many bytes
/// ```
///
/// The title is compiled ahead of time, so what it does with the answer is ARM
/// rather than bytecode. `0x39528` reads up to 1024 bytes into one buffer and
/// `0x37fa4` appends them to an accumulator, and `0x38e78` looks at that
/// accumulator only once it holds **more than 1023 bytes** - so a reply is one
/// 1024-byte block, the same way the request is.
///
/// `0x38e78` then reads the five words above, drops the block if the screen, the
/// step and `[12]` all repeat what came last, and indexes the table at `0x3ed90`
/// by the screen. `0x3931c` is where `0x66` lands, and it has a case for the two
/// steps this answers:
///
/// - `0xca`, for the `0xc9` the screen opens with, and this is the shop's list:
///   `0x381f4`'s case for `0x66` at `0x38404` takes `[16]` as a row count and
///   reads that many rows of three words - the item, how many one purchase
///   grants, and its price - into the three arrays `0x1aff0`, `0x1b036` and
///   `0x1b0c0` draw a row from. The screen sends `0xcb` next, which `0x38b44`
///   builds with nothing of its own - a zero length over a body the send buffer
///   still holds from the `0xc9`.
/// - `0xcd`, for that `0xcb`. `0x37fec` copies `[8]` bytes from the block's
///   offset 20 into `0x150de98` and NUL-terminates them, and that is a message
///   the screen draws before acknowledging with `0xce` and moving its own state
///   on. The message was the service's to write, so it is answered as none - a
///   zero length is what lets the screen move on rather than wait.
///
/// Buying a row opens `0x6b` the same way, and `0x3943a` is its handler. Its
/// `0xc9` carries nothing - `0x38dd2` writes a body only for `0xcb` - and its
/// `0xca` is answered with nothing back; the screen then sends `0xcb` with the
/// row it chose. `0x38466` reads the result of that off the body rather than
/// the step's word, and of the four codes it knows only **301** reaches
/// `0x384a6`, which grants the item before showing its message.
///
/// `None` for anything that is not one of these blocks: the block has to be the
/// full 1024, the screen and step have to be a pair this answers, and each has
/// to carry what that step carries - the subscriber number written as a length
/// and its digits for the shop's hello, nothing of its own for the rest.
pub fn lgt_local_fixed_block_response(request: &[u8]) -> Option<Vec<u8>> {
    /// What the title reads and writes a block as, padding included.
    const BLOCK: usize = 1024;
    /// The five words in front of a step's body.
    const HEADER: usize = 20;
    /// Where `[8]` is measured from.
    const LENGTH_FROM: usize = 16;
    /// The screen the list is asked under, and the one a purchase is.
    const SHOP_SCREEN: u32 = 0x66;
    const PURCHASE_SCREEN: u32 = 0x6b;
    /// The step a screen opens with, and the one that answers it.
    const HELLO_STEP: u32 = 0xc9;
    const GRANTED_STEP: u32 = 0xca;
    /// The step a screen sends behind that, and the one that answers it.
    const SECOND_STEP: u32 = 0xcb;
    const RESULT_STEP: u32 = 0xcd;
    /// A row: the item, the quantity, the price.
    const ROW: usize = 12;
    /// What the list answer holds: the row count, and then the rows.
    const ROWS: u32 = (4 + RAGNAROK_VIOLET_SHOP_ROWS.len() * ROW) as u32;
    /// The one code `0x384a6` grants a purchase on.
    const GRANTED_RESULT: u32 = 301;

    if request.len() != BLOCK {
        return None;
    }

    let word = |at: usize| u32::from_be_bytes([request[at], request[at + 1], request[at + 2], request[at + 3]]);

    let screen = word(0);

    // What the block says of itself has to fit in the block.
    let length = word(8) as usize;
    if length > BLOCK - LENGTH_FROM {
        return None;
    }

    // The shop's hello opens with the subscriber number, written as a length
    // and that many digits; every other step here carries nothing of its own.
    let opens_with_subscriber = || {
        let digits = word(HEADER) as usize;
        match request.get(HEADER + 4..HEADER + 4 + digits) {
            Some(subscriber) => digits > 0 && subscriber.iter().all(u8::is_ascii_digit),
            None => false,
        }
    };

    let (step, answer) = match (screen, word(4)) {
        (SHOP_SCREEN, HELLO_STEP) if length >= 4 && opens_with_subscriber() => (GRANTED_STEP, ROWS),
        (SHOP_SCREEN, SECOND_STEP) if length == 0 => (RESULT_STEP, 0),
        (PURCHASE_SCREEN, HELLO_STEP) if length == 0 => (GRANTED_STEP, 0),
        // The purchase's own result, which is a word behind the step's word.
        (PURCHASE_SCREEN, SECOND_STEP) => (RESULT_STEP, 8),
        _ => return None,
    };

    let mut response = vec![0u8; BLOCK];
    response[0..4].copy_from_slice(&screen.to_be_bytes());
    response[4..8].copy_from_slice(&step.to_be_bytes());
    // What the answer says of itself, measured from [16] the way the request's
    // own length is: the count and its rows for the list, nothing for the
    // message.
    response[8..12].copy_from_slice(&answer.to_be_bytes());

    match (screen, step) {
        (SHOP_SCREEN, GRANTED_STEP) => {
            response[HEADER - 4..HEADER].copy_from_slice(&(RAGNAROK_VIOLET_SHOP_ROWS.len() as u32).to_be_bytes());
            for (index, (item, count, price)) in RAGNAROK_VIOLET_SHOP_ROWS.iter().enumerate() {
                let at = HEADER + index * ROW;
                response[at..at + 4].copy_from_slice(&item.to_be_bytes());
                response[at + 4..at + 8].copy_from_slice(&count.to_be_bytes());
                response[at + 8..at + 12].copy_from_slice(&price.to_be_bytes());
            }
        }
        // `0x38466` reads the result off the body rather than the step's word.
        (PURCHASE_SCREEN, RESULT_STEP) => response[HEADER..HEADER + 4].copy_from_slice(&GRANTED_RESULT.to_be_bytes()),
        _ => {}
    }

    Some(response)
}

/// What answers the `ENSLGT` record 블레이드마스터4 buys 하트 with.
///
/// 블레이드마스터4 (`0002BA50`) opens a billing socket to port 5018 when a
/// 하트 purchase is confirmed and writes one 67-byte record:
///
/// ```text
/// "ENSLGT" 11 79 00 31 00 36 00 "01046119269" 00 x9 "Emulator" 00 00
/// "100" "0002BA50004" 00 x5 54 0b 00 00
/// ```
///
/// `0x5fbbc` builds it, and the pieces are its own: `"ENS"` and `"LGT"` written
/// three bytes each, a `u8` and then little-endian `u16`s through `0x45134` and
/// `0x45144`, the subscriber number as a fixed 21 bytes, the handset as ten,
/// `"100"`, and the product code and its price as one twenty-byte record -
/// `"0002BA50004"`, one of four the module carries, and `0x0b54` for the 2900원
/// the screen names.
///
/// The `u16` at `[9]` is what the exchange is: `0x5fc6c` keeps it at
/// `0x150bb70+0x14`, and `0x618d2` switches a reply on the same field. A 하트
/// purchase walks `0x31` to `0x34`, each granted step building the next.
///
/// A reply is framed by its own magic rather than by any length: `0x61854` walks
/// the bytes received looking for `E`, `N`, `S`, and reads from there a `u16`
/// command, a `u16` length it waits on until that many bytes have arrived, and
/// then `0x60cf8` takes a `u16` result and keeps it at `0x150bb70+0x1a`. Zero is
/// the only value that is not one of the errors it names, and every handler
/// `0x618e4` picks for these exchanges turns on exactly that:
///
/// - `0x4e508` for `0x31` returns 2 on a nonzero result, which reaches the
///   failure at `0x619a6`; on zero it runs `0x4e4d4`, which builds the step
///   behind it.
/// - `0x4e524` for `0x32` and `0x34` hands `0x150bb70+0x60` to `0x1ad0c` when
///   the result is zero and skips it otherwise.
/// - `0x4e544` for `0x33` reads one more `u32` behind the result, so that is the
///   one exchange whose answer carries a body past it.
///
/// 드래곤하트2 (`0002CC04`) writes the same record on starting up, while it says
/// 기존에 저장되어 있는 데이터가 있는지 확인중입니다 - 48 bytes, version `0x7b`,
/// exchange `0x26`:
///
/// ```text
/// "ENSLGT" 11 7b 00 26 00 23 00 "01083062925" 00 x9 "Emulator" 00 00 "103" 01
/// ```
///
/// Its own `binary.mod` frames a reply exactly the way 블레이드마스터4 does:
/// `0x24448` counts the bytes in, checks the first three against `"ENS"` once
/// seven have arrived, reads a little-endian `u16` command and a `u16` length,
/// and then waits for that many more before handing the body - which sits at
/// `0x1554bdc + 0xa` and is walked by a cursor at `0x1554bdc` through readers
/// for `s8`, `s16`, `u32` and a buffer - to the exchange's own handler.
///
/// So this answers any of these records rather than one title's walk: the `u16`
/// at `[11]` is what the record says it carries past the header, which is the
/// check that tells one of these apart from anything else opening with the
/// magic. Only 블레이드마스터4's `0x33` is known to read past the result, so only
/// that one, at that version, gets a word behind it.
///
/// `None` for anything that is not one of these records: it has to open with the
/// magic, carry the byte the builder fixes, say its own length where the builder
/// puts it, and have the subscriber number in digits behind that.
pub fn lgt_local_ens_record_response(request: &[u8]) -> Option<Vec<u8>> {
    /// What the reply is framed by, and what the request opens with.
    const REPLY_TAG: &[u8] = b"ENS";
    const REQUEST_TAG: &[u8] = b"ENSLGT";
    /// The byte the builder fixes at `[6]`.
    const MARK: u8 = 0x11;
    /// Where the subscriber number is written, as a fixed twenty-one bytes -
    /// and what `[11..13]` counts from, so it is also the header's own length.
    const SUBSCRIBER: usize = 13;
    /// 블레이드마스터4's version, and the one exchange of its walk whose handler
    /// reads a word behind the result.
    const BLADEMASTER4: u16 = 0x79;
    const WORD_EXCHANGE: u16 = 0x33;
    const GRANTED_RESULT: u16 = 0;

    if !request.starts_with(REQUEST_TAG) || request.len() < SUBSCRIBER + 2 {
        return None;
    }

    let word = |at: usize| u16::from_le_bytes([request[at], request[at + 1]]);

    // The record says its own length behind the header, which is what tells one
    // of these apart from anything else that happens to open with the magic.
    if request[6] != MARK || word(11) as usize != request.len() - SUBSCRIBER {
        return None;
    }

    // The subscriber number, NUL-padded to twenty-one bytes.
    let digits = request[SUBSCRIBER..].iter().position(|&byte| byte == 0)?;
    if digits == 0 || !request[SUBSCRIBER..SUBSCRIBER + digits].iter().all(u8::is_ascii_digit) {
        return None;
    }

    let (version, exchange) = (word(7), word(9));

    // The result, and for the one exchange this has read a handler for that
    // reads further, the word behind it.
    let mut body = Vec::from(GRANTED_RESULT.to_le_bytes());
    if (version, exchange) == (BLADEMASTER4, WORD_EXCHANGE) {
        body.extend_from_slice(&0u32.to_le_bytes());
    }

    let mut response = Vec::from(REPLY_TAG);
    response.extend_from_slice(&exchange.to_le_bytes());
    response.extend_from_slice(&(body.len() as u16).to_le_bytes());
    response.extend_from_slice(&body);

    Some(response)
}

/// What answers the big-endian record 레전드오브마스터 sends its purchases in.
///
/// 레전드오브마스터 (`0002A4B1`) opens `BillSocket://211.189.18.116:9407` and
/// writes a 55-byte record. Buying a 최상급강화석 for 500원 writes:
///
/// ```text
/// 00 37  08 36  00 ... 00  64  00 ... 00  12  01 f4  <item, EUC-KR>  00 ... 00  c8 d1
/// ```
///
/// ```text
/// [0..2]   u16 BE - the record's own length
/// [2..4]   u16 BE - the command
/// [4..53]  the body: the item, its price in won, and the counters around them
/// [53..55] a checksum
/// ```
///
/// The title is compiled ahead of time, so what it does with the answer is ARM
/// rather than bytecode. Its network thread's `run` is a state machine over one
/// field, and the read state at `0xf3e68` is the whole of the reply's shape:
///
/// - `read(header, 0, 4)`, then `getShort(header, 0)` as the length - which has
///   to be above zero - and `getShort(header, 2)` as the command, which has to
///   be **above 1000** or the thread drops the connection. `getShort` at
///   `0xe26a8` is `(buf[off] << 8) | buf[off + 1]`, so both are big-endian.
/// - `read(body, 0, length - 6)` when that is positive, kept as the reply body.
/// - `read(header, 0, 4)` once more - a four-byte tail it reads past and never
///   looks at.
///
/// Which is not the shape of its own requests: the 55 it writes are four of
/// header, 49 of body and two of checksum, so the length counts six of overhead
/// either way but the tail it reads is twice the tail it writes. The answer is
/// built for the reader rather than mirrored off the writer.
///
/// The dispatcher at `0x5b79c` then zeroes the body's read cursor and switches
/// on the command. `0x0837` - the request's own command plus one - reaches
/// `0x63d04`, which takes **one signed byte** off the body and treats `0` and
/// `6` as granted; anything else raises the flag the 통신장애 notice is drawn
/// from. Nothing else in that handler reads the body.
///
/// So the answer is the command plus one and a zero status byte. The body is
/// padded past the one byte this command reads because the cursor is shared
/// with every other command's handler, and a body only as long as its shortest
/// reader would put a longer one out of bounds.
///
/// 레전드오브마스터2 (`000308FF`) reaches the same `BILL_GW_IP`,
/// `211.189.18.116`, and opening its shop writes a record of the same shape
/// under a command of its own:
///
/// ```text
/// 00 2d  03 e8  00 ... 00  64 00 00 00  01 00 00 00  00 ... 00
/// ```
///
/// Forty-five bytes that declare forty-five, and `0x03e8` - one thousand
/// exactly. Which went unanswered while the bound below was read off the
/// request rather than the answer: the thread drops a reply at or under a
/// thousand, and a reply is the request's command plus one, so a request of
/// exactly a thousand is answered as `0x03e9` and dispatches. Only a request
/// below that can no longer be answered without costing the title its socket.
///
/// `None` for anything that is not one of these records: the declared length has
/// to be the record in hand, and the command has to be one a title sends - the
/// even ones - rather than one it is sent.
pub fn lgt_local_big_endian_record_response(request: &[u8]) -> Option<Vec<u8>> {
    /// A length and a command, which is what the title reads before anything
    /// else.
    const HEADER: usize = 4;
    /// Read past and discarded, but it has to be there to be read past.
    const TAIL: usize = 4;
    /// The length field counts the header and the tail as six between them.
    const LENGTH_OVERHEAD: usize = 6;
    /// Room behind the status byte, for the handlers that read further.
    const BODY: usize = 0x20;
    /// Which the purchase handler spells "granted".
    const GRANTED: u8 = 0;
    /// Below this the title drops the connection rather than dispatching.
    const LEAST_COMMAND: u16 = 1000;

    // A frame is a length and a command, and the body is optional: 레전드오브
    // 마스터2's second request is `00 04 04 b0` and nothing else, which its own
    // builder at `0x3a2a8` writes as body-plus-four. Holding out for six
    // dropped it on the floor, unanswered.
    if request.len() < HEADER || u16::from_be_bytes([request[0], request[1]]) as usize != request.len() {
        return None;
    }

    // A title's own commands are the even ones; the odd are what it is answered
    // with. Answering an odd command would be answering an answer.
    let command = u16::from_be_bytes([request[2], request[3]]);
    // The bound belongs to the answer the title reads, which is this plus one -
    // a request of exactly the bound is answered above it and dispatches.
    if command + 1 <= LEAST_COMMAND || command % 2 != 0 {
        return None;
    }

    // 레전드오브마스터2 is answered at the request plus two.
    //
    // Its dispatcher is a comparison chain at `0x25758`, and every command in
    // it ends in `02` or `04` where a request ends in `00`. 1001 is not in it
    // at all, which is what `엉뚱한 패킷날라옴 / 인덱스:1001` was saying, and
    // 1002 is, three comparisons in:
    //
    // ```text
    // 0x2579a  subs r3, #0x64      ; 0x44e - 0x64 = 0x3ea = 1002
    // 0x2579c  cmp  r2, r3
    // 0x2579e  bne  0x257a2
    // 0x257a0  b    0x25b10
    // ```
    //
    // `0x25b10` writes 14 to `0x1501dac` - the screen that draws `[아이템샵]`
    // and its list, and the only place in the binary that writes it. Its guard
    // is `r1 == 0`, and the caller at `0x24f3e` leaves r1 holding the mode byte
    // from `+0x835`, which the shop's request carries as zero.
    //
    // Answered that way it asks the next thing, 1200, and 1202 is the arm that
    // takes a count off the reply, allocates four bytes an entry and fills the
    // list (`0x27270`-`0x27282`). So the exchange walks: the round hundreds are
    // what this title asks under, and two past them is where it is answered.
    //
    // 영웅서기5 shares this gateway but not that shape - it asks under `0x0836`
    // and reads its reply at `0x0837` - so the rule is drawn where the two
    // differ rather than off a list of commands seen so far.
    const LOM2_REQUEST_STEP: u16 = 100;

    let answer = if command % LOM2_REQUEST_STEP == 0 { command + 2 } else { command + 1 };

    let mut response = vec![0u8; HEADER + BODY + TAIL];
    response[0..2].copy_from_slice(&((BODY + LENGTH_OVERHEAD) as u16).to_be_bytes());
    response[2..4].copy_from_slice(&answer.to_be_bytes());
    response[HEADER] = GRANTED;

    Some(response)
}
/// What answers the length-prefixed command 영웅서기4 opens its online menu with.
///
/// 영웅서기4 (`0002D74B`) reaches `210.222.18.31:8894` through
/// `MC_netBillSocket` - the carrier's socket carries a game service here rather
/// than a purchase - and speaks a frame of its own:
///
/// ```text
/// [0..4]  u32 LE - the frame's own length, this field included
/// [4]     u8     - the major command
/// [5]     u8     - the minor command
/// [6..]   the body
/// ```
///
/// Opening 상점 writes `07 00 00 00 01 01 04`, and five seconds later
/// `06 00 00 00 00 0a` - which the title names itself, through
/// `MC_knlPrintk`: `[SEND PROTOCL] MAJOR_SYSTEM_MESSAGE / MINOR_KEEP_ALIVE_MSG`.
///
/// The title is native, so its receive path is ARM. The callback at `0x572c8`
/// queues whatever arrives, and the dispatcher at `0x61518` drops anything under
/// six bytes, reads the major at `[4]` and the minor at `[5]`, and switches on
/// the major - `1`, `5`, `0x14` and `0x64` are handled and everything else is
/// dropped without a word, the title's own major `0` keep-alive included.
///
/// Which is what the online menu's login is, read out of those handlers:
///
/// | the title sends | the handler | what it does next |
/// |-----------------|-------------|-------------------|
/// | `1/0x01`        | `0x612d0`   | reads nothing of the reply; answers with its `PHONENUMBER` as `1/0x3d` |
/// | `1/0x3d`        | `0x6137a`   | reads nothing of the reply; answers `1/0x3e` |
/// | `1/0x3e`        | `0x613b8`   | reads nothing of the reply while the 상점 flag is set; closes the 서버 응답을 기다리는중 notice and asks for the catalogue as `5/0x3f` |
/// | `5/0x3f`        | `0x5eb1e`   | takes a `u16 LE` count at `[8]` and that many 37-byte rows behind it, then opens the shop screen |
///
/// Buying from that screen is two more, and both read a status byte at `[6]`
/// and a NUL-terminated message at `[8]` - the pattern every `major 5` handler
/// shares, where `1` is the only status that is not an error box:
///
/// | the title sends | the handler | what it does next |
/// |-----------------|-------------|-------------------|
/// | `5/0x42`        | `0x5e9b2`   | the charge, carrying the row's handle and price. On `1` it asks for the item as `5/0x40`; on anything else it draws the message and stops |
/// | `5/0x40`        | `0x5e882`   | the delivery. On `1` it reads `[7]` as an offset and takes the byte at `[8 + offset]`: `0xff` puts the row's own item in the bag and returns to the shop, and anything else is an index into `/ITM/DAT/_ITM_CASH_RANOMBOX` (`0x2544c`) |
///
/// The 창고 is the same menu under its other label, and three more of the same
/// shape - see [`hero4_warehouse`] for how its listing is the catalogue's:
///
/// | the title sends | the handler | what it does next |
/// |-----------------|-------------|-------------------|
/// | `0x14/0x46`     | `0x5e4f0`   | the character, 1138 bytes of it. Reads nothing of the reply; closes the notice and asks for the listing as `5/0x3d` |
/// | `5/0x3d`        | `0x5ea26`   | takes a byte at `[6]`, then the catalogue's own header and 37-byte rows one further along, and asks `5/0x14` |
/// | `5/0x14`        | `0x5eb04`   | takes four bytes at `[6]` into `0x1566d70`, which nothing reads back |
/// | `5/0x41`        | `0x5ea10`   | the deposit, carrying the item as 36 bytes. Reads `[6]` alone: zero draws the error box at `0x5ec80` and the item stays in the bag, anything else runs the move at `0x5e658` |
/// | `5/0x04`        | `0x5ec20`   | the withdrawal, naming the row by the first 16 bytes of its record. Reads nothing of the reply; builds the item out of its own tables and asks for the listing again |
///
/// `5/0x03` follows a deposit carrying the same record, and is not answered
/// because it cannot be: the title's own dispatcher takes `minor - 4`, so a
/// `5/0x03` reply lands under `0x5e868`'s table and is dropped unread.
///
/// So the first three are answered with the command alone - the title only needs
/// to see its own command come back to take the next step - the catalogue is
/// answered with the sixteen items [`hero4_catalogue`] lays out, the 창고 with
/// what [`hero4_deposit`] has been handed and [`hero4_withdraw`] has not taken
/// back, and the two halves of a purchase are granted.
///
/// What a purchase delivers is decided by [`hero4_delivery`]: the four box rows
/// by a draw over the box table, and the other twelve by the `0xff` that hands
/// over the row's own item. Both messages are left empty, because there is no
/// server here to have written one and the granted path draws its own notice
/// rather than the reply's.
///
/// `None` for anything else, the keep-alive included: the frame has to declare
/// its own length, and the command pair has to be one of the four whose answer
/// is known. A command answered wrongly here does not stall the title - it puts
/// it through a branch meant for a different exchange.
pub fn lgt_local_major_minor_response(request: &[u8]) -> Option<Vec<u8>> {
    /// A length, a major and a minor - and the least the dispatcher will look
    /// at, which drops anything under six bytes.
    const HEADER: usize = 6;

    if request.len() < HEADER || u32::from_le_bytes([request[0], request[1], request[2], request[3]]) as usize != request.len() {
        return None;
    }

    /// The status every `major 5` handler reads at `[6]`, and the only one that
    /// is not an error box.
    const GRANTED: u8 = 1;
    let (major, minor) = (request[4], request[5]);
    let body: Vec<u8> = match (major, minor) {
        (1, 0x01) | (1, 0x3d) | (1, 0x3e) => Vec::new(),
        // The 창고 upload. `0x5e4f0` reads nothing of the reply - it closes the
        // notice, counts the save and asks for the listing as `5/0x3d`.
        (0x14, 0x46) => Vec::new(),
        (5, 0x3f) => hero4_catalogue(),
        (5, 0x3d) => hero4_warehouse(),
        (5, 0x41) => hero4_deposit(&request[HEADER..])?,
        (5, 0x04) => hero4_withdraw(&request[HEADER..])?,
        // Whatever `0x5eb04` stores at `0x1566d70` and no other instruction in
        // the archive reads back.
        (5, 0x14) => vec![0, 0, 0, 0],
        // Charged. The message is at `[8]`, empty, and unread on this path.
        (5, 0x42) => vec![GRANTED, 0, 0, 0],
        (5, 0x40) => hero4_delivery(&request[HEADER..])?,
        _ => return None,
    };

    let length = HEADER + body.len();
    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(&(length as u32).to_le_bytes());
    response.push(major);
    response.push(minor);
    response.extend_from_slice(&body);

    Some(response)
}

/// Which stretch of `/ITM/DAT/_ITM_CASH_RANOMBOX` each 보물함 draws from.
///
/// Keyed by the price, because that is what the delivery carries back. Four of
/// [`hero4_catalogue`]'s sixteen rows are the boxes, and they are the only rows
/// priced in five hundreds - the other twelve top out at 500 - so a price of
/// 1500 upward names one box and nothing else.
///
/// The table it draws from is in three parts, and the parts are what the
/// stretches follow:
///
/// ```text
///  0..7   the consumable bundles: 30..50 엘릭서, 3..6 부활의서, 2..5 고급제련석
///  7..17  the four-piece 투구/갑옷/장갑/신발 sets, levels 15-20 up to 50-60
/// 17..23  the 검/건/스태프 sets, over the same level bands
/// ```
///
/// **How the carrier's server weighted a box is not in the archive.** The table
/// is the title's own and every record here is one of its twenty-three, but
/// which of them each box could draw was the server's to decide and it is gone.
/// So: the cheapest box draws the consumables, the dearest draws the weapon
/// sets, and the two between them span the middle - dearer is further up the
/// table, which is the one thing the prices themselves say.
const HERO4_BOX_DRAWS: [(u32, u8, u8); 4] = [
    // 작은보물함, 1500.
    (1500, 0, 7),
    // 보물함, 2000.
    (2000, 0, 17),
    // 큰보물함, 2500.
    (2500, 7, 23),
    // 오래된보물함, 3000.
    (3000, 17, 23),
];

/// What answers 영웅서기4 taking delivery of what it just paid for.
///
/// `0x5e9e0` writes `5/0x40` behind the charge: eight bytes of the row's own
/// `+0x254` and then four of its `+0x60`, which `0x5e1d8` filled from bytes 21
/// to 25 of the catalogue row - the price this side priced it at. The `+0x254`
/// block is zero for every row [`hero4_catalogue`] lays out, so the price is the
/// whole of what names which row was bought.
///
/// `0x5e882` reads `[6]` as the status, `[7]` as how far past the message the
/// item byte sits, and that byte as either `0xff` - put the row's own item in the
/// bag - or an index into `/ITM/DAT/_ITM_CASH_RANOMBOX`, which `0x2544c` opens.
///
/// So only the four rows [`HERO4_BOX_DRAWS`] names are answered with a draw.
/// Everything else is answered `0xff` and hands over what the shop said it was
/// selling, which is what buying 엘릭서 or 소켓확장 was always supposed to do -
/// answering those with a draw as well turned every purchase into a box.
///
/// `None` for a frame that is not the length this message is.
fn hero4_delivery(bought: &[u8]) -> Option<Vec<u8>> {
    /// The status `0x5e882` reads at `[6]`.
    const GRANTED: u8 = 1;
    /// The one item byte that is not an index: hand over the row's own item.
    const OWN_ITEM: u8 = 0xff;
    /// The row's `+0x254` and then its `+0x60`.
    const DELIVERY_REQUEST: usize = 8 + 4;
    const PRICE_AT: usize = 8;

    if bought.len() != DELIVERY_REQUEST {
        return None;
    }

    let price = u32::from_le_bytes([bought[PRICE_AT], bought[PRICE_AT + 1], bought[PRICE_AT + 2], bought[PRICE_AT + 3]]);

    let item = match HERO4_BOX_DRAWS.iter().find(|(paid, _, _)| *paid == price) {
        Some(&(_, from, to)) => from + next_box_draw(to - from),
        None => OWN_ITEM,
    };

    // `[7]` is how far past the message the item byte sits, so the empty message
    // takes the one byte and the item follows it.
    Some(vec![GRANTED, 1, 0, item])
}

/// How far into a box's stretch of the table the next purchase draws.
///
/// The item byte in a `5/0x40` delivery is an index into
/// `/ITM/DAT/_ITM_CASH_RANOMBOX`, and `0xff` is the one value that is not: it
/// tells the delivery to hand over the shop row's own item instead. Four of the
/// sixteen rows are the boxes themselves - `작은보물함`, `보물함`, `큰보물함`
/// and `오래된보물함` - so answering `0xff` put a box in the bag rather than
/// what a box is for, and nothing ever opened it.
///
/// `0x5e8b8` takes the other branch to `0x2544c`, which loads that table,
/// walks it to the drawn record (the table is `[u16 length][body]` records back
/// to back, and `0x39fd0` walks them), and reads the body as
///
/// ```text
/// [0]     u8 - the level the record is for, low end, or 0 for none
/// [1]     u8 - the high end
/// [2..14] four (kind, id, count) triples, a count of 0 ending the list
/// ```
///
/// granting each triple through the title's own `0x3bc68`. Read out of the
/// archive it is twenty-three records exactly, in 368 bytes:
///
/// ```text
///  0..7   the consumable bundles: 30..50 엘릭서, 3..6 부활의서, 2..5 고급제련석
///  7..17  a four-piece 투구/갑옷/장갑/신발 set, for levels 15-20 up to 50-60
/// 17..23  a 검/건/스태프 set, for the same level bands
/// ```
///
/// Which record the carrier's server drew, and how it weighted them, is not
/// something the archive knows - so draw uniformly over the stretch
/// [`HERO4_BOX_DRAWS`] gives the box, which is inside the table and nowhere
/// else. 영웅서기5's boxes are the same problem and draw with this too, over
/// [`HERO5_BOX_LADDER`]. The sequence is a step of a xorshift rather than a counter so
/// consecutive purchases are not the table in order, and it is deterministic
/// within a run, which is what lets a test say what it does.
fn next_box_draw(records: u8) -> u8 {
    next_draw(records as u32) as u8
}

/// The next draw below `range`, out of the same sequence.
///
/// A step of a xorshift rather than a counter, so consecutive draws are not the
/// table in order, and deterministic within a run, which is what lets a test say
/// what it does.
fn next_draw(range: u32) -> u32 {
    use core::sync::atomic::{AtomicU32, Ordering};

    /// Any non-zero seed; xorshift never leaves zero once it is out of it.
    static STATE: AtomicU32 = AtomicU32::new(0x9e37_79b9);

    let mut state = STATE.load(Ordering::Relaxed);
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    STATE.store(state, Ordering::Relaxed);

    state % range
}

/// What answers 영웅서기4 taking an item back out of its 창고.
///
/// 선택된 장비를 싱글 창고에 옮기겠습니까 writes `5/0x04`: twenty-two bytes, the
/// six of a header and the [`HERO4_ITEM_IDENTITY`] naming which row. `0x5ec20`
/// reads none of the answer at all - it builds the item out of its own tables
/// and `0x5de8c` asks for the listing again - so the answer is the command back
/// and nothing else.
///
/// Which is why the row has to go here rather than on the listing that follows:
/// the item is in the bag by then, and a 창고 that still had it would be handing
/// out a second one. `changed` is set only when a row actually went, so a
/// withdrawal that names nothing held writes nothing back.
///
/// `None` for a frame that is not the length this message is.
fn hero4_withdraw(identity: &[u8]) -> Option<Vec<u8>> {
    if identity.len() != HERO4_ITEM_IDENTITY {
        return None;
    }

    let mut held = HERO4_WAREHOUSE.lock();

    match held.rows.iter().position(|row| row[..HERO4_ITEM_IDENTITY] == *identity) {
        Some(at) => {
            held.rows.remove(at);
            held.changed = true;
        }
        None => tracing::debug!("영웅서기4 asked for an item its 창고 was not holding: {identity:02x?}"),
    }

    Some(Vec::new())
}

/// How many rows 영웅서기4's 창고 screen has room for.
///
/// `0x5ea26` clears the row array at `0x1566b1e` sixteen bytes wide before it
/// fills it, one byte a row, which is the same sixteen the catalogue fills. A
/// seventeenth would be written past it.
const HERO4_WAREHOUSE_ROWS: usize = 16;

/// The item record a 창고 deposit carries, and the whole of a listing row bar
/// its first byte.
///
/// `0x5e6ac` serialises an item into thirty-six bytes - two eight-byte blocks
/// from `+0x24c` and `+0x254`, then the kind, the id, the grade, the price at
/// `+0x60`, and the rest a byte at a time - and `0x5e1d8` reads a listing row
/// back through the same offsets one further along. So the record a deposit
/// sends is the row a listing sends back, behind one byte.
const HERO4_ITEM_RECORD: usize = 36;

/// What names one item of 영웅서기4's among the others.
///
/// The first sixteen of its record: the two eight-byte blocks `0x5e6ac` writes
/// out of `+0x24c` and `+0x254`. They are the whole of what a withdrawal
/// carries behind `1600 0000 0504`, and two items of the same kind and grade
/// differ here and nowhere else in the front half of a record.
const HERO4_ITEM_IDENTITY: usize = 16;

/// What 영웅서기4's 창고 is kept under, in the title's own record namespace.
///
/// Among the title's saves rather than beside them, and deliberately: an export
/// collects a title's record stores by its product id, so a 창고 kept anywhere
/// else is one a reinstall loses even when the save comes back. The item is out
/// of the bag by then and the save says so, which makes that a loss of the item
/// rather than of a convenience.
///
/// The cost is that a title enumerating its own databases would see this one.
/// 영웅서기4 cannot: it imports no record-store call at all, keeping its save in
/// a file. A title that both reaches these answers and lists its databases would
/// need this somewhere it cannot look.
pub const HERO4_WAREHOUSE_STORE: &str = "hero4_warehouse";

/// What 영웅서기4's 창고 has been given.
///
/// There is no account here for a 창고 to have been left on, so this is the
/// whole of one: what the title deposited, in the order it deposited it, as the
/// records [`HERO4_ITEM_RECORD`] describes back to back.
///
/// It is brought in from [`HERO4_WAREHOUSE_STORE`] the first time a frame needs
/// it and written back whenever it changes, because the item is out of the
/// title's own bag the moment a deposit is granted - see [`hero4_deposit`].
static HERO4_WAREHOUSE: spin::Mutex<Hero4Warehouse> = spin::Mutex::new(Hero4Warehouse::new());

/// The rows, and whether they have been read in and whether they still match
/// what was written out.
struct Hero4Warehouse {
    rows: Vec<[u8; HERO4_ITEM_RECORD]>,
    /// False until [`load_hero4_warehouse`] has run, whether or not anything was
    /// kept - an empty 창고 is a 창고, and reading it in twice would lose a
    /// deposit made between the two.
    loaded: bool,
    changed: bool,
}

impl Hero4Warehouse {
    const fn new() -> Self {
        Self {
            rows: Vec::new(),
            loaded: false,
            changed: false,
        }
    }
}

/// Whether this frame is one whose answer 영웅서기4's 창고 is behind, and the
/// 창고 has not been read in yet.
///
/// Three: `5/0x3d` answers out of it, `5/0x41` adds to it and `5/0x04` takes
/// from it. Every other frame here, and every other title's, is none of its
/// business - which is what keeps this off the path of a title that has no 창고
/// at all.
pub fn hero4_warehouse_needs_loading(request: &[u8]) -> bool {
    const HEADER: usize = 6;

    if request.len() < HEADER || u32::from_le_bytes([request[0], request[1], request[2], request[3]]) as usize != request.len() {
        return false;
    }
    if !matches!((request[4], request[5]), (5, 0x3d) | (5, 0x41) | (5, 0x04)) {
        return false;
    }

    !HERO4_WAREHOUSE.lock().loaded
}

/// Fill 영웅서기4's 창고 from what was kept for it.
///
/// The records back to back, as [`hero4_warehouse_to_keep`] wrote them. A
/// trailing part-record is dropped rather than guessed at, and anything past
/// [`HERO4_WAREHOUSE_ROWS`] with it - the listing has no room to send it back.
///
/// Marks the 창고 read in whether or not anything was there, so an empty store
/// is not read again over a deposit made after it.
pub fn load_hero4_warehouse(kept: &[u8]) {
    let mut warehouse = HERO4_WAREHOUSE.lock();

    warehouse.rows = kept
        .chunks_exact(HERO4_ITEM_RECORD)
        .take(HERO4_WAREHOUSE_ROWS)
        .map(|record| {
            let mut row = [0u8; HERO4_ITEM_RECORD];
            row.copy_from_slice(record);
            row
        })
        .collect();
    warehouse.loaded = true;
    warehouse.changed = false;
}

/// What 영웅서기4's 창고 holds, when it is not what was last kept for it.
///
/// `None` when nothing has changed, so the caller writes only what a deposit
/// actually moved.
pub fn hero4_warehouse_to_keep() -> Option<Vec<u8>> {
    let mut warehouse = HERO4_WAREHOUSE.lock();

    if !warehouse.changed {
        return None;
    }
    warehouse.changed = false;

    let mut kept = Vec::with_capacity(warehouse.rows.len() * HERO4_ITEM_RECORD);
    for row in &warehouse.rows {
        kept.extend_from_slice(row);
    }

    Some(kept)
}

/// What 영웅서기4's 창고 holds.
///
/// The 창고 is the other half of the online menu `0x61998` labels - the same NPC
/// screen that reads 상점 when the flag at `+0xd4` is set. Opening it writes a
/// `0x14/0x46` of its own: a 1138-byte record carrying the character as it
/// stands. `0x5e3a0` takes major `0x14`, `0x5e4f0` takes minor `0x46`, and that
/// handler reads nothing at all of the reply - it closes the 서버 응답을
/// 기다리는중 notice, counts the save at `0x151b190`, and appends `05 3d 00 00`
/// through `0x56f5c` to ask for the listing.
///
/// Which is `5/0x3d` at `0x5ea26`, and it is `5/0x3f` - the catalogue
/// [`hero4_catalogue`] answers - with one byte in front. Laid side by side, the
/// two handlers fill the same fields of the same record:
///
/// ```text
///           5/0x3f (0x5eb1e)      5/0x3d (0x5ea26)
/// 0x1566b1a  <- [6]                <- [7]
/// 0x1566b1b  <- [7]                <- [8]
/// 0x1566b1c  <- [8..10] the count  <- [9..11] the count
/// 0x1566b1e  <- rows from [10]     <- rows from [11]
/// 0x1566b18  -                     <- [6]
/// ```
///
/// Rows are the same 37 bytes either way, walked by the same `0x5e1d8`. The one
/// field `0x3d` has to itself is `[6]`, into `0x1566b18`, which no instruction
/// in the archive reads back - so it takes the granted status every other
/// `major 5` reply carries at `[6]`, which is the only value that could matter
/// if something did.
///
/// Behind the listing the title asks `5/0x14`, and that one is answered with
/// four bytes for the same reason: `0x5eb04` stores them at `0x1566d70` and
/// nothing reads them.
///
/// What the listing carries is what [`hero4_deposit`] has been handed, laid out
/// as the rows the title reads: a byte of its own - zero, which is what the
/// catalogue's rows carry there - and then the record the deposit sent.
fn hero4_warehouse() -> Vec<u8> {
    /// The status every `major 5` reply carries at `[6]`.
    const GRANTED: u8 = 1;
    /// The two bytes the catalogue leads with, which this listing shares.
    const LEAD: [u8; 2] = [0, 1];
    /// A row, its own first byte included.
    const ROW: usize = 1 + HERO4_ITEM_RECORD;

    let held = HERO4_WAREHOUSE.lock();

    let mut body = Vec::with_capacity(1 + LEAD.len() + 2 + held.rows.len() * ROW);
    body.push(GRANTED);
    body.extend_from_slice(&LEAD);
    body.extend_from_slice(&(held.rows.len() as u16).to_le_bytes());

    for record in &held.rows {
        // The byte `0x1566b1e + i` takes, which the catalogue leaves zero.
        body.push(0);
        body.extend_from_slice(record);
    }

    body
}

/// What answers 영웅서기4 moving an item into its 창고.
///
/// 선택된 장비를 대전 창고에 옮기겠습니까 writes `5/0x41`: forty-two bytes, the
/// six of a header and the thirty-six [`HERO4_ITEM_RECORD`] describes.
/// `0x5ea10` reads exactly one byte of the answer. `[6]` zero takes `0x5ec80`,
/// which draws the message at `[8]` as an error box and leaves the item where it
/// was; anything else runs `0x5e658`, which is the move itself - the title takes
/// the item out of its own bag and the screen goes on.
///
/// So the item is gone from the bag the moment this grants it, and the only
/// place it then exists is the listing [`hero4_warehouse`] sends back. That is
/// what a 창고 was, and it is why this keeps what it is handed rather than
/// granting and forgetting.
///
/// So it is kept where it survives the run: [`hero4_warehouse_to_keep`] hands
/// the rows to whoever answered the frame, to write into
/// [`HERO4_WAREHOUSE_STORE`], and [`load_hero4_warehouse`] brings them back the
/// next time a 창고 frame needs them. Losing them would be losing the item
/// outright - the title has already saved itself without it.
///
/// A deposit past the sixteenth is refused rather than dropped, with the empty
/// message the error box reads at `[8]`: `0x5ea26` only has room for
/// [`HERO4_WAREHOUSE_ROWS`], and granting a row it cannot list back would lose
/// the item outright. `None` for a frame that is not the length this message is,
/// which is the only shape whose record is known.
fn hero4_deposit(record: &[u8]) -> Option<Vec<u8>> {
    /// What `0x5ea10` takes as the move having gone through.
    const GRANTED: u8 = 1;
    /// The one value it takes as the error box, whose message is at `[8]`.
    const REFUSED: u8 = 0;

    if record.len() != HERO4_ITEM_RECORD {
        return None;
    }

    let mut held = HERO4_WAREHOUSE.lock();

    if held.rows.len() >= HERO4_WAREHOUSE_ROWS {
        // The status, a byte where the granted path reads none, and the message
        // the error box draws - empty, because there is no server to have
        // written one.
        return Some(vec![REFUSED, 0, 0]);
    }

    let mut row = [0u8; HERO4_ITEM_RECORD];
    row.copy_from_slice(record);
    held.rows.push(row);
    held.changed = true;

    Some(vec![GRANTED])
}

/// The body of 영웅서기4's shop catalogue: the page it is on, how many pages
/// there are, and the rows themselves.
///
/// The handler at `0x5eb1e` reads the body as
///
/// ```text
/// [0]      u8  - the page this is
/// [1]      u8  - how many pages there are
/// [2..4]   u16 LE - how many rows follow
/// [4..]    the rows, 37 bytes each
/// ```
///
/// and keeps the first three at `0x15669e8+0x132`, which is where the shop
/// screen's own left/right handler at `0x5ee10` reads the page from and wraps it
/// against the page count. One page and sixteen rows on it.
///
/// A row is read by `0x5e1d8`, which builds each item with the title's own
/// factory and then overrides four of its fields out of the row:
///
/// ```text
/// [0]      u8  - a per-row flag the shop screen keeps beside the list
/// [1..9]   the server's first handle for the row, echoed back on a purchase
/// [9..17]  its second, which is what a purchase actually sends
/// [17]     u8  - the item kind, which is what the local item table is keyed by
/// [18]     u8  - the item id in that table, where its name and icon come from
/// [19]     u8  - the grade
/// [21..25] u32 LE - the price
/// ```
///
/// **The list itself is not the original service's.** It is the one the
/// 영웅서기4_보물함 build carries, which reaches the same screen without a server
/// at all: its patch redirects the two `0x5ded8` catalogue requests to a stub
/// that returns without sending, and builds the sixteen items itself at
/// `0x7d31c` from a table of ids at `0x7d3c4` and a table of grades at `0x7d3d4`,
/// the grade in the low nibble and a multiplier in the high one, priced at fifty
/// won a step for the first twelve rows and five hundred for the last four. The
/// first row's grade is `0x14` rather than its low nibble, which is that build's
/// own exception and is kept here.
///
/// Reproducing it over the wire rather than patching the module is what lets an
/// unmodified archive reach the same screen: the title's own handler builds the
/// same items from the same ids, and everything the row does not name - the
/// item's name, icon and stats - still comes from the archive's own item table.
fn hero4_catalogue() -> Vec<u8> {
    /// Every row is this wide, whether or not it fills it.
    const ROW: usize = 37;
    /// The kind the local item table is keyed by for all sixteen, which is what
    /// the 보물함 build passes its factory.
    const KIND: u8 = 8;
    /// Rows one to twelve are priced in fifties, the last four in five hundreds.
    const CHEAP_ROWS: usize = 12;
    const CHEAP_STEP: u32 = 50;
    const COSTLY_STEP: u32 = 500;
    /// The first row's grade, which the reference build spells out rather than
    /// taking from its table.
    const FIRST_GRADE: u8 = 0x14;

    /// `(item id, packed grade)` - the grade in the low nibble and the price's
    /// multiplier in the high one, as `0x7d3c4` and `0x7d3d4` pair them.
    const ROWS: [(u8, u8); 16] = [
        (0x0f, 0x50),
        (0x05, 0x5a),
        (0x10, 0x45),
        (0x14, 0x21),
        (0x13, 0xa1),
        (0x18, 0xaa),
        (0x15, 0xa5),
        (0x16, 0xa1),
        (0x11, 0x61),
        (0x12, 0x61),
        (0x1d, 0xa1),
        (0x17, 0xa1),
        (0x19, 0x31),
        (0x1a, 0x41),
        (0x1b, 0x51),
        (0x1c, 0x61),
    ];

    let mut body = vec![0u8; 4 + ROWS.len() * ROW];
    body[0] = 0;
    body[1] = 1;
    body[2..4].copy_from_slice(&(ROWS.len() as u16).to_le_bytes());

    for (index, (item, packed)) in ROWS.into_iter().enumerate() {
        let step = if index < CHEAP_ROWS { CHEAP_STEP } else { COSTLY_STEP };
        let price = step * (packed >> 4) as u32;
        let grade = if index == 0 { FIRST_GRADE } else { packed & 0x0f };

        let row = &mut body[4 + index * ROW..4 + (index + 1) * ROW];
        row[17] = KIND;
        row[18] = item;
        row[19] = grade;
        row[21..25].copy_from_slice(&price.to_le_bytes());
    }

    body
}
/// What answers the text record 아니마 buys a cash item with.
///
/// 아니마 (`0003266D`) reaches `211.239.165.13:8035` through `MC_netBillSocket`
/// and writes ASCII. Buying a 부활마법서 for 3000원 writes forty bytes:
///
/// ```text
/// AM40    1911112222 10 SB_부활마법서_3000_M
/// ^^ ^^^^^^ ^^^^^^^^^^ ^^ ^^^^^^^^^^^^^^^^^^
/// |  |      |          |  the command, EUC-KR
/// |  |      |          the two characters `%2.2s` fills, always "10"
/// |  |      the ten digits `0xf03c` copies in
/// |  the whole record's length, `%-6d`
/// the tag
/// ```
///
/// which `0x3d954` builds as `sprintk(dest, "AM%-6d%10.10s%2.2s", length, id,
/// "10")` and copies out as exactly twenty bytes before the command.
///
/// A **reply** is framed differently, and much more simply. `0x3ebd8` waits for
/// the tag - `AM` as a `u16`, or `@` for the other server's `@A` - then
/// `atoi`s the text at `[2]` as the whole record's length and hands the record
/// on once that many bytes have arrived. `0x3dd4c` then takes six characters of
/// that length and reads the body from `[8]`:
///
/// ```text
/// AM10    SB
/// ^^ ^^^^^^ ^^
/// |  |      the body
/// |  the length, six characters this side rather than twenty
/// the tag
/// ```
///
/// The body's first two characters are all a purchase is asked for. `0x3df38`
/// switches on the transaction the title set - `0x3cb7c` sets `15` for a
/// purchase - and every one of those thirty-two handlers opens by comparing two
/// characters. `15` is `0x3e954`, which compares them against `SB` and returns
/// granted or refused on that alone.
///
/// So the answer is the tag, the reply's own length, and the two characters the
/// command was sent under - taken from the request rather than chosen here. That
/// is exact for a purchase. A handler that reads a body past those two
/// characters finds it empty, which is not something to fill in from this side.
///
/// `None` for anything that is not one of these records: it has to carry the
/// tag, declare its own length there, and have a command behind the header.
pub fn lgt_local_text_record_response(request: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"AM";
    /// `%-6d`, which is also the width the reply's own length is read at.
    const LENGTH_FIELD: usize = 6;
    /// The tag, the length, the subscriber's ten digits and the two `%2.2s`
    /// fills - what the title copies out before its command.
    const REQUEST_HEADER: usize = TAG.len() + LENGTH_FIELD + 10 + 2;
    /// A reply carries the tag and the length alone.
    const REPLY_HEADER: usize = TAG.len() + LENGTH_FIELD;
    /// Which is all a purchase's handler compares.
    const COMMAND: usize = 2;

    if !request.starts_with(TAG) || request.len() < REQUEST_HEADER + COMMAND {
        return None;
    }

    if atoi(&request[TAG.len()..TAG.len() + LENGTH_FIELD])? != request.len() {
        return None;
    }

    let command = &request[REQUEST_HEADER..REQUEST_HEADER + COMMAND];
    if !command.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }

    let length = REPLY_HEADER + COMMAND;
    let digits = format!("{length}");
    if digits.len() > LENGTH_FIELD {
        return None;
    }

    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(TAG);
    // Left justified, the way the title writes its own.
    response.extend_from_slice(digits.as_bytes());
    response.resize(REPLY_HEADER, b' ');
    response.extend_from_slice(command);

    Some(response)
}

/// The leading number of an ASCII field, as C's `atoi` reads one: optional
/// blanks, then digits, stopping at the first byte that is not one.
///
/// `None` where there is no number at all, so a field that is not one is not
/// read as zero.
fn atoi(field: &[u8]) -> Option<usize> {
    let digits = field.iter().skip_while(|byte| byte.is_ascii_whitespace());
    let mut value: Option<usize> = None;

    for byte in digits.take_while(|byte| byte.is_ascii_digit()) {
        value = Some(value.unwrap_or(0).checked_mul(10)?.checked_add((byte - b'0') as usize)?);
    }

    value
}
/// What answers the tagged record the 와일드프론티어 titles buy a cash item with.
///
/// Both reach a billing socket and write a record under the same eight byte
/// header, which each title's own builder fills the same way - `0x4c0cc` in
/// 와일드프론티어 (`0002CB52`), `0x2c7ac` in 와일드프론티어2 (`0003535F`):
///
/// ```text
/// [0..2]  the tag, `KP`
/// [2..4]  u16 LE - the whole record's length
/// [4..6]  u16 LE - the shape, which is the one thing the two do not share
/// [6]     u8     - what the record is
/// [7]     u8
/// ```
///
/// The first writes shape `7` and a thirty-six byte purchase as record `9` - the
/// item as its own aid and a three digit code, the subscriber's number, then the
/// price. The second writes shape `27` and a forty byte purchase as record `3` -
/// the subscriber first, then the item, a word, then the price.
///
/// They read a reply differently, and the difference is what the answer's body
/// has to be.
///
/// The first frames it: `0xfdb6` takes four bytes, reads `[2]` as a `u16` for
/// the whole record's length, reads the rest, and `0xfeaa` treats `[7]` as an
/// error unless it is zero before passing `[8..]` and `[6]` to `0x4c792`. That
/// switches on the record byte, and the purchase's `9` is `0x4c998`: it compares
/// the **first byte of the body** against `1`.
///
/// The second does not frame it at all. `0x2d3c0` appends whatever arrives and
/// runs the loop at `0x2cc60`, which takes eight bytes whenever that many are
/// buffered and switches on `[6]` alone - the length and `[7]` go unread, and
/// each handler waits for as much as it needs of its own. The purchase's `3` is
/// `0x2d212`: it waits for twelve, takes a **`u32` at `[8]`** and grants on its
/// low byte being `1`.
///
/// So the answer is the tag, its own length, the shape it was asked in, the
/// record byte it was asked under, a zero status, and a granted body sized the
/// way that shape's reader reads one. That is exact for a purchase. The other
/// record kinds share the header; one that reads more behind the body finds
/// nothing, which is not something to fill in from here.
///
/// `None` for anything that is not one of these records: it has to carry the
/// tag, declare its own length, and be in a shape whose reader is known.
pub fn lgt_local_tagged_record_response(request: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"KP";
    /// The tag, the length, the shape, the record byte and one more.
    const HEADER: usize = 8;
    /// `[7]`, which the first title reads as an error unless it is zero.
    const GRANTED_STATUS: u8 = 0;

    /// 와일드프론티어's shape, whose purchase is granted on one byte.
    const BYTE_BODY_SHAPE: u16 = 7;
    /// 와일드프론티어2's, whose purchase is granted on a `u32`'s low byte.
    const WORD_BODY_SHAPE: u16 = 27;

    if !request.starts_with(TAG) || request.len() < HEADER {
        return None;
    }

    if u16::from_le_bytes([request[2], request[3]]) as usize != request.len() {
        return None;
    }

    let shape = u16::from_le_bytes([request[4], request[5]]);
    let body: &[u8] = match shape {
        BYTE_BODY_SHAPE => &[1],
        WORD_BODY_SHAPE => &[1, 0, 0, 0],
        _ => return None,
    };

    let length = HEADER + body.len();
    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(TAG);
    response.extend_from_slice(&(length as u16).to_le_bytes());
    response.extend_from_slice(&shape.to_le_bytes());
    // The record byte comes back as it was asked under, which is what the
    // handler is chosen by.
    response.push(request[6]);
    response.push(GRANTED_STATUS);
    response.extend_from_slice(body);

    Some(response)
}
/// 엘피스's online menu and its item purchase, whose every message is one byte
/// of opcode behind a five byte header.
///
/// The title carries its own message library at `0x64000`, and both directions
/// go through it. `0x64984` writes a message and `0x64888` reads one, and the
/// frame they agree on is five bytes:
///
/// ```text
/// [0..2]  u16 BE - the whole message, this header counted
/// [2]     u8
/// [3]     u8     - the opcode
/// [4]     u8
/// [5..]          - the body
/// ```
///
/// `0x64a88` is where every one of the writer's message kinds ends up, and it
/// is the one place the length is written: `total = body + 5`. The reader's
/// `0x64354` takes the same five apart, refuses a message whose declared length
/// is not the bytes in hand, and hands `[3]` to the title's own table at
/// `0x6d160` - two hundred and sixteen entries, one per opcode. So an answer is
/// read by whichever handler its own opcode names, and what each one takes out
/// of the body is what the body has to be.
///
/// The menu opens with the library's type `4`, whose body `0x64d28` lays out and
/// whose opcode `0x64dde` fixes at zero:
///
/// ```text
/// [0..2]   u16 BE - the service, which is 1006, 1017 or 1036
/// [2..4]   u16 BE
/// [4]      u8
/// [5]      u8
/// [6..26]         - the build, "Ver 1.0.4"
/// [26..68]        - the subscriber's number
/// [68..98]        - the handset model
/// ```
///
/// Ninety-eight bytes, so a hundred and three on the wire - for 엘피스.
/// 슈퍼액션히어로3 writes the same layout under service 1017 and stops at the
/// subscriber's number, sixty-eight bytes and seventy-three on the wire:
///
/// ```text
/// 00 49 00 00 00 03 f9 00 00 02 05 "V.1.0.0" ... "01062170215" ...
/// ```
///
/// So the handset model is what the longer one carries and the shorter one
/// leaves off, and what says a frame is this opening is the service in front of
/// it rather than the length behind it. Opcode zero's handler
/// at `0x488f0` reads no body at all: it switches on the screen the menu was
/// entered from and sends that screen's own next request. Opcode one, which the
/// library's type `5` writes with no body of its own, is the same kind of thing:
/// `0x47e70` raises the title's event `5` and returns. Both are answered with
/// the opcode they were asked under and nothing behind it.
///
/// Buying an item is a walk, and each step is a message whose sender records
/// which step it left off at in `[0x16c9bac + 0x20]`:
///
/// - `0x474a4` sends opcode `0x43` as the product code and a quantity, two `u16`
///   each, and its answer at `0x480c6` takes a **`u32`** and keeps it.
/// - `0x47604` sends opcode `0xc9` as a `u16` `0x14`, the amount, and the
///   quantity. Its answer at `0x47fea` takes a **`u32` and eight bytes** - the
///   order and the code that stands for it - and hands both straight to
///   `0x47b04`, which sends them back out under opcode `0xcb`.
///
/// - That one's answer at `0x47dd0` takes a **`u32`** and moves the title on to
///   `0x47b5c`, which sends opcode `0x44` as the order, the product code and the
///   quantity, and the code that stands for the order - four, two, two and eight
///   bytes.
/// - That last one's answer at `0x481d6` takes **nothing** out of the body. It
///   releases the message, raises the title's event `5`, and writes the step
///   marker back to zero, which is the purchase finishing.
///
/// 슈퍼액션히어로3 sends one more of these once its session is open - opcode
/// `0x32`, ninety-seven bytes of body:
///
/// ```text
/// 00 66 00 32 00 00 00 ... 01 00 00 38 ... 0c b6 1d 5b ...
/// ```
///
/// 엘피스's handler for that opcode, `0x48c5c`, takes a **`u32`** and keeps it
/// at `[0x15041b8]` without comparing it against anything, so it is answered
/// with one.
///
/// None of them compares what it reads against anything, so the numbers are the
/// shop's to issue; the eight bytes come back as a string, and the two the title
/// copies past them are the zeroes it cleared. What matters is the size each
/// reader takes, which is what these answers are.
///
/// 아이뮤지션2 (`00032548`) opens the same session under service 1017 and then
/// sends opcode `0x14`, its licence check, which it will not start without:
///
/// ```text
/// 00 4f 00 14 00  0c  04 28  "Emulator" ...  03  "00032548" ...
/// ```
///
/// Its sender at `0x1e3b2` lays that body out as a byte, a `u16`, fifty bytes
/// of handset model, a byte and twenty of application id - seventy-four, which
/// is the `MC_knlAlloc(0x4a)` in front of it on the wire. The answer's reader
/// is not guessed at: the title parses every reply in `0x1d598`, which takes
/// the opcode out of `[3]` and jumps through the table at `0x6ca78`, and that
/// table's entry for `0x14` is `0x1d854`. There it takes a **byte**, a
/// **`u16`** length, and **that many bytes**, keeping them at `[ctx+0x44]`,
/// `[ctx+0x46]` and `[ctx+0x48]` - the same three fields the request filled in,
/// so the answer overwrites the model with whatever the licence server had to
/// say. The byte is the verdict and the string is only kept, so the answer is
/// the verdict and an empty string.
///
/// `None` for anything that is not one of those messages: it has to declare its
/// own length, leave `[2]` and `[4]` clear, and be an opcode whose reader's
/// shape is known.
pub fn lgt_local_opcode_header_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length, two bytes the title writes clear, and the opcode between
    /// them.
    const HEADER: usize = 5;

    /// The opcode the menu opens the session under, and answers under.
    const SESSION_OPCODE: u8 = 0x00;
    /// What `0x64d28` lays out, before the header - up to and including the
    /// subscriber's number. The handset model behind it brings 엘피스's to 98,
    /// and 슈퍼액션히어로3 leaves it off.
    const SESSION_BODY_MIN: usize = 68;
    /// The three services `0x471f8` writes into the body's first two bytes.
    const SERVICES: [u16; 3] = [1006, 1017, 1036];

    /// The library's type `5`, which carries nothing and is answered in kind.
    const SIGNAL_OPCODE: u8 = 0x01;

    /// What 슈퍼액션히어로3 sends once its session is open, and `0x48c5c` reads
    /// the answer of.
    const REPORT_OPCODE: u8 = 0x32;

    /// 아이뮤지션2's licence check, which it sends the moment its session is
    /// open and will not start without.
    const LICENCE_OPCODE: u8 = 0x14;
    /// Its body, laid out by the sender at `0x1e3b2`: a byte, a `u16`, fifty
    /// bytes of handset model, a byte, and twenty of application id.
    const LICENCE_BODY: usize = 1 + 2 + 50 + 1 + 20;
    /// The verdict the answer's reader keeps at `[ctx+0x44]`. Zero is the one
    /// the title carries on from.
    const LICENCED: u8 = 0;

    /// `0x474a4`'s product code and quantity.
    const ORDER_OPCODE: u8 = 0x43;
    /// `0x47604`'s amount, and `0x47fea` reads the answer.
    const APPROVAL_OPCODE: u8 = 0xc9;
    const APPROVAL_ANSWER_OPCODE: u8 = 0xca;
    /// `0x47b04` sends the approval back, and `0x47dd0` reads the answer.
    const CONFIRM_OPCODE: u8 = 0xcb;
    const CONFIRM_ANSWER_OPCODE: u8 = 0xcc;
    /// `0x47b5c` closes the walk, and `0x481d6` reads the answer for its opcode
    /// alone.
    const SETTLE_OPCODE: u8 = 0x44;

    /// The order every step of the walk carries, which is the shop's to issue
    /// and which nothing in the title compares against anything.
    const ORDER: u32 = 1;
    /// The eight bytes `0x47fea` keeps as the order's code, and `0x47b04` sends
    /// back out. It reads ten of them into a buffer it cleared, so this is a
    /// string of eight with its terminator already behind it.
    const ORDER_CODE: &[u8; 8] = b"00000001";

    if request.len() < HEADER || u16::from_be_bytes([request[0], request[1]]) as usize != request.len() {
        return None;
    }

    if request[2] != 0 || request[4] != 0 {
        return None;
    }

    let body = &request[HEADER..];
    let (opcode, answer): (u8, Vec<u8>) = match request[3] {
        SESSION_OPCODE if body.len() >= SESSION_BODY_MIN && SERVICES.contains(&u16::from_be_bytes([body[0], body[1]])) => {
            (SESSION_OPCODE, Vec::new())
        }
        SIGNAL_OPCODE if body.is_empty() => (SIGNAL_OPCODE, Vec::new()),
        // Whatever it carries, its answer is the one `u32` `0x48c5c` takes.
        REPORT_OPCODE if !body.is_empty() => (REPORT_OPCODE, Vec::from(0u32.to_be_bytes())),
        // The verdict, and a string behind its length - which the title keeps
        // but does not need, so it is answered with none.
        LICENCE_OPCODE if body.len() == LICENCE_BODY => {
            let mut answer = alloc::vec![LICENCED];
            answer.extend_from_slice(&0u16.to_be_bytes());
            (LICENCE_OPCODE, answer)
        }
        // The product code and the quantity, and a `u32` back.
        ORDER_OPCODE if body.len() == 4 => (ORDER_OPCODE, Vec::from(ORDER.to_be_bytes())),
        // The `u16` `0x14` `0x47604` opens with, the amount, and the quantity.
        APPROVAL_OPCODE if body.len() == 8 && u16::from_be_bytes([body[0], body[1]]) == 0x14 => {
            let mut answer = Vec::from(ORDER.to_be_bytes());
            answer.extend_from_slice(ORDER_CODE);
            (APPROVAL_ANSWER_OPCODE, answer)
        }
        // The order and its code, sent back the way `0x47fea` handed them over.
        CONFIRM_OPCODE if body.len() == 4 + ORDER_CODE.len() => (CONFIRM_ANSWER_OPCODE, Vec::from(ORDER.to_be_bytes())),
        // The order, the product code and the quantity, and the order's code
        // behind them. Nothing reads the answer's body, so it has none.
        SETTLE_OPCODE if body.len() == 4 + 4 + ORDER_CODE.len() => (SETTLE_OPCODE, Vec::new()),
        _ => return None,
    };

    let length = HEADER + answer.len();
    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(&(length as u16).to_be_bytes());
    response.push(0);
    response.push(opcode);
    response.push(0);
    response.extend_from_slice(&answer);

    Some(response)
}

/// The granted answer to an application billing request, in the frame shape
/// `lgt_local_purchase_success_response` establishes for the purchase
/// transaction: the `0xffff` marker, the frame length, the request's own type
/// plus one, and a zero status - which is what this protocol spells "granted".
///
/// `None` for anything that is not one of these frames, which is not something
/// to answer with a guess.
pub fn lgt_local_granted_response(request: &[u8]) -> Option<Vec<u8>> {
    let frame = BillFrame::parse(request)?;
    let order = frame.order;
    let message_type = frame.message_type;

    let length = order.write(GRANTED_FRAME_SIZE as u16);
    let response_type = order.write(message_type.wrapping_add(1));

    // Answered in the order it was asked in: a title that wrote its length
    // little end first reads the answer's the same way.
    Some(vec![0xff, 0xff, length[0], length[1], response_type[0], response_type[1], 0x00])
}

/// The opening exchange 던파귀검사편 and 바람의나라 share, answered the way
/// their own readers read it.
///
/// The title opens a `MC_netBillSocket` for `211.115.203.30:10012` and writes
/// one frame before it will leave `사용자 인증`. Both directions carry the same
/// eight-byte header, little end first, and the length counts the header:
///
/// ```text
/// [0..4]   u32 - the whole frame's length
/// [4..6]   u16 - 0xffff
/// [6..8]   u16 - the command
/// ```
///
/// `0x727c` is the one place that header is written and `0x6fdc` the one place
/// it is read. The reader takes bytes until it holds more than seven, refuses a
/// frame whose `[4..6]` is not `0xffff`, waits until it holds the length the
/// frame declares, and hands everything past the header to `0xe3c0` - which
/// switches on the command alone.
///
/// The request is command `0x2711`, laid out by `0xb030` as two length-prefixed
/// strings: the title's own name, and then either that name again or
/// `UserAuthentication`, whichever the screen asked under. The name is the
/// title's alone - `DnFSwordMan` for one, `Baram` for the other - so it is the
/// shape that says a frame is this exchange, not the name:
///
/// ```text
/// 29 00 00 00 ff ff 11 27 0b 00 "DnFSwordMan"       12 00 "UserAuthentication"
/// 23 00 00 00 ff ff 11 27 05 00 "Baram"             12 00 "UserAuthentication"
/// ```
///
/// Its answer is command `0x2712`, whose reader `0xe360` takes a fixed shape
/// out of the body and compares it against nothing:
///
/// ```text
/// [0]      u8  - the result
/// [1..3]   u16 - a message length
/// [3..]          the message, that many bytes
/// ```
///
/// `0xb234` is what reads the result, and during authentication - where
/// `[0x1500097]` is the non-zero state `0x2e824` put there - **0** and **2**
/// both go on, while 1, 3 and 4 close the socket and stop the title. So this
/// answers 0, with no message behind it: nothing displays one on the way
/// through, and `0xe360` copies a zero-length one happily.
///
/// What the title sends next, having gone on, is command `0x28a0`: one string
/// of the pairs `0x7890` spells out -
///
/// ```text
///   phonenum:01024417543 carrier:lgt platform:lgt_wipic app_name:DnFSwordMan
///   external_app_version:1.0.0 ... sms:(null)
/// ```
///
/// Its answer is `0x28a1`, and `0xe140` reads a longer fixed shape:
///
/// ```text
/// [0]       u8  - the result
/// [1..12]         eleven bytes it copies to a buffer nothing then reads
/// [12..14]  u16 - a message length
/// [14..]          the message, that many bytes
/// ```
///
/// `0xb234` reads that result too, and this one is the other way round: **0**
/// closes the socket and anything from 1 up goes on. So it answers 1, with the
/// eleven bytes zero and no message.
///
/// A screen that opens its own connection - 세라샵 does, to buy an item - takes
/// a third step instead of that one. `0xb234` sends it to `0x7818` rather than
/// to `0xaebc` when `[0x1500097]` is set, and `0x7818` writes command `0x00`:
/// the subscriber's number as eleven bytes and a `u16` behind it.
///
/// ```text
/// 15 00 00 00 ff ff 00 00 "01024417543" 01 00
/// ```
///
/// Its answer is command `0x01`, read by `0xe084`:
///
/// ```text
/// [0]      u8  - the result
/// [1..3]   u16 - a message length
/// [3..]          the message, that many bytes
/// [3+n..]        four bytes, which `0xe084` keeps only when the result is 0
/// ```
///
/// Here **0** is the result that goes on and 1 or more stops - the opposite of
/// the step before it again - so it answers 0, no message, and the four bytes
/// zero, which is what makes the reader take all four.
///
/// Past that the screen sends what it opened the connection for. 세라샵's is
/// command `0x20`, which `0x7390` lays out as a `u32` code and the byte
/// `0x1e` behind it - thirteen bytes in all:
///
/// ```text
/// 0d 00 00 00 ff ff 20 00 42 00 00 00 1e
/// ```
///
/// Its answer is command `0x21`, and `0xc098` reads the same three fields
/// `0xe360` does - a result, a message length, and that many bytes. Nothing in
/// `0xb234` branches on this one's result at all; the screen's own state is
/// what moves on. So it answers 0 with no message.
///
/// Neither payload can be left out altogether. `0x70a0` allocates a block only
/// for a frame that declares more than its header, and hands `0xe3c0` a null
/// pointer otherwise, which both readers would read from - so each answer is
/// the bytes its own reader takes and no fewer.
///
/// `None` for anything that is not one of those requests: it has to declare its
/// own length, carry the marker, be a command whose reader's shape is known,
/// and spell strings that end exactly where the frame does.
pub fn lgt_local_marked_command_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length, the marker and the command.
    const HEADER: usize = 8;
    const MARKER: u16 = 0xffff;

    /// What `0xb030` sends, and what `0xe360` reads the answer of.
    const AUTH_REQUEST: u16 = 0x2711;
    const AUTH_ANSWER: u16 = 0x2712;

    /// What `0x7890` sends after that, and `0xe140` reads the answer of.
    const REGISTER_REQUEST: u16 = 0x28a0;
    const REGISTER_ANSWER: u16 = 0x28a1;

    /// What `0x7818` sends instead, on a connection a screen opened for
    /// itself, and `0xe084` reads the answer of.
    const SESSION_REQUEST: u16 = 0x0000;
    const SESSION_ANSWER: u16 = 0x0001;
    /// The subscriber's number and the `u16` behind it, which is the whole of
    /// that request's body.
    const SESSION_BODY: usize = 11 + 2;
    /// The four bytes `0xe084` takes past the message, and only for a granted
    /// result.
    const SESSION_TRAILER: usize = 4;

    /// What `0x7390` sends for the screen's own errand, and `0xc098` reads the
    /// answer of.
    const ERRAND_REQUEST: u16 = 0x0020;
    const ERRAND_ANSWER: u16 = 0x0021;
    /// A `u32` code and the byte `0x7390` always writes behind it.
    const ERRAND_BODY: usize = 5;
    const ERRAND_TAIL: u8 = 0x1e;

    /// The results `0xb234` goes on from. They are not the same value: the
    /// register step stops on 0 where the other two go on from it.
    const AUTH_GRANTED: u8 = 0;
    const REGISTER_GRANTED: u8 = 1;
    const SESSION_GRANTED: u8 = 0;

    /// The eleven bytes `0xe140` takes between the result and the message,
    /// and then reads nothing out of - it copies them to a stack buffer and
    /// the message length has to land at `[12..14]` behind them.
    const REGISTER_UNREAD: usize = 11;

    fn u16_at(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
    }

    /// One length-prefixed string off the front of a body.
    fn string_at(body: &[u8]) -> Option<(&[u8], &[u8])> {
        if body.len() < 2 {
            return None;
        }

        let length = u16_at(body, 0) as usize;
        body[2..].split_at_checked(length)
    }

    if request.len() < HEADER || u32::from_le_bytes(request[0..4].try_into().ok()?) as usize != request.len() {
        return None;
    }

    if u16_at(request, 4) != MARKER {
        return None;
    }

    let body = &request[HEADER..];
    let (answer, payload): (u16, Vec<u8>) = match u16_at(request, 6) {
        // Two strings, ending where the frame does. The first is the title's
        // own name, so it is only held to being one - printable and not empty.
        AUTH_REQUEST => {
            let (name, rest) = string_at(body)?;
            let (_, rest) = string_at(rest)?;

            if name.is_empty() || !name.iter().all(u8::is_ascii_graphic) || !rest.is_empty() {
                return None;
            }

            (AUTH_ANSWER, vec![AUTH_GRANTED, 0, 0])
        }
        // One string of pairs, ending where the frame does.
        REGISTER_REQUEST => {
            let (_, rest) = string_at(body)?;

            if !rest.is_empty() {
                return None;
            }

            let mut payload = vec![0u8; 1 + REGISTER_UNREAD + 2];
            payload[0] = REGISTER_GRANTED;

            (REGISTER_ANSWER, payload)
        }
        // The subscriber's number and a `u16`, and nothing else. Digits are
        // what say the frame is that request rather than some other title's
        // thirteen bytes under a command as plain as zero.
        SESSION_REQUEST if body.len() == SESSION_BODY && body[..11].iter().all(u8::is_ascii_digit) => {
            let mut payload = vec![0u8; 1 + 2 + SESSION_TRAILER];
            payload[0] = SESSION_GRANTED;

            (SESSION_ANSWER, payload)
        }
        // A `u32` code and the byte behind it, which is what says the frame is
        // `0x7390`'s rather than five other bytes under this command.
        ERRAND_REQUEST if body.len() == ERRAND_BODY && body[4] == ERRAND_TAIL => (ERRAND_ANSWER, vec![AUTH_GRANTED, 0, 0]),
        _ => return None,
    };

    let length = HEADER + payload.len();
    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(&(length as u32).to_le_bytes());
    response.extend_from_slice(&MARKER.to_le_bytes());
    response.extend_from_slice(&answer.to_le_bytes());
    response.extend_from_slice(&payload);

    Some(response)
}

/// 바이오크로니클's login, answered the way its own reader reads it.
///
/// The title opens a `MC_netBillSocket` and writes one frame before it will
/// leave 처리중, then repeats a second one every three seconds while it waits.
/// `0x3050c` is the one place a frame's header is written and `0x34cd0` the one
/// place it is read, and between them the shape is:
///
/// ```text
/// [0..4]    u32 BE - the whole frame's length, the only field this end first
/// [4..8]    u32 LE - that length less these four bytes
/// [8..12]   u32 LE - the command
/// [12..16]  u32 LE - the session, which the login's answer is what issues
/// [16]      u8     - zero
/// [17..21]  u32 LE - 123456789, a constant the title carries at 0x30550
/// [21..]             the body
/// ```
///
/// The reader takes the first four bytes, swaps them through `0x305b4`, waits
/// for that many, and hands everything past them to a queue its main loop at
/// `0x441ac` walks. That loop takes the command out of the record and jumps
/// through the table at `0x5f494`, which has an entry for each command from 1
/// to 0x44 - and every one of them is a request's command plus one. The login
/// is command `0`:
///
/// ```text
/// 00 00 00 7a 76 00 00 00 00 00 00 00 00 00 00 00 00 15 cd 5b 07 00 03 02
/// "01055452383" ... "Emulator" ...
/// ```
///
/// So its answer is command `1`, and `0x44368` reads none of the body: it
/// takes the session out of the header, keeps it at `[0x150eee4]`, and every
/// frame the title writes afterwards carries it. A session of zero is what it
/// has already, so the answer issues one.
///
/// With a session in hand the title sends command `0x36`, four bytes of body:
///
/// ```text
/// 00 00 00 19 15 00 00 00 36 00 00 00 01 00 00 00 00 15 cd 5b 07 3a 9d 4f 7f
/// ```
///
/// Its answer is command `0x37`, and `0x45a46` takes two `u32` out of the body
/// and compares neither against anything the frame carries: the first has to be
/// **0**, or the title takes its failure branch, and the second it keeps at
/// `[0x1504be4]` before advancing to state 6. So that answer is two zeroes.
///
/// The frame it repeats while waiting is command `8`, whose answer `0x45052`
/// walks a table of sessions rather than the title's own - it is other players,
/// not this walk - so it is left alone.
///
/// `None` for anything that is not one of those two: it has to declare its
/// length at both ends, carry the constant, and be a command whose reader's
/// shape is known.
pub fn lgt_local_biochronicle_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length twice, the command, the session, a zero and the constant.
    const HEADER: usize = 21;
    /// What `0x30550` holds and `0x3050c` writes into every frame.
    const CONSTANT: u32 = 123_456_789;

    const LOGIN_REQUEST: u32 = 0;
    const LOGIN_ANSWER: u32 = 1;

    /// What the title sends once it has a session, and `0x45a46` reads the
    /// answer of.
    const READY_REQUEST: u32 = 0x36;
    const READY_ANSWER: u32 = 0x37;
    /// The result `0x45a46` goes on from, and the value it keeps behind it.
    const READY_BODY: usize = 8;

    /// The session `0x44368` keeps and the title then carries. Anything but the
    /// zero it starts with; nothing compares it against anything else.
    const SESSION: u32 = 1;

    fn u32_le(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
    }

    if request.len() < HEADER {
        return None;
    }

    let declared = u32::from_be_bytes(request[0..4].try_into().ok()?) as usize;
    if declared != request.len() || u32_le(request, 4) as usize != request.len() - 4 {
        return None;
    }

    if u32_le(request, 17) != CONSTANT {
        return None;
    }

    // The login is what issues a session; everything after it carries the one
    // it was issued, and is answered under the same.
    let (answer, session, body): (u32, u32, Vec<u8>) = match u32_le(request, 8) {
        LOGIN_REQUEST => (LOGIN_ANSWER, SESSION, Vec::new()),
        READY_REQUEST => (READY_ANSWER, u32_le(request, 12), vec![0u8; READY_BODY]),
        _ => return None,
    };

    let length = HEADER + body.len();
    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(&(length as u32).to_be_bytes());
    response.extend_from_slice(&((length - 4) as u32).to_le_bytes());
    response.extend_from_slice(&answer.to_le_bytes());
    response.extend_from_slice(&session.to_le_bytes());
    response.push(0);
    response.extend_from_slice(&CONSTANT.to_le_bytes());
    response.extend_from_slice(&body);

    Some(response)
}

/// 데스티니아's certificate, answered the way its own reader reads it.
///
/// The title opens a `MC_netBillSocket` and writes one frame, seventy-one
/// bytes, before it will go on:
///
/// ```text
/// 47 00 0a 01 "01046119269" ... "Emulator" ... "1.0.1" ... 38 50 00 00 ...
/// ```
///
/// Its frames carry a four-byte header, both fields little end first - the
/// length, counting the header, and then the kind. `0x3290` is the one place a
/// reply's is read: it takes the length at `[0..2]`, keeps the kind at `[2..4]`,
/// subtracts the four it already holds, and reads that many more before handing
/// the message to `0x7ab0`.
///
/// That handler takes a **signed byte** off the front of the body and stops on
/// a negative one; then it switches on the kind, and `0x010b` - the request's
/// kind and one - is the one that goes on. It reads **forty bytes** and then
/// **one more**, and does not check the forty against anything: `0x794c` lays
/// them beside twelve bytes of its own and four more, runs the lot through
/// `0x7858`, and writes it to `certi.crc`. What it returns is whether that file
/// took more than nothing, not whether the bytes were right.
///
/// So the answer is a granted zero, forty bytes for the title to keep, and the
/// byte behind them - which picks between two states the title goes on in, and
/// is zero here for the plainer of the two.
///
/// Having kept them the title opens a second socket and asks the other kind
/// `0x7ab0` knows, `0x0200`, twenty bytes of the subscriber's number and four
/// more:
///
/// ```text
/// 14 00 00 02 "01046119269" 38 50 00 00
/// ```
///
/// Its answer is `0x0201`, and `0x7b74` reads nothing at all past the signed
/// byte every reply starts with: it closes the socket and goes on to state
/// `0xd`. So that answer is the header and that byte.
///
/// This exchange is a title's first run only. `0x78c0` opens `certi.crc` at
/// startup and, when it is there, decodes the sixty bytes back and the title
/// asks for nothing - which is why the log that shows this walk also shows
/// `MC_fsOpen("certi.crc", mode=1) exists=false` just before it.
///
/// `None` for anything that is not one of those two: it has to declare its own
/// length, be a kind whose reader's shape is known, and carry the subscriber's
/// number where these carry it.
pub fn lgt_local_destinia_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length and the kind, both little end first.
    const HEADER: usize = 4;

    /// What the title sends, and what `0x7ab0` reads the answer of.
    const CERTIFICATE_REQUEST: u16 = 0x010a;
    const CERTIFICATE_ANSWER: u16 = 0x010b;
    /// The subscriber's number, the handset, the build and the rest of what
    /// `0x7ab0`'s request carries - a fixed frame.
    const CERTIFICATE_SIZE: usize = 71;

    /// What it asks on the socket it opens next, and `0x7b74` reads the answer
    /// of - which is nothing past the byte every reply starts with.
    const CONFIRM_REQUEST: u16 = 0x0200;
    const CONFIRM_ANSWER: u16 = 0x0201;
    const CONFIRM_SIZE: usize = 20;

    /// The signed byte `0x7ab0` stops on when it is negative.
    const GRANTED: u8 = 0;
    /// The forty bytes `0x794c` writes to `certi.crc` without reading.
    const CERTIFICATE: usize = 40;

    fn u16_le(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
    }

    if request.len() < HEADER + 1 || u16_le(request, 0) as usize != request.len() || !request[4].is_ascii_digit() {
        return None;
    }

    // Every reply starts with the signed byte `0x7ab0` stops on when it is
    // negative; what follows it is whatever that kind's own reader takes.
    let (answer, body): (u16, Vec<u8>) = match (u16_le(request, 2), request.len()) {
        (CERTIFICATE_REQUEST, CERTIFICATE_SIZE) => (CERTIFICATE_ANSWER, vec![0u8; CERTIFICATE + 1]),
        (CONFIRM_REQUEST, CONFIRM_SIZE) => (CONFIRM_ANSWER, Vec::new()),
        _ => return None,
    };

    let length = HEADER + 1 + body.len();
    let mut response = Vec::with_capacity(length);
    response.extend_from_slice(&(length as u16).to_le_bytes());
    response.extend_from_slice(&answer.to_le_bytes());
    response.push(GRANTED);
    response.extend_from_slice(&body);

    Some(response)
}

/// 블레이드마스터3's login, answered the way its own reader reads it.
///
/// The title opens a `MC_netBillSocket` and writes one frame, twenty-eight
/// bytes, and waits:
///
/// ```text
/// 14 00 01 00 "01031768576" 00 00 00 00 68 00 00 00 ba 02 de 24
/// ```
///
/// A length and a kind, both little end first, then the body and four bytes
/// behind it. `0x179d0` is what decides a reply has all arrived and `0x176c0`
/// what reads it, and both pick their shape from the connection's own state at
/// `[0x150bc8c + 8]` rather than from anything the frame carries. The state this
/// exchange runs in reads a `u16` length - `0x17b5e`, the same shape the request
/// is written in - where the two states above it read a `u32` one.
///
/// `0x176c0` then wants three fields and compares two of them:
///
/// ```text
/// u16 - 4, in both of the states that read a length this way
/// u16 - the kind: 1 where the reply's third field is kept, 2 where it must be 0
/// ?   - the third field, which the first of those two stores at [0x150002c+0x18]
/// ```
///
/// So the answer is that `4`, the kind the request came under, and zeroes -
/// which the state that keeps the third field keeps as nothing, and the state
/// that checks it accepts.
///
/// `None` for anything that is not that login: it has to declare its own length,
/// be that kind, and carry the subscriber's number where this one carries it.
pub fn lgt_local_blademaster3_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length and the kind.
    const HEADER: usize = 4;
    /// What the title writes, and what `0x176c0` reads the answer of.
    const LOGIN_REQUEST: usize = 28;
    const LOGIN_BODY: u16 = 20;
    const LOGIN_KIND: u16 = 1;

    /// The first field `0x17710` and `0x17740` both insist on.
    const MARK: u16 = 4;
    /// The kind, the third field and the four bytes every frame ends with.
    const ANSWER_BODY: u16 = 4;

    fn u16_le(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
    }

    if request.len() != LOGIN_REQUEST || u16_le(request, 0) != LOGIN_BODY || u16_le(request, 2) != LOGIN_KIND {
        return None;
    }

    if !request[HEADER].is_ascii_digit() {
        return None;
    }

    let mut response = Vec::with_capacity(12);
    response.extend_from_slice(&ANSWER_BODY.to_le_bytes());
    response.extend_from_slice(&MARK.to_le_bytes());
    response.extend_from_slice(&LOGIN_KIND.to_le_bytes());
    response.extend_from_slice(&[0u8; 6]);

    Some(response)
}

/// The answer to the id-framed request 짜요짜요타이쿤4 opens with.
///
/// 짜요짜요타이쿤4 (`0002AB99`) opens a billing socket and writes a 36-byte
/// record that is not one of the `0xffff`-framed messages above:
///
/// ```text
/// [0..4]   u32 LE - the record's own length, header included
/// [4..8]   u32 LE - the message id
/// [8..12]  u32 LE - 0x33, the protocol revision every request carries
/// [12..36] the request's own fields
/// ```
///
/// Its receive side is the same shape read back. `0x9a4e` waits for more than
/// seven bytes, reads the leading `u32` as the frame's length, and holds the
/// frame back until that many bytes have arrived; `0x9afc` then reads the second
/// `u32` and returns it as the id the dispatcher switches on. So an answer is a
/// length and an id, and nothing else is required of it.
///
/// The id the title opens with is `0x01F00000`, built at `0xf8d4` as
/// `0xf8 << 17`, and its handler at `0xf6f0` is two instructions: it posts
/// `0x01F00000` to the title's own event queue and reads nothing out of the
/// frame. The eight-byte frame that carries just the id is therefore the whole
/// answer, and the smallest one this protocol can express.
///
/// `None` for anything else, including this protocol's other ids - their
/// handlers read fields out of the frame, and what those fields should say is
/// not something to answer with a guess.
pub fn lgt_local_id_framed_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The length and the id ahead of every frame.
    const HEADER: usize = 8;
    /// The revision at `+8` that `0xef7c` and `0xe80c` alike write.
    const REVISION: u32 = 0x33;
    /// What the title asks first, and what its answer is addressed by.
    const OPENING: u32 = 0x01F0_0000;
    /// The whole of that request: the header, the revision, and its fields.
    /// Those fields are not held to anything - the capture that named this
    /// request had them zero and the emulator's own run has one of them at 1,
    /// so they are the request's arguments rather than part of its shape.
    const OPENING_REQUEST: usize = 36;

    fn u32_le(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
    }

    if request.len() != OPENING_REQUEST || u32_le(request, 0) != OPENING_REQUEST as u32 {
        return None;
    }
    if u32_le(request, 4) != OPENING || u32_le(request, 8) != REVISION {
        return None;
    }

    let mut response = Vec::with_capacity(HEADER);
    response.extend_from_slice(&(HEADER as u32).to_le_bytes());
    response.extend_from_slice(&OPENING.to_le_bytes());

    Some(response)
}

/// The answer to the `LGT`-tagged purchase 테라-영원의혼돈 writes.
///
/// 테라-영원의혼돈 (`0002B76D`) parks on `전송 중입니다..` the moment a shop
/// purchase is confirmed, and then draws `연결 상태가 원활하지 않습니다`. What it
/// wrote is a 111-byte record of its own, not one of the framed messages above:
///
/// ```text
/// [0..3]    "LGT"
/// [3..14]   the header: the message it is, and the item's price as a u32 LE
/// [14..65]  the item, as ASCII - the title's own app id and the item's code,
///           then its name in EUC-KR, zero filled
/// [65..111] the shop it is, the same way
/// ```
///
/// The reply it waits for is not that shape at all. Its receive is a two-step
/// state machine at `0x128d10`: state 4 reads exactly four bytes, state 5 reads
/// those four back as a `u32` little-endian through `0x2847c` and waits for that
/// many more. So an answer is a length and a body.
///
/// What the body has to say is decided by the transaction the shop set, not by
/// the request byte. `0x118e5e` jumps through a table at `0x177f44` indexed by
/// that transaction, and each entry opens by comparing the body's first byte
/// against the one reply it takes; anything else is `연결 상태가 원활하지
/// 않습니다`. `0x121e3c` is the 구매하시겠습니까 dialog - transaction `0xd4`,
/// request byte `0x14` - and pressing 예 on it runs `0x121e9a`, which sets the
/// transaction to `0xd3`, keeps the request byte at `0x14`, and starts the
/// exchange. Index `0xd3 - 0x64` is `0x11a2f8`, and that handler reads exactly
/// two bytes:
///
/// - `[0]` must be `0x14`, the request byte come back. `0x11ac56` takes anything
///   else and is the notice.
/// - `[1]` must be `1`. `0x11ac1e` takes anything else, and puts that byte
///   straight into the status word the notice is chosen from.
///
/// On both it sets the flag at `0x150fe0d` that says the exchange succeeded and
/// clears that status word itself at `0x11a394`, so `0x118762` draws `구매가
/// 완료되었습니다`. Nothing behind those two bytes is read, so there is nothing
/// behind them to send: the title already knows the item it asked for.
///
/// `None` for anything that is not that record: it has to carry the tag, be the
/// length this message is, name an app id where this one does, and be the one
/// request byte whose answer is known. Another of this title's transactions
/// takes a different first byte, and answering it with this one would be
/// answering it wrongly.
pub fn lgt_local_tera_response(request: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"LGT";
    /// The whole of the purchase record, header and both fixed-width fields.
    const PURCHASE_REQUEST: usize = 111;
    /// The request byte at `[3]`, which comes back as the reply's first byte.
    const PURCHASE_AT: usize = 3;
    const PURCHASE: u8 = 0x14;
    /// Where the item field starts, and the app id at the head of it.
    const APP_ID_AT: usize = 14;
    const APP_ID_LEN: usize = 8;

    /// The one status `0x11a30c` reads as the purchase having gone through.
    const GRANTED: u8 = 1;

    if request.len() != PURCHASE_REQUEST || !request.starts_with(TAG) || request[PURCHASE_AT] != PURCHASE {
        return None;
    }
    if !request[APP_ID_AT..APP_ID_AT + APP_ID_LEN].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }

    let body = [PURCHASE, GRANTED];

    let mut response = Vec::with_capacity(4 + body.len());
    response.extend_from_slice(&(body.len() as u32).to_le_bytes());
    response.extend_from_slice(&body);

    Some(response)
}

/// The answers to 영웅서기5's shop and 창고, whose gateway is the one they open.
///
/// 영웅서기5 (`00032870`) connects to `210.222.18.28:18182` when the shop or the
/// 창고 opens. Everything about the protocol is big-endian, and a reply is the
/// same shape as the request that asked for it:
///
/// ```text
/// [0..4]    u32  the frame's own length, which has to be the bytes on the wire
/// [4..12]   the eight-byte service code, "G1000157"
/// [12..16]  u32  command, 0..8
/// [16..20]  u32  sub-command
/// [20..]    fields: a u32, or a u32 length and that many bytes
/// ```
///
/// The title is native, so all of this is ARM. `0x385b8` is the socket callback:
/// on data it reads the length at `[0]`, drops the frame unless it equals what
/// arrived, and queues it. `0x38498` takes it off the queue, drops anything
/// under 20 bytes, sets its cursor to `[12]` and reads the command and the
/// sub-command, then dispatches through the nine-entry table at `0x111ee8`.
/// Each command has its own sub-command table.
///
/// Every step reads the same two things first - a result where only zero is not
/// an error, and a length-prefixed message the error paths draw - and then
/// whatever else that step wants. A zero length reads nothing and moves the
/// cursor nowhere, so each step's smallest answer is its own field count of
/// zeroes, and that is what these are:
///
/// | command | sub | the fields the handler reads | it then asks |
/// |---------|-----|------------------------------|--------------|
/// | 0 | 2 | nothing at all (`0x33e58` answers only sub 1) | — |
/// | 1 | 1 | result, message, a value it keeps as value * 1000 (`0x37cec`) | 1/5, or 1/3 or 1/2 |
/// | 1 | 3 | result, message, a second blob it keeps 15 bytes of (`0x37ef6`) | 5/1, or a greeting |
/// | 4 | 1 | result, message (`0x362a4`) | 4/6 |
/// | 4 | 3 | result, message, the item being handed back | 4/6 |
/// | 4 | 6 | result, message, a row count and the rows (`0x365f4`) | 4/7 |
/// | 4 | 7 | result, message, a `u32` it puts on the screen (`0x36710`) | — |
/// | 5 | 1 | result, message (`0x360e8`) | 5/2 |
/// | 5 | 2 | result, message, a `u32` it stores (`0x36146`) | — |
/// | 6 | 2 | result, message (`0x35b0a`) | 6/3 |
/// | 6 | 3 | result, message, a row count and the rows (`0x35b88`) | — |
/// | 7 | 1 | result, message, a text the title draws (`0x33b94`) | — |
///
/// 4/1, 4/3, 4/6 and 4/7 are 창고관리: a deposit, a withdrawal, the listing of
/// what the 창고 is holding - whose rows are the same records 6/3 delivers - and
/// then the trade currency the 창고 screen prints. A granted deposit takes the
/// item out of the title's own bag and a granted withdrawal puts back whatever
/// the answer carries, so what was deposited has to be kept: see
/// [`HERO5_WAREHOUSE`]. 0/2 is the keep-alive `0x392b8` sends on a timer of its own
/// rather than an exchange the title is waiting on, and 7/1 is what the 창고
/// asks for once its greeting is dismissed - the one request whose builder (`0x340c8`) leaves the
/// waiting flag at `ctx + 0xb` clear, so `0x35824` puts the "Recieve" progress
/// dialog up and nothing but an answer takes it down again.
///
/// The two the title actually opens with are 1/1, which both the shop and the
/// 창고 send on connecting, and then 6/2 for a purchase or 1/3 for the 창고.
/// Answering 1/1 was enough to move both of them on to those, which is how the
/// rest of this table was found: an accepted answer is not silent, it produces
/// the next request, and the next request names the step to read.
///
/// A row count of zero is an empty list rather than a broken one - `0x35c1a`
/// compares the row it is on against the count before reading anything, so
/// nothing is read - and the same holds for 5/2's own list at `0x361a0`.
///
/// Three fields here are not zero, because zero is a wrong answer there rather
/// than an empty one: 1/1's keep-alive interval, which zero makes "every tick";
/// 1/3's nickname, which the 창고 greets by name; and 6/3's list, which is the
/// purchase being handed to the bag rather than a receipt for it, so an empty
/// one is a purchase that never arrives. See [`HERO5_PING_SECONDS`],
/// [`HERO5_SUBSCRIBER`] and [`hero5_delivery`].
///
/// `None` for everything else: the frame has to declare its own length, carry
/// this title's service code, and be one of the steps above. The commands whose
/// handlers have not been read are left unanswered rather than guessed - a
/// command answered wrongly does not leave the title waiting, it puts it through
/// a branch meant for a different exchange.
/// How often, in seconds, 영웅서기5 is told to send its keep-alive.
///
/// `0x37cec` keeps 1/1's third field as `value * 1000` in `ctx + 0x14`, and
/// `0x392b8` writes a 0/2 frame once the clock is past the last request plus
/// half of that. Zero is therefore not "no keep-alive", it is one per tick -
/// which is what the 창고 capture is nearly all of, 26 pings a second.
const HERO5_PING_SECONDS: u32 = 60;

/// The bytes of 1/3's third blob the 창고 greets by name.
///
/// Both of `0x37ef6`'s paths read the blob into a cleared 0x28 buffer and copy
/// exactly this many of it to `ctx + 0x351`; `0x38044` then formats that into
/// the "님 반갑습니다." line. A zero-length blob reads nothing, which is why the
/// greeting in the capture had no name in front of it.
const HERO5_NICKNAME: usize = 15;

/// The subscriber number 영웅서기5 puts in its 1/1 frame, kept for 1/3.
///
/// There is no account here to hold a nickname, and 1/3 is a bare header, so the
/// only name to greet with is the one the title itself supplied one step
/// earlier. Empty until a 1/1 has gone by, which in both captured flows is the
/// first frame on the socket.
static HERO5_SUBSCRIBER: spin::Mutex<Vec<u8>> = spin::Mutex::new(Vec::new());

/// The subscriber number out of a 영웅서기5 1/1 frame.
///
/// The first field after the header: a length and that many bytes. The title
/// pads it to 16 with a NUL and whatever was after it in memory, so the number
/// is what comes before the NUL.
fn hero5_subscriber(request: &[u8]) -> Vec<u8> {
    const FIELDS_AT: usize = 20;

    let Some(length) = request.get(FIELDS_AT..FIELDS_AT + 4) else {
        return Vec::new();
    };
    let length = u32::from_be_bytes(length.try_into().unwrap()) as usize;

    let Some(number) = request.get(FIELDS_AT + 4..FIELDS_AT + 4 + length) else {
        return Vec::new();
    };
    let number = &number[..number.iter().position(|&byte| byte == 0).unwrap_or(number.len())];

    number[..number.len().min(HERO5_NICKNAME)].to_vec()
}

/// The table of 영웅서기5's `res/c/csv/item_18.dat`, as `0xe748` numbers them.
///
/// That call takes a table and a row and hands back a name; it checks the table
/// against 0x12 and indexes a nineteen-entry jump table, one per `item_NN.dat`.
/// 18 is the last of them and everything the shop sells is in it - every name in
/// the three catalogues is in that file and in no other.
const HERO5_ITEM_TABLE: u8 = 18;

/// What 영웅서기5's shop hands over for each of its own product ids.
///
/// The catalogue is in the title, not on the wire: `res/c/csv/cash_single.dat`,
/// `cash_network.dat` and `cash_expert.dat` carry a product id, a name, a price
/// and an `x` and a `y`, and a purchase names the product by that id - 4 for
/// 엘릭서(20), 18 to 21 for the four 유물함. `x` is the `item_18.dat` row the
/// product is and `y` how many of it: 엘릭서(20) is row 34 twenty times,
/// 작은 유물함 row 15 once, 달성의부적(5) row 9 five times. Every row of all
/// three catalogues lines up that way, and the three agree wherever they
/// overlap, so this is their union.
///
/// (product id, `item_18.dat` row, how many).
const HERO5_SHOP_ROWS: [(u32, u8, u32); 42] = [
    (0, 0, 1),   // 창고확장(nt)
    (1, 1, 1),   // 프리미엄판매권
    (2, 2, 1),   // 기간연장(7일)
    (3, 3, 1),   // 기간제한해제
    (4, 34, 20), // 엘릭서(20)
    (5, 5, 1),   // 작은오브원석
    (6, 6, 1),   // 오브원석
    (7, 7, 1),   // 하이퍼오브
    (8, 8, 1),   // 고급제련석
    (9, 9, 1),   // 달성의부적
    (10, 9, 5),  // 달성의부적(5)
    (11, 10, 1), // 안전의부적
    (12, 10, 5), // 안전의부적(5)
    (13, 11, 1), // 역행의 기원
    (14, 12, 1), // 소켓확장
    (15, 13, 1), // 복원의 서
    (16, 13, 3), // 복원의 서(5)
    (17, 14, 1), // 창고 확장
    (18, 15, 1), // 작은 유물함
    (19, 16, 1), // 유물함
    (20, 17, 1), // 큰 유물함
    (21, 18, 1), // 오래된 유물함
    (22, 19, 1), // 특성 초기화
    (23, 20, 1), // 스탯 초기화
    (24, 21, 1), // 초기화 세트
    (26, 23, 1), // 부활의 부적
    (27, 23, 5), // 부활의 부적(5)
    (28, 24, 1), // 오토루팅
    (29, 25, 1), // 성장의 서
    (30, 26, 3), // 환생의 서(3)
    (31, 27, 3), // 장갑의 서(3)
    (32, 28, 3), // 시간의 서(3)
    (33, 29, 3), // 집중의 서(3)
    (34, 30, 1), // 보호의 부적(3)
    (35, 31, 1), // 마석
    (36, 35, 3), // 포도주(3)
    (37, 36, 5), // 천사의 날개(5)
    (41, 38, 1), // 환전한도증가
    (42, 39, 1), // 워리어의 혼
    (43, 40, 1), // 로그의 혼
    (44, 41, 1), // 건슬링어의혼
    (45, 42, 1), // 나이트의 혼
];

/// How many bytes of stats a row of an equipment table carries.
///
/// `item_00.dat` through `item_10.dat` all parse end to end as a count and then,
/// per row, a `u16` pair, a length and a name, a `u32` of what the game charges,
/// a length and a description, and these - which are what the item is. They land
/// in the item the title builds at `+0x14c` through `+0x160`, one for one, which
/// is where [`hero5_equipment_tail`] takes them back out of.
const HERO5_TABLE_STATS: usize = 21;

/// What each 영웅서기5 유물함 draws from.
///
/// The thirteenth byte of an equipment row is its grade, and the whole of
/// `item_00.dat` through `item_10.dat` sorts by it. What the numbers are called
/// is in `menu_text.dat` 87 to 92 - 노멀, 레어, 에픽, 영웅, 전설, 세트 - and
/// again in `common_text.dat` 215 to 219 with the colour tag the name is drawn
/// in in front of each: `&레어|`, `}에픽|`, `` `영웅| ``, `^전설|`, `<세트|`.
/// So the grades run 0 노멀, 1 레어, 2 에픽, 3 영웅, 4 전설, and 5 upward a set.
///
/// The tables hold no 레어 at all: 362 rows are 노멀, the six 에픽 are all
/// 장신구, **252 are 영웅** (the named pieces - 스톰브링거, 팬텀아머), **81 are
/// 전설** (the ones with a title - 그람:궁극의힘, 라그나블로커), and **5 to 18
/// are the fourteen sets** `res/c/csv/set_option.dat` names, four pieces each.
///
/// Each row carries its own options in its last six bytes as three
/// (option, value) pairs - 팬텀의부적 is option 17 at 5, option 21 at 2 and
/// option 18 at 5 - so a piece goes over as the piece the title itself would
/// drop, grade and options and set bonus and all, with nothing invented for it.
/// The first pass handed over grade-0 rows, which is why it was 껍데기.
///
/// (item table, the row in it, that row's [`HERO5_TABLE_STATS`] bytes).
const HERO5_BOX_TRINKETS: [(u8, u8, [u8; HERO5_TABLE_STATS]); 16] = [
    (
        10,
        0,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x22, 0x00, 0x02, 0x01, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        ],
    ), // 장신구 고렘의인장 (lv1)
    (
        10,
        1,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x22, 0x00, 0x02, 0x01, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        ],
    ), // 장신구 데몬의뿔 (lv1)
    (
        10,
        2,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x20, 0x07, 0x23, 0x04, 0x12, 0x05,
        ],
    ), // 장신구 콘돌의깃털 (lv1)
    (
        10,
        3,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x11, 0x05, 0x15, 0x02, 0x12, 0x05,
        ],
    ), // 장신구 팬텀의부적 (lv1)
    (
        10,
        4,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x24, 0x00, 0x04, 0x01, 0x00, 0x2e, 0x07, 0x0e, 0x06, 0x12, 0x05,
        ],
    ), // 장신구 기사의징표 (lv1)
    (
        10,
        5,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x22, 0x00, 0x02, 0x01, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        ],
    ), // 장신구 독사의이빨 (lv1)
    (
        10,
        6,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x22, 0x00, 0x02, 0x01, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        ],
    ), // 장신구 쟈칼의발톱 (lv1)
    (
        10,
        7,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x20, 0x0d, 0x23, 0x08, 0x12, 0x0a,
        ],
    ), // 장신구 공작의깃털 (lv1)
    (
        10,
        8,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x11, 0x0a, 0x15, 0x04, 0x12, 0x0a,
        ],
    ), // 장신구 오우거의사슬 (lv1)
    (
        10,
        9,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x24, 0x00, 0x04, 0x01, 0x00, 0x2e, 0x0d, 0x0e, 0x0b, 0x12, 0x0a,
        ],
    ), // 장신구 스폰의사슬 (lv1)
    (
        10,
        10,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x22, 0x00, 0x02, 0x01, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        ],
    ), // 장신구 샌드웜의비늘 (lv1)
    (
        10,
        11,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x22, 0x00, 0x02, 0x01, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        ],
    ), // 장신구 시바스아뮬렛 (lv1)
    (
        10,
        12,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x20, 0x13, 0x23, 0x0b, 0x12, 0x0e,
        ],
    ), // 장신구 기간틱아뮬렛 (lv1)
    (
        10,
        13,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x11, 0x0e, 0x15, 0x06, 0x12, 0x0e,
        ],
    ), // 장신구 배리어이어링 (lv1)
    (
        10,
        14,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x24, 0x00, 0x04, 0x01, 0x00, 0x2e, 0x13, 0x0e, 0x11, 0x12, 0x0e,
        ],
    ), // 장신구 홀리즈이어링 (lv1)
    (
        10,
        15,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x00, 0x01, 0x00, 0x24, 0x00, 0x04, 0x01, 0x00, 0x1e, 0x0a, 0x20, 0x16, 0x29, 0x0e,
        ],
    ), // 장신구 카오스이어링 (lv1)
];

const HERO5_BOX_HEROIC: [(u8, u8, [u8; HERO5_TABLE_STATS]); 252] = [
    (
        0,
        17,
        [
            0x01, 0x00, 0x26, 0x01, 0x01, 0x00, 0x31, 0x00, 0x5c, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x29, 0x02, 0x2b, 0x05, 0xff, 0x00,
        ],
    ), // 검 스톰브링거 (lv12)
    (
        0,
        18,
        [
            0x08, 0x00, 0x3b, 0x01, 0x00, 0x00, 0x19, 0x00, 0x74, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x0f, 0x02, 0x1a, 0x02, 0xff, 0x00,
        ],
    ), // 검 실가라스 (lv12)
    (
        0,
        19,
        [
            0x02, 0x00, 0x29, 0x01, 0x01, 0x00, 0x47, 0x00, 0x84, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x12, 0x03, 0x10, 0x04, 0xff, 0x00,
        ],
    ), // 검 투란기어 (lv21)
    (
        0,
        20,
        [
            0x09, 0x00, 0x3e, 0x01, 0x00, 0x00, 0x23, 0x00, 0xa7, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x30, 0x01, 0x21, 0x03, 0xff, 0x00,
        ],
    ), // 검 캘라보그 (lv21)
    (
        0,
        21,
        [
            0x03, 0x00, 0x2c, 0x01, 0x01, 0x00, 0x5a, 0x00, 0xa7, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x1c, 0x03, 0x31, 0x04, 0xff, 0x00,
        ],
    ), // 검 바리사다 (lv29)
    (
        0,
        22,
        [
            0x0a, 0x00, 0x41, 0x01, 0x00, 0x00, 0x2d, 0x00, 0xd4, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x21, 0x03, 0x16, 0x04, 0xff, 0x00,
        ],
    ), // 검 지옥수호도끼 (lv29)
    (
        0,
        23,
        [
            0x04, 0x00, 0x2f, 0x01, 0x01, 0x00, 0x72, 0x00, 0xd4, 0x00, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x13, 0x14, 0x21, 0x05, 0xff, 0x00,
        ],
    ), // 검 알비스 (lv39)
    (
        0,
        24,
        [
            0x0b, 0x00, 0x44, 0x01, 0x00, 0x00, 0x39, 0x00, 0x0d, 0x01, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x2d, 0x0a, 0x2e, 0x0a, 0xff, 0x00,
        ],
    ), // 검 바르마사 (lv39)
    (
        0,
        35,
        [
            0x02, 0x00, 0x29, 0x01, 0x01, 0x00, 0x80, 0x00, 0xef, 0x00, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x2d, 0x0c, 0x12, 0x06, 0xff, 0x00,
        ],
    ), // 검 붉은사자대검 (lv45)
    (
        0,
        36,
        [
            0x08, 0x00, 0x3b, 0x01, 0x00, 0x00, 0x40, 0x00, 0x2f, 0x01, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x10, 0x07, 0x10, 0x09, 0xff, 0x00,
        ],
    ), // 검 푸른비룡도끼 (lv45)
    (
        0,
        37,
        [
            0x03, 0x00, 0x2c, 0x01, 0x01, 0x00, 0x8a, 0x00, 0x00, 0x01, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x29, 0x05, 0x0f, 0x09, 0xff, 0x00,
        ],
    ), // 검 그라디우스 (lv49)
    (
        0,
        38,
        [
            0x09, 0x00, 0x3e, 0x01, 0x00, 0x00, 0x45, 0x00, 0x45, 0x01, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x26, 0x05, 0x2e, 0x0c, 0xff, 0x00,
        ],
    ), // 검 알테나워액스 (lv49)
    (
        0,
        39,
        [
            0x04, 0x00, 0x2f, 0x01, 0x01, 0x00, 0x96, 0x00, 0x17, 0x01, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x12, 0x06, 0x11, 0x0a, 0xff, 0x00,
        ],
    ), // 검 디바인세이버 (lv54)
    (
        0,
        40,
        [
            0x0a, 0x00, 0x41, 0x01, 0x00, 0x00, 0x4b, 0x00, 0x62, 0x01, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x2d, 0x0e, 0x12, 0x07, 0xff, 0x00,
        ],
    ), // 검 발카리안 (lv54)
    (
        0,
        41,
        [
            0x05, 0x00, 0x31, 0x01, 0x01, 0x00, 0xa2, 0x00, 0x2d, 0x01, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x0f, 0x09, 0x30, 0x01, 0xff, 0x00,
        ],
    ), // 검 피어스 (lv59)
    (
        0,
        42,
        [
            0x0c, 0x00, 0x46, 0x01, 0x00, 0x00, 0x51, 0x00, 0x7e, 0x01, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x2d, 0x0f, 0x2d, 0x12, 0xff, 0x00,
        ],
    ), // 검 바테우간 (lv59)
    (
        0,
        61,
        [
            0x02, 0x00, 0x29, 0x01, 0x01, 0x00, 0xae, 0x00, 0x43, 0x01, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x29, 0x07, 0x30, 0x01, 0xff, 0x00,
        ],
    ), // 검 파괴군주검 (lv64)
    (
        0,
        62,
        [
            0x08, 0x00, 0x3b, 0x01, 0x00, 0x00, 0x57, 0x00, 0x9a, 0x01, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x2d, 0x11, 0x26, 0x08, 0xff, 0x00,
        ],
    ), // 검 디스트로이어 (lv64)
    (
        0,
        63,
        [
            0x03, 0x00, 0x2c, 0x01, 0x01, 0x00, 0xc8, 0x00, 0x74, 0x01, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x0f, 0x0c, 0x1e, 0x0a, 0xff, 0x00,
        ],
    ), // 검 패왕신검 (lv75)
    (
        0,
        64,
        [
            0x09, 0x00, 0x3e, 0x01, 0x00, 0x00, 0x64, 0x00, 0xd8, 0x01, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x10, 0x0c, 0x2e, 0x13, 0xff, 0x00,
        ],
    ), // 검 미명의도끼 (lv75)
    (
        0,
        65,
        [
            0x04, 0x00, 0x2f, 0x01, 0x01, 0x00, 0xde, 0x00, 0x9c, 0x01, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x21, 0x09, 0x12, 0x0b, 0xff, 0x00,
        ],
    ), // 검 천황패도 (lv84)
    (
        0,
        66,
        [
            0x0a, 0x00, 0x41, 0x01, 0x00, 0x00, 0x6f, 0x00, 0x0b, 0x02, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x26, 0x09, 0x21, 0x0b, 0xff, 0x00,
        ],
    ), // 검 암흑폭풍도끼 (lv84)
    (
        0,
        67,
        [
            0x05, 0x00, 0x31, 0x01, 0x01, 0x00, 0xfb, 0x00, 0xd1, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x1c, 0x0a, 0x26, 0x0c, 0xff, 0x00,
        ],
    ), // 검 헬리시온 (lv96)
    (
        0,
        68,
        [
            0x0b, 0x00, 0x43, 0x01, 0x00, 0x00, 0x7d, 0x00, 0x4f, 0x02, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x1e, 0x0a, 0x1e, 0x0c, 0xff, 0x00,
        ],
    ), // 검 아스카론 (lv96)
    (
        0,
        69,
        [
            0x05, 0x00, 0x32, 0x01, 0x01, 0x00, 0xfb, 0x00, 0xd1, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x2d, 0x19, 0x13, 0x3a, 0xff, 0x00,
        ],
    ), // 검 앤서러 (lv96)
    (
        0,
        70,
        [
            0x0b, 0x00, 0x44, 0x01, 0x00, 0x00, 0x7d, 0x00, 0x4f, 0x02, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x12, 0x0a, 0x13, 0x3a, 0xff, 0x00,
        ],
    ), // 검 그라티스 (lv96)
    (
        1,
        17,
        [
            0x11, 0x00, 0x56, 0x01, 0x01, 0x01, 0x30, 0x00, 0x3e, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x21, 0x02, 0x21, 0x02, 0xff, 0x00,
        ],
    ), // 단검 데몬브레이커 (lv12)
    (
        1,
        18,
        [
            0x18, 0x00, 0x6b, 0x01, 0x00, 0x01, 0x25, 0x00, 0x4a, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x1c, 0x02, 0x1e, 0x02, 0xff, 0x00,
        ],
    ), // 단검 데몬스퀘이드 (lv12)
    (
        1,
        19,
        [
            0x12, 0x00, 0x59, 0x01, 0x01, 0x01, 0x48, 0x00, 0x5e, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x0f, 0x04, 0x2e, 0x06, 0xff, 0x00,
        ],
    ), // 단검 소울퀘이드 (lv21)
    (
        1,
        20,
        [
            0x19, 0x00, 0x6e, 0x01, 0x00, 0x01, 0x38, 0x00, 0x70, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x1e, 0x03, 0x26, 0x03, 0xff, 0x00,
        ],
    ), // 단검 본시커 (lv21)
    (
        1,
        21,
        [
            0x13, 0x00, 0x5c, 0x01, 0x01, 0x01, 0x5d, 0x00, 0x7b, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x13, 0x0f, 0x13, 0x12, 0xff, 0x00,
        ],
    ), // 단검 켈베로스 (lv29)
    (
        1,
        22,
        [
            0x1a, 0x00, 0x71, 0x01, 0x00, 0x01, 0x48, 0x00, 0x91, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x26, 0x03, 0x21, 0x04, 0xff, 0x00,
        ],
    ), // 단검 지옥무쇠도검 (lv29)
    (
        1,
        23,
        [
            0x14, 0x00, 0x5f, 0x01, 0x01, 0x01, 0x77, 0x00, 0x9f, 0x00, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x1c, 0x04, 0x13, 0x18, 0xff, 0x00,
        ],
    ), // 단검 지옥불꽃단검 (lv39)
    (
        1,
        24,
        [
            0x1b, 0x00, 0x74, 0x01, 0x00, 0x01, 0x5d, 0x00, 0xbc, 0x00, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x21, 0x04, 0x21, 0x05, 0xff, 0x00,
        ],
    ), // 단검 지옥화염도검 (lv39)
    (
        1,
        35,
        [
            0x12, 0x00, 0x59, 0x01, 0x01, 0x01, 0x87, 0x00, 0xb4, 0x00, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x30, 0x01, 0x21, 0x06, 0xff, 0x00,
        ],
    ), // 단검 팬텀브레이커 (lv45)
    (
        1,
        36,
        [
            0x18, 0x00, 0x6b, 0x01, 0x00, 0x01, 0x69, 0x00, 0xd5, 0x00, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x2d, 0x0c, 0x21, 0x06, 0xff, 0x00,
        ],
    ), // 단검 킬레바르드 (lv45)
    (
        1,
        37,
        [
            0x13, 0x00, 0x5c, 0x01, 0x01, 0x01, 0x92, 0x00, 0xc2, 0x00, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x0f, 0x08, 0x1e, 0x06, 0xff, 0x00,
        ],
    ), // 단검 암흑절단대거 (lv49)
    (
        1,
        38,
        [
            0x19, 0x00, 0x6e, 0x01, 0x00, 0x01, 0x72, 0x00, 0xe6, 0x00, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x0e, 0x08, 0x14, 0x0f, 0xff, 0x00,
        ],
    ), // 단검 암흑폭풍도검 (lv49)
    (
        1,
        39,
        [
            0x14, 0x00, 0x5f, 0x01, 0x01, 0x01, 0x9f, 0x00, 0xd4, 0x00, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x0f, 0x09, 0x14, 0x11, 0xff, 0x00,
        ],
    ), // 단검 맹공의룬검 (lv54)
    (
        1,
        40,
        [
            0x1a, 0x00, 0x71, 0x01, 0x00, 0x01, 0x7c, 0x00, 0xfb, 0x00, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x1c, 0x06, 0x1e, 0x07, 0xff, 0x00,
        ],
    ), // 단검 크림슨아이 (lv54)
    (
        1,
        41,
        [
            0x15, 0x00, 0x61, 0x01, 0x01, 0x01, 0xad, 0x00, 0xe6, 0x00, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x30, 0x01, 0x30, 0x01, 0xff, 0x00,
        ],
    ), // 단검 빌스키르니르 (lv59)
    (
        1,
        42,
        [
            0x1c, 0x00, 0x76, 0x01, 0x00, 0x01, 0x86, 0x00, 0x10, 0x01, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x2d, 0x0f, 0x29, 0x08, 0xff, 0x00,
        ],
    ), // 단검 크림슨투스 (lv59)
    (
        1,
        61,
        [
            0x12, 0x00, 0x59, 0x01, 0x01, 0x01, 0xba, 0x00, 0xf8, 0x00, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x26, 0x07, 0x13, 0x27, 0xff, 0x00,
        ],
    ), // 단검 마도학살검 (lv64)
    (
        1,
        62,
        [
            0x18, 0x00, 0x6b, 0x01, 0x00, 0x01, 0x91, 0x00, 0x25, 0x01, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x21, 0x07, 0x13, 0x27, 0xff, 0x00,
        ],
    ), // 단검 마도진혼검 (lv64)
    (
        1,
        63,
        [
            0x13, 0x00, 0x5c, 0x01, 0x01, 0x01, 0xd7, 0x00, 0x1f, 0x01, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x0e, 0x0c, 0x0f, 0x0e, 0xff, 0x00,
        ],
    ), // 단검 비슈느 (lv75)
    (
        1,
        64,
        [
            0x19, 0x00, 0x6e, 0x01, 0x00, 0x01, 0xa7, 0x00, 0x53, 0x01, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x21, 0x08, 0x21, 0x0a, 0xff, 0x00,
        ],
    ), // 단검 심장파괴검 (lv75)
    (
        1,
        65,
        [
            0x14, 0x00, 0x5f, 0x01, 0x01, 0x01, 0xef, 0x00, 0x3f, 0x01, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x2d, 0x16, 0x14, 0x1a, 0xff, 0x00,
        ],
    ), // 단검 플래시크래셔 (lv84)
    (
        1,
        66,
        [
            0x1a, 0x00, 0x71, 0x01, 0x00, 0x01, 0xba, 0x00, 0x79, 0x01, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x0f, 0x0d, 0x0f, 0x10, 0xff, 0x00,
        ],
    ), // 단검 아웃버스트 (lv84)
    (
        1,
        67,
        [
            0x15, 0x00, 0x61, 0x01, 0x01, 0x01, 0x0f, 0x01, 0x6a, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x30, 0x01, 0x21, 0x0c, 0xff, 0x00,
        ],
    ), // 단검 크래비안 (lv96)
    (
        1,
        68,
        [
            0x1b, 0x00, 0x73, 0x01, 0x00, 0x01, 0xd3, 0x00, 0xac, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x1c, 0x0a, 0x2e, 0x18, 0xff, 0x00,
        ],
    ), // 단검 진마도학살검 (lv96)
    (
        1,
        69,
        [
            0x15, 0x00, 0x62, 0x01, 0x01, 0x01, 0x0f, 0x01, 0x6a, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x21, 0x0a, 0x0f, 0x12, 0xff, 0x00,
        ],
    ), // 단검 진마도진혼검 (lv96)
    (
        1,
        70,
        [
            0x1b, 0x00, 0x74, 0x01, 0x00, 0x01, 0xd3, 0x00, 0xac, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x0e, 0x0f, 0x1e, 0x0c, 0xff, 0x00,
        ],
    ), // 단검 크리스터 (lv96)
    (
        2,
        17,
        [
            0x24, 0x00, 0x8f, 0x01, 0x01, 0x02, 0x23, 0x00, 0x2b, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x29, 0x02, 0x26, 0x02, 0xff, 0x00,
        ],
    ), // 총 속사의피마펭 (lv12)
    (
        2,
        18,
        [
            0x2b, 0x00, 0xa4, 0x01, 0x00, 0x02, 0x21, 0x00, 0x4d, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x0f, 0x02, 0x17, 0x02, 0xff, 0x00,
        ],
    ), // 총 울르의방아쇠 (lv12)
    (
        2,
        19,
        [
            0x25, 0x00, 0x92, 0x01, 0x01, 0x02, 0x33, 0x00, 0x3e, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x30, 0x01, 0x0f, 0x04, 0xff, 0x00,
        ],
    ), // 총 파이널버스터 (lv21)
    (
        2,
        20,
        [
            0x2c, 0x00, 0xa7, 0x01, 0x00, 0x02, 0x2f, 0x00, 0x6d, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x1d, 0x03, 0x17, 0x03, 0xff, 0x00,
        ],
    ), // 총 데몬암즈 (lv21)
    (
        2,
        21,
        [
            0x26, 0x00, 0x95, 0x01, 0x01, 0x02, 0x40, 0x00, 0x4e, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x29, 0x03, 0x2e, 0x07, 0xff, 0x00,
        ],
    ), // 총 크라켄웨폰 (lv29)
    (
        2,
        22,
        [
            0x2d, 0x00, 0xaa, 0x01, 0x00, 0x02, 0x3b, 0x00, 0x8a, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x13, 0x0f, 0x26, 0x04, 0xff, 0x00,
        ],
    ), // 총 플라즈마캐논 (lv29)
    (
        2,
        23,
        [
            0x27, 0x00, 0x98, 0x01, 0x01, 0x02, 0x51, 0x00, 0x63, 0x00, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x13, 0x14, 0x23, 0x05, 0xff, 0x00,
        ],
    ), // 총 파이너리슈터 (lv39)
    (
        2,
        24,
        [
            0x2e, 0x00, 0xad, 0x01, 0x00, 0x02, 0x4b, 0x00, 0xaf, 0x00, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x1e, 0x04, 0x17, 0x05, 0xff, 0x00,
        ],
    ), // 총 헬파이어 (lv39)
    (
        2,
        35,
        [
            0x25, 0x00, 0x92, 0x01, 0x01, 0x02, 0x5b, 0x00, 0x6f, 0x00, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x30, 0x01, 0x2e, 0x0b, 0xff, 0x00,
        ],
    ), // 총 블랙카이트 (lv45)
    (
        2,
        36,
        [
            0x2b, 0x00, 0xa4, 0x01, 0x00, 0x02, 0x54, 0x00, 0xc5, 0x00, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x1d, 0x05, 0x29, 0x06, 0xff, 0x00,
        ],
    ), // 총 블랙하피 (lv45)
    (
        2,
        37,
        [
            0x26, 0x00, 0x95, 0x01, 0x01, 0x02, 0x62, 0x00, 0x77, 0x00, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x2d, 0x0d, 0x0f, 0x09, 0xff, 0x00,
        ],
    ), // 총 해리어롱바렐 (lv49)
    (
        2,
        38,
        [
            0x2c, 0x00, 0xa7, 0x01, 0x00, 0x02, 0x5a, 0x00, 0xd3, 0x00, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x22, 0x05, 0x17, 0x06, 0xff, 0x00,
        ],
    ), // 총 썬더버드 (lv49)
    (
        2,
        39,
        [
            0x27, 0x00, 0x98, 0x01, 0x01, 0x02, 0x6a, 0x00, 0x82, 0x00, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x1e, 0x06, 0x1e, 0x07, 0xff, 0x00,
        ],
    ), // 총 팔켄매그넘 (lv54)
    (
        2,
        40,
        [
            0x2d, 0x00, 0xaa, 0x01, 0x00, 0x02, 0x62, 0x00, 0xe5, 0x00, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x22, 0x06, 0x26, 0x07, 0xff, 0x00,
        ],
    ), // 총 호크암즈 (lv54)
    (
        2,
        41,
        [
            0x28, 0x00, 0x9a, 0x01, 0x01, 0x02, 0x73, 0x00, 0x8c, 0x00, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x0f, 0x09, 0x30, 0x01, 0xff, 0x00,
        ],
    ), // 총 데스리미터 (lv59)
    (
        2,
        42,
        [
            0x2f, 0x00, 0xaf, 0x01, 0x00, 0x02, 0x6a, 0x00, 0xf7, 0x00, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x2d, 0x0f, 0x13, 0x24, 0xff, 0x00,
        ],
    ), // 총 유니콘혼 (lv59)
    (
        2,
        61,
        [
            0x25, 0x00, 0x92, 0x01, 0x01, 0x02, 0x7b, 0x00, 0x96, 0x00, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x26, 0x07, 0x2e, 0x10, 0xff, 0x00,
        ],
    ), // 총 제비우스 (lv64)
    (
        2,
        62,
        [
            0x2b, 0x00, 0xa4, 0x01, 0x00, 0x02, 0x72, 0x00, 0x0a, 0x01, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x0f, 0x0a, 0x17, 0x08, 0xff, 0x00,
        ],
    ), // 총 이그니스 (lv64)
    (
        2,
        63,
        [
            0x26, 0x00, 0x95, 0x01, 0x01, 0x02, 0x8d, 0x00, 0xad, 0x00, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x13, 0x26, 0x14, 0x17, 0xff, 0x00,
        ],
    ), // 총 더블엘터 (lv75)
    (
        2,
        64,
        [
            0x2c, 0x00, 0xa7, 0x01, 0x00, 0x02, 0x83, 0x00, 0x32, 0x01, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x26, 0x08, 0x14, 0x17, 0xff, 0x00,
        ],
    ), // 총 알펜로즈 (lv75)
    (
        2,
        65,
        [
            0x27, 0x00, 0x98, 0x01, 0x01, 0x02, 0x9d, 0x00, 0xbf, 0x00, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x23, 0x09, 0x2e, 0x15, 0xff, 0x00,
        ],
    ), // 총 템페스트 (lv84)
    (
        2,
        66,
        [
            0x2d, 0x00, 0xaa, 0x01, 0x00, 0x02, 0x91, 0x00, 0x52, 0x01, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x13, 0x2b, 0x16, 0x0b, 0xff, 0x00,
        ],
    ), // 총 카이져스톰 (lv84)
    (
        2,
        67,
        [
            0x28, 0x00, 0x9a, 0x01, 0x01, 0x02, 0xb1, 0x00, 0xd8, 0x00, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x29, 0x0a, 0x16, 0x0c, 0xff, 0x00,
        ],
    ), // 총 소울크로우 (lv96)
    (
        2,
        68,
        [
            0x2e, 0x00, 0xac, 0x01, 0x00, 0x02, 0xa4, 0x00, 0x7e, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x30, 0x01, 0x26, 0x0c, 0xff, 0x00,
        ],
    ), // 총 스트레인저 (lv96)
    (
        2,
        69,
        [
            0x28, 0x00, 0x9b, 0x01, 0x01, 0x02, 0xb1, 0x00, 0xd8, 0x00, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x29, 0x0a, 0x18, 0x06, 0xff, 0x00,
        ],
    ), // 총 알테나암즈 (lv96)
    (
        2,
        70,
        [
            0x2e, 0x00, 0xad, 0x01, 0x00, 0x02, 0xa4, 0x00, 0x7e, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x1e, 0x0a, 0x16, 0x0c, 0xff, 0x00,
        ],
    ), // 총 진알펜로즈 (lv96)
    (
        3,
        17,
        [
            0x33, 0x00, 0xbc, 0x01, 0x01, 0x03, 0x35, 0x00, 0x41, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x1c, 0x02, 0x2b, 0x05, 0xff, 0x00,
        ],
    ), // 창 오웬스피어 (lv12)
    (
        3,
        18,
        [
            0x3a, 0x00, 0xd1, 0x01, 0x00, 0x03, 0x29, 0x00, 0x4d, 0x00, 0x1a, 0x00, 0x03, 0x0c, 0x00, 0x1c, 0x02, 0x2e, 0x03, 0xff, 0x00,
        ],
    ), // 창 소울랜스 (lv12)
    (
        3,
        19,
        [
            0x34, 0x00, 0xbf, 0x01, 0x01, 0x03, 0x4d, 0x00, 0x5e, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x12, 0x03, 0x2e, 0x06, 0xff, 0x00,
        ],
    ), // 창 엘븐윙스피어 (lv21)
    (
        3,
        20,
        [
            0x3b, 0x00, 0xd4, 0x01, 0x00, 0x03, 0x3c, 0x00, 0x6f, 0x00, 0x1a, 0x00, 0x03, 0x15, 0x00, 0x12, 0x03, 0x1e, 0x03, 0xff, 0x00,
        ],
    ), // 창 클로우할버드 (lv21)
    (
        3,
        21,
        [
            0x35, 0x00, 0xc2, 0x01, 0x01, 0x03, 0x62, 0x00, 0x78, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x10, 0x05, 0x1c, 0x04, 0xff, 0x00,
        ],
    ), // 창 할비스트 (lv29)
    (
        3,
        22,
        [
            0x3c, 0x00, 0xd7, 0x01, 0x00, 0x03, 0x4c, 0x00, 0x8e, 0x00, 0x1a, 0x00, 0x03, 0x1d, 0x00, 0x29, 0x03, 0x1e, 0x04, 0xff, 0x00,
        ],
    ), // 창 하이스피어 (lv29)
    (
        3,
        23,
        [
            0x36, 0x00, 0xc5, 0x01, 0x01, 0x03, 0x7d, 0x00, 0x99, 0x00, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x11, 0x06, 0x13, 0x18, 0xff, 0x00,
        ],
    ), // 창 이실그라드 (lv39)
    (
        3,
        24,
        [
            0x3d, 0x00, 0xda, 0x01, 0x00, 0x03, 0x61, 0x00, 0xb4, 0x00, 0x1a, 0x00, 0x03, 0x27, 0x00, 0x2d, 0x0a, 0x11, 0x08, 0xff, 0x00,
        ],
    ), // 창 임모티르 (lv39)
    (
        3,
        35,
        [
            0x34, 0x00, 0xbf, 0x01, 0x01, 0x03, 0x8d, 0x00, 0xac, 0x00, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x12, 0x05, 0x14, 0x0e, 0xff, 0x00,
        ],
    ), // 창 오웬글레이브 (lv45)
    (
        3,
        36,
        [
            0x3a, 0x00, 0xd1, 0x01, 0x00, 0x03, 0x6d, 0x00, 0xcb, 0x00, 0x1c, 0x00, 0x03, 0x2d, 0x00, 0x10, 0x07, 0x1c, 0x06, 0xff, 0x00,
        ],
    ), // 창 섀도우랜스 (lv45)
    (
        3,
        37,
        [
            0x35, 0x00, 0xc2, 0x01, 0x01, 0x03, 0x97, 0x00, 0xb9, 0x00, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x30, 0x01, 0x0e, 0x09, 0xff, 0x00,
        ],
    ), // 창 다크랜스 (lv49)
    (
        3,
        38,
        [
            0x3b, 0x00, 0xd4, 0x01, 0x00, 0x03, 0x76, 0x00, 0xdb, 0x00, 0x1c, 0x00, 0x03, 0x31, 0x00, 0x10, 0x08, 0x2e, 0x0c, 0xff, 0x00,
        ],
    ), // 창 블루스톰 (lv49)
    (
        3,
        39,
        [
            0x36, 0x00, 0xc5, 0x01, 0x01, 0x03, 0xa5, 0x00, 0xc9, 0x00, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x30, 0x01, 0x2b, 0x14, 0xff, 0x00,
        ],
    ), // 창 셸트라이던트 (lv54)
    (
        3,
        40,
        [
            0x3c, 0x00, 0xd7, 0x01, 0x00, 0x03, 0x80, 0x00, 0xee, 0x00, 0x1c, 0x00, 0x03, 0x36, 0x00, 0x12, 0x06, 0x21, 0x07, 0xff, 0x00,
        ],
    ), // 창 바나헤임 (lv54)
    (
        3,
        41,
        [
            0x37, 0x00, 0xc7, 0x01, 0x01, 0x03, 0xb2, 0x00, 0xd9, 0x00, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x30, 0x01, 0x29, 0x08, 0xff, 0x00,
        ],
    ), // 창 아에기스 (lv59)
    (
        3,
        42,
        [
            0x3e, 0x00, 0xdc, 0x01, 0x00, 0x03, 0x8a, 0x00, 0x01, 0x01, 0x1c, 0x00, 0x03, 0x3b, 0x00, 0x26, 0x06, 0x1c, 0x08, 0xff, 0x00,
        ],
    ), // 창 섀도우스피어 (lv59)
    (
        3,
        61,
        [
            0x34, 0x00, 0xbf, 0x01, 0x01, 0x03, 0xbf, 0x00, 0xea, 0x00, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x26, 0x07, 0x30, 0x01, 0x15, 0x05,
        ],
    ), // 창 메탈리그림 (lv64)
    (
        3,
        62,
        [
            0x3a, 0x00, 0xd1, 0x01, 0x00, 0x03, 0x95, 0x00, 0x14, 0x01, 0x1c, 0x00, 0x03, 0x40, 0x00, 0x1e, 0x07, 0x26, 0x08, 0x2e, 0x14,
        ],
    ), // 창 태양척살장창 (lv64)
    (
        3,
        63,
        [
            0x35, 0x00, 0xc2, 0x01, 0x01, 0x03, 0xdc, 0x00, 0x0d, 0x01, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x30, 0x01, 0x1c, 0x0a, 0x16, 0x0c,
        ],
    ), // 창 암흑척살장창 (lv75)
    (
        3,
        64,
        [
            0x3b, 0x00, 0xd4, 0x01, 0x00, 0x03, 0xab, 0x00, 0x3e, 0x01, 0x1c, 0x00, 0x03, 0x4b, 0x00, 0x10, 0x0c, 0x1c, 0x0a, 0x2e, 0x17,
        ],
    ), // 창 발라스칼프 (lv75)
    (
        3,
        65,
        [
            0x36, 0x00, 0xc5, 0x01, 0x01, 0x03, 0xf4, 0x00, 0x2b, 0x01, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x12, 0x09, 0x1e, 0x0b, 0x30, 0x01,
        ],
    ), // 창 드발린의빛 (lv84)
    (
        3,
        66,
        [
            0x3c, 0x00, 0xd7, 0x01, 0x00, 0x03, 0xbe, 0x00, 0x61, 0x01, 0x1e, 0x00, 0x03, 0x54, 0x00, 0x30, 0x01, 0x2b, 0x1f, 0x16, 0x0d,
        ],
    ), // 창 라우페이장창 (lv84)
    (
        3,
        67,
        [
            0x37, 0x00, 0xc7, 0x01, 0x01, 0x03, 0x14, 0x01, 0x51, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x10, 0x0f, 0x14, 0x1d, 0x16, 0x0f,
        ],
    ), // 창 아그나르장창 (lv96)
    (
        3,
        68,
        [
            0x3d, 0x00, 0xd9, 0x01, 0x00, 0x03, 0xd7, 0x00, 0x8f, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x0e, 0x0f, 0x2e, 0x18, 0x26, 0x0f,
        ],
    ), // 창 리프트레시브 (lv96)
    (
        3,
        69,
        [
            0x37, 0x00, 0xc8, 0x01, 0x01, 0x03, 0x14, 0x01, 0x51, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x29, 0x0a, 0x30, 0x01, 0x14, 0x25,
        ],
    ), // 창 스키르니르 (lv96)
    (
        3,
        70,
        [
            0x3d, 0x00, 0xda, 0x01, 0x00, 0x03, 0xd7, 0x00, 0x8f, 0x01, 0x1e, 0x00, 0x03, 0x60, 0x00, 0x2d, 0x19, 0x1c, 0x0c, 0x2e, 0x1d,
        ],
    ), // 창 로스크바장창 (lv96)
    (
        5,
        17,
        [
            0x6c, 0x00, 0x56, 0x00, 0x01, 0x05, 0x0a, 0x00, 0x0a, 0x00, 0x1b, 0x00, 0x03, 0x0d, 0x00, 0x1f, 0x03, 0x23, 0x02, 0xff, 0x00,
        ],
    ), // 투구 다크서클릿 (lv13)
    (
        5,
        18,
        [
            0x70, 0x00, 0x86, 0x00, 0x01, 0x06, 0x0d, 0x00, 0x0d, 0x00, 0x1b, 0x00, 0x03, 0x0d, 0x00, 0x20, 0x03, 0x10, 0x03, 0xff, 0x00,
        ],
    ), // 투구 빙해의투구 (lv13)
    (
        5,
        19,
        [
            0x6d, 0x00, 0x62, 0x00, 0x01, 0x05, 0x11, 0x00, 0x11, 0x00, 0x1b, 0x00, 0x03, 0x19, 0x00, 0x0f, 0x04, 0x20, 0x07, 0xff, 0x00,
        ],
    ), // 투구 메인기어 (lv25)
    (
        5,
        20,
        [
            0x71, 0x00, 0x92, 0x00, 0x01, 0x06, 0x15, 0x00, 0x15, 0x00, 0x1b, 0x00, 0x03, 0x19, 0x00, 0x24, 0x03, 0x20, 0x07, 0xff, 0x00,
        ],
    ), // 투구 팬텀스카우터 (lv25)
    (
        5,
        21,
        [
            0x6e, 0x00, 0x6e, 0x00, 0x01, 0x05, 0x19, 0x00, 0x19, 0x00, 0x1b, 0x00, 0x03, 0x28, 0x00, 0x0f, 0x07, 0x27, 0x05, 0xff, 0x00,
        ],
    ), // 투구 블러디아이 (lv40)
    (
        5,
        22,
        [
            0x72, 0x00, 0x9e, 0x00, 0x01, 0x06, 0x20, 0x00, 0x20, 0x00, 0x1b, 0x00, 0x03, 0x28, 0x00, 0x1f, 0x09, 0x24, 0x05, 0xff, 0x00,
        ],
    ), // 투구 바하무트헬름 (lv40)
    (
        5,
        35,
        [
            0x6c, 0x00, 0x56, 0x00, 0x01, 0x05, 0x1d, 0x00, 0x1d, 0x00, 0x1b, 0x00, 0x03, 0x2e, 0x00, 0x28, 0x05, 0x23, 0x06, 0xff, 0x00,
        ],
    ), // 투구 벨아이 (lv46)
    (
        5,
        36,
        [
            0x70, 0x00, 0x86, 0x00, 0x01, 0x06, 0x24, 0x00, 0x24, 0x00, 0x1b, 0x00, 0x03, 0x2e, 0x00, 0x20, 0x0a, 0x23, 0x06, 0xff, 0x00,
        ],
    ), // 투구 카이얀헬름 (lv46)
    (
        5,
        37,
        [
            0x6d, 0x00, 0x62, 0x00, 0x01, 0x05, 0x1f, 0x00, 0x1f, 0x00, 0x1b, 0x00, 0x03, 0x32, 0x00, 0x0f, 0x08, 0x10, 0x0a, 0xff, 0x00,
        ],
    ), // 투구 사티라스햇 (lv50)
    (
        5,
        38,
        [
            0x71, 0x00, 0x92, 0x00, 0x01, 0x06, 0x27, 0x00, 0x27, 0x00, 0x1b, 0x00, 0x03, 0x32, 0x00, 0x24, 0x06, 0x20, 0x0d, 0xff, 0x00,
        ],
    ), // 투구 리트무쇠헬름 (lv50)
    (
        5,
        39,
        [
            0x6e, 0x00, 0x6e, 0x00, 0x01, 0x05, 0x22, 0x00, 0x22, 0x00, 0x1d, 0x00, 0x03, 0x37, 0x00, 0x0f, 0x09, 0x20, 0x0e, 0xff, 0x00,
        ],
    ), // 투구 바솔로뮤햇 (lv55)
    (
        5,
        40,
        [
            0x72, 0x00, 0x9e, 0x00, 0x01, 0x06, 0x2a, 0x00, 0x2a, 0x00, 0x1d, 0x00, 0x03, 0x37, 0x00, 0x1f, 0x0c, 0x27, 0x07, 0xff, 0x00,
        ],
    ), // 투구 바니시헬름 (lv55)
    (
        5,
        41,
        [
            0x6f, 0x00, 0x7a, 0x00, 0x01, 0x05, 0x25, 0x00, 0x25, 0x00, 0x1d, 0x00, 0x03, 0x3c, 0x00, 0x20, 0x0d, 0x24, 0x08, 0xff, 0x00,
        ],
    ), // 투구 티탄전쟁투구 (lv60)
    (
        5,
        42,
        [
            0x73, 0x00, 0xaa, 0x00, 0x01, 0x06, 0x2e, 0x00, 0x2e, 0x00, 0x1d, 0x00, 0x03, 0x3c, 0x00, 0x28, 0x07, 0x1f, 0x0f, 0xff, 0x00,
        ],
    ), // 투구 스쿨드워헬름 (lv60)
    (
        5,
        63,
        [
            0x6c, 0x00, 0x56, 0x00, 0x01, 0x05, 0x28, 0x00, 0x28, 0x00, 0x1d, 0x00, 0x03, 0x41, 0x00, 0x28, 0x07, 0x23, 0x08, 0xff, 0x00,
        ],
    ), // 투구 브레툰기어 (lv65)
    (
        5,
        64,
        [
            0x70, 0x00, 0x86, 0x00, 0x01, 0x06, 0x32, 0x00, 0x32, 0x00, 0x1d, 0x00, 0x03, 0x41, 0x00, 0x20, 0x0e, 0x23, 0x08, 0xff, 0x00,
        ],
    ), // 투구 릴리스투구 (lv65)
    (
        5,
        65,
        [
            0x6d, 0x00, 0x62, 0x00, 0x01, 0x05, 0x2e, 0x00, 0x2e, 0x00, 0x1f, 0x00, 0x03, 0x4c, 0x00, 0x0f, 0x0c, 0x10, 0x0e, 0xff, 0x00,
        ],
    ), // 투구 베리엘서클릿 (lv76)
    (
        5,
        66,
        [
            0x71, 0x00, 0x92, 0x00, 0x01, 0x06, 0x39, 0x00, 0x39, 0x00, 0x1f, 0x00, 0x03, 0x4c, 0x00, 0x24, 0x08, 0x20, 0x13, 0xff, 0x00,
        ],
    ), // 투구 발데르워헬름 (lv76)
    (
        5,
        67,
        [
            0x6e, 0x00, 0x6e, 0x00, 0x01, 0x05, 0x33, 0x00, 0x33, 0x00, 0x1f, 0x00, 0x03, 0x55, 0x00, 0x0f, 0x0d, 0x20, 0x15, 0xff, 0x00,
        ],
    ), // 투구 헤이드기어 (lv85)
    (
        5,
        68,
        [
            0x72, 0x00, 0x9e, 0x00, 0x01, 0x06, 0x40, 0x00, 0x40, 0x00, 0x1f, 0x00, 0x03, 0x55, 0x00, 0x1f, 0x12, 0x27, 0x0b, 0xff, 0x00,
        ],
    ), // 투구 플레인투구 (lv85)
    (
        5,
        69,
        [
            0x6f, 0x00, 0x7a, 0x00, 0x01, 0x05, 0x3a, 0x00, 0x3a, 0x00, 0x1f, 0x00, 0x03, 0x62, 0x00, 0x20, 0x14, 0x24, 0x0c, 0xff, 0x00,
        ],
    ), // 투구 아시크서클릿 (lv98)
    (
        5,
        70,
        [
            0x73, 0x00, 0xaa, 0x00, 0x01, 0x06, 0x49, 0x00, 0x49, 0x00, 0x1f, 0x00, 0x03, 0x62, 0x00, 0x28, 0x0a, 0x1f, 0x18, 0xff, 0x00,
        ],
    ), // 투구 페르세헬름 (lv98)
    (
        5,
        71,
        [
            0x75, 0x00, 0xc2, 0x00, 0x01, 0x05, 0x3a, 0x00, 0x3a, 0x00, 0x1f, 0x00, 0x03, 0x62, 0x00, 0x20, 0x14, 0x16, 0x0c, 0xff, 0x00,
        ],
    ), // 투구 가비두헬멧 (lv98)
    (
        5,
        72,
        [
            0x74, 0x00, 0xb6, 0x00, 0x01, 0x06, 0x49, 0x00, 0x49, 0x00, 0x1f, 0x00, 0x03, 0x62, 0x00, 0x12, 0x0a, 0x27, 0x0c, 0xff, 0x00,
        ],
    ), // 투구 티르전쟁투구 (lv98)
    (
        5,
        73,
        [
            0x79, 0x00, 0xea, 0x00, 0x01, 0x05, 0x3a, 0x00, 0x3a, 0x00, 0x1f, 0x00, 0x03, 0x62, 0x00, 0x12, 0x0a, 0x24, 0x0c, 0xff, 0x00,
        ],
    ), // 투구 무극위상투구 (lv98)
    (
        5,
        74,
        [
            0x75, 0x00, 0xc2, 0x00, 0x01, 0x06, 0x49, 0x00, 0x49, 0x00, 0x1f, 0x00, 0x03, 0x62, 0x00, 0x24, 0x0a, 0x1f, 0x18, 0xff, 0x00,
        ],
    ), // 투구 몰라크투구 (lv98)
    (
        5,
        81,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x22, 0x00, 0x03, 0x01, 0x00, 0x6d, 0x05, 0x1e, 0x04, 0xff, 0x00,
        ],
    ), // 투구 청금석헤어핀 (lv1)
    (
        5,
        82,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x22, 0x00, 0x03, 0x01, 0x00, 0x6d, 0x05, 0x20, 0x08, 0xff, 0x00,
        ],
    ), // 투구 석류석서클릿 (lv1)
    (
        5,
        84,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x6d, 0x0a, 0x1e, 0x08, 0xff, 0x00,
        ],
    ), // 투구 남옥서클릿 (lv1)
    (
        5,
        85,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x6d, 0x0a, 0x20, 0x0f, 0xff, 0x00,
        ],
    ), // 투구 묘안석헤어핀 (lv1)
    (
        5,
        87,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x6d, 0x0e, 0x1e, 0x0b, 0xff, 0x00,
        ],
    ), // 투구 루비헤어핀 (lv1)
    (
        5,
        88,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x23, 0x00, 0x03, 0x01, 0x00, 0x6d, 0x0e, 0x20, 0x16, 0xff, 0x00,
        ],
    ), // 투구 토파즈서클릿 (lv1)
    (
        6,
        17,
        [
            0x02, 0x00, 0x57, 0x00, 0x01, 0x05, 0x20, 0x00, 0x20, 0x00, 0x1b, 0x00, 0x03, 0x0c, 0x00, 0x20, 0x03, 0x13, 0x08, 0xff, 0x00,
        ],
    ), // 갑옷 다크코트 (lv12)
    (
        6,
        18,
        [
            0x06, 0x00, 0x87, 0x00, 0x01, 0x06, 0x28, 0x00, 0x28, 0x00, 0x1b, 0x00, 0x03, 0x0c, 0x00, 0x23, 0x02, 0x14, 0x04, 0xff, 0x00,
        ],
    ), // 갑옷 빙해의갑옷 (lv12)
    (
        6,
        19,
        [
            0x03, 0x00, 0x63, 0x00, 0x01, 0x05, 0x37, 0x00, 0x37, 0x00, 0x1b, 0x00, 0x03, 0x18, 0x00, 0x10, 0x04, 0x23, 0x03, 0xff, 0x00,
        ],
    ), // 갑옷 메인컴뱃슈트 (lv24)
    (
        6,
        20,
        [
            0x07, 0x00, 0x93, 0x00, 0x01, 0x06, 0x44, 0x00, 0x44, 0x00, 0x1b, 0x00, 0x03, 0x18, 0x00, 0x24, 0x03, 0x12, 0x03, 0xff, 0x00,
        ],
    ), // 갑옷 팬텀아머 (lv24)
    (
        6,
        21,
        [
            0x04, 0x00, 0x6f, 0x00, 0x01, 0x05, 0x53, 0x00, 0x53, 0x00, 0x1b, 0x00, 0x03, 0x27, 0x00, 0x1f, 0x08, 0x24, 0x05, 0xff, 0x00,
        ],
    ), // 갑옷 블러디워메일 (lv39)
    (
        6,
        22,
        [
            0x08, 0x00, 0x9f, 0x00, 0x01, 0x06, 0x68, 0x00, 0x68, 0x00, 0x1b, 0x00, 0x03, 0x27, 0x00, 0x21, 0x04, 0x12, 0x05, 0xff, 0x00,
        ],
    ), // 갑옷 바하무트아머 (lv39)
    (
        6,
        35,
        [
            0x02, 0x00, 0x57, 0x00, 0x01, 0x05, 0x5e, 0x00, 0x5e, 0x00, 0x1b, 0x00, 0x03, 0x2d, 0x00, 0x20, 0x0a, 0x13, 0x1c, 0xff, 0x00,
        ],
    ), // 갑옷 벨슈트 (lv45)
    (
        6,
        36,
        [
            0x06, 0x00, 0x87, 0x00, 0x01, 0x06, 0x76, 0x00, 0x76, 0x00, 0x1b, 0x00, 0x03, 0x2d, 0x00, 0x23, 0x05, 0x14, 0x0e, 0xff, 0x00,
        ],
    ), // 갑옷 카이얀아머 (lv45)
    (
        6,
        37,
        [
            0x03, 0x00, 0x63, 0x00, 0x01, 0x05, 0x66, 0x00, 0x66, 0x00, 0x1b, 0x00, 0x03, 0x31, 0x00, 0x10, 0x08, 0x23, 0x06, 0xff, 0x00,
        ],
    ), // 갑옷 사티라스코트 (lv49)
    (
        6,
        38,
        [
            0x07, 0x00, 0x93, 0x00, 0x01, 0x06, 0x7f, 0x00, 0x7f, 0x00, 0x1b, 0x00, 0x03, 0x31, 0x00, 0x24, 0x05, 0x12, 0x06, 0xff, 0x00,
        ],
    ), // 갑옷 리트무쇠갑옷 (lv49)
    (
        6,
        39,
        [
            0x04, 0x00, 0x6f, 0x00, 0x01, 0x05, 0x6f, 0x00, 0x6f, 0x00, 0x1d, 0x00, 0x03, 0x36, 0x00, 0x1f, 0x0b, 0x24, 0x07, 0xff, 0x00,
        ],
    ), // 갑옷 바솔로뮤로브 (lv54)
    (
        6,
        40,
        [
            0x08, 0x00, 0x9f, 0x00, 0x01, 0x06, 0x8b, 0x00, 0x8b, 0x00, 0x1d, 0x00, 0x03, 0x36, 0x00, 0x21, 0x06, 0x12, 0x07, 0xff, 0x00,
        ],
    ), // 갑옷 바니시아머 (lv54)
    (
        6,
        41,
        [
            0x05, 0x00, 0x7b, 0x00, 0x01, 0x05, 0x79, 0x00, 0x79, 0x00, 0x1d, 0x00, 0x03, 0x3b, 0x00, 0x10, 0x09, 0x23, 0x08, 0xff, 0x00,
        ],
    ), // 갑옷 티탄전쟁갑옷 (lv59)
    (
        6,
        42,
        [
            0x09, 0x00, 0xab, 0x00, 0x01, 0x06, 0x97, 0x00, 0x97, 0x00, 0x1d, 0x00, 0x03, 0x3b, 0x00, 0x2f, 0x3c, 0x13, 0x24, 0xff, 0x00,
        ],
    ), // 갑옷 스쿨드워메일 (lv59)
    (
        6,
        63,
        [
            0x02, 0x00, 0x57, 0x00, 0x01, 0x05, 0x82, 0x00, 0x82, 0x00, 0x1d, 0x00, 0x03, 0x40, 0x00, 0x20, 0x0d, 0x13, 0x27, 0xff, 0x00,
        ],
    ), // 갑옷 브레툰코트 (lv64)
    (
        6,
        64,
        [
            0x06, 0x00, 0x87, 0x00, 0x01, 0x06, 0xa3, 0x00, 0xa3, 0x00, 0x1d, 0x00, 0x03, 0x40, 0x00, 0x23, 0x07, 0x14, 0x14, 0xff, 0x00,
        ],
    ), // 갑옷 릴리스갑옷 (lv64)
    (
        6,
        65,
        [
            0x03, 0x00, 0x63, 0x00, 0x01, 0x05, 0x97, 0x00, 0x97, 0x00, 0x1f, 0x00, 0x03, 0x4b, 0x00, 0x10, 0x0c, 0x23, 0x0a, 0xff, 0x00,
        ],
    ), // 갑옷 베리엘아머 (lv75)
    (
        6,
        66,
        [
            0x07, 0x00, 0x93, 0x00, 0x01, 0x06, 0xbd, 0x00, 0xbd, 0x00, 0x1f, 0x00, 0x03, 0x4b, 0x00, 0x24, 0x08, 0x12, 0x0a, 0xff, 0x00,
        ],
    ), // 갑옷 발데르워메일 (lv75)
    (
        6,
        67,
        [
            0x04, 0x00, 0x6f, 0x00, 0x01, 0x05, 0xa8, 0x00, 0xa8, 0x00, 0x1f, 0x00, 0x03, 0x54, 0x00, 0x1f, 0x11, 0x24, 0x0b, 0xff, 0x00,
        ],
    ), // 갑옷 헤이드코트 (lv84)
    (
        6,
        68,
        [
            0x08, 0x00, 0x9f, 0x00, 0x01, 0x06, 0xd2, 0x00, 0xd2, 0x00, 0x1f, 0x00, 0x03, 0x54, 0x00, 0x21, 0x09, 0x12, 0x0b, 0xff, 0x00,
        ],
    ), // 갑옷 플레인갑옷 (lv84)
    (
        6,
        69,
        [
            0x05, 0x00, 0x7b, 0x00, 0x01, 0x05, 0xc1, 0x00, 0xc1, 0x00, 0x1f, 0x00, 0x03, 0x61, 0x00, 0x10, 0x0f, 0x23, 0x0c, 0xff, 0x00,
        ],
    ), // 갑옷 아시크코트 (lv97)
    (
        6,
        70,
        [
            0x09, 0x00, 0xab, 0x00, 0x01, 0x06, 0xf1, 0x00, 0xf1, 0x00, 0x1f, 0x00, 0x03, 0x61, 0x00, 0x2f, 0x62, 0x13, 0x3b, 0xff, 0x00,
        ],
    ), // 갑옷 페르세흉갑 (lv97)
    (
        6,
        71,
        [
            0x0b, 0x00, 0xc3, 0x00, 0x01, 0x05, 0xc1, 0x00, 0xc1, 0x00, 0x1f, 0x00, 0x03, 0x61, 0x00, 0x1f, 0x14, 0x20, 0x18, 0xff, 0x00,
        ],
    ), // 갑옷 가비두코트 (lv97)
    (
        6,
        72,
        [
            0x0a, 0x00, 0xb7, 0x00, 0x01, 0x06, 0xf1, 0x00, 0xf1, 0x00, 0x1f, 0x00, 0x03, 0x61, 0x00, 0x21, 0x0a, 0x14, 0x1e, 0xff, 0x00,
        ],
    ), // 갑옷 티르전쟁갑옷 (lv97)
    (
        6,
        73,
        [
            0x36, 0x00, 0xeb, 0x00, 0x01, 0x05, 0xc1, 0x00, 0xc1, 0x00, 0x1f, 0x00, 0x03, 0x61, 0x00, 0x20, 0x14, 0x1f, 0x18, 0xff, 0x00,
        ],
    ), // 갑옷 무극위상코트 (lv97)
    (
        6,
        74,
        [
            0x0b, 0x00, 0xc3, 0x00, 0x01, 0x06, 0xf1, 0x00, 0xf1, 0x00, 0x1f, 0x00, 0x03, 0x61, 0x00, 0x10, 0x0f, 0x16, 0x0c, 0xff, 0x00,
        ],
    ), // 갑옷 몰라크갑옷 (lv97)
    (
        6,
        81,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x11, 0x05, 0x6d, 0x06, 0xff, 0x00,
        ],
    ), // 갑옷 더 풀 (lv1)
    (
        6,
        82,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x1e, 0x04, 0x20, 0x08, 0xff, 0x00,
        ],
    ), // 갑옷 매지션즈레드 (lv1)
    (
        6,
        83,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x20, 0x07, 0x6d, 0x06, 0xff, 0x00,
        ],
    ), // 갑옷 프리스티스 (lv1)
    (
        6,
        84,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x29, 0x04, 0x20, 0x08, 0xff, 0x00,
        ],
    ), // 갑옷 엠프리스 (lv1)
    (
        6,
        85,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x11, 0x0a, 0x6d, 0x0b, 0xff, 0x00,
        ],
    ), // 갑옷 옐로템퍼런스 (lv1)
    (
        6,
        86,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x1e, 0x07, 0x20, 0x0f, 0xff, 0x00,
        ],
    ), // 갑옷 심판자 (lv1)
    (
        6,
        87,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x20, 0x0d, 0x6d, 0x0b, 0xff, 0x00,
        ],
    ), // 갑옷 스타더스트 (lv1)
    (
        6,
        88,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x29, 0x07, 0x20, 0x0f, 0xff, 0x00,
        ],
    ), // 갑옷 실버 채리옷 (lv1)
    (
        6,
        89,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x11, 0x0e, 0x6d, 0x11, 0xff, 0x00,
        ],
    ), // 갑옷 페일저스티스 (lv1)
    (
        6,
        90,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x1e, 0x0a, 0x20, 0x16, 0xff, 0x00,
        ],
    ), // 갑옷 나이트메어 (lv1)
    (
        6,
        91,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x20, 0x13, 0x6d, 0x11, 0xff, 0x00,
        ],
    ), // 갑옷 휠오브포츈 (lv1)
    (
        6,
        92,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x21, 0x00, 0x03, 0x01, 0x00, 0x29, 0x0a, 0x20, 0x16, 0xff, 0x00,
        ],
    ), // 갑옷 더 월드 (lv1)
    (
        7,
        17,
        [
            0x02, 0x00, 0x58, 0x00, 0x01, 0x05, 0x06, 0x00, 0x08, 0x00, 0x1b, 0x00, 0x03, 0x0a, 0x00, 0x20, 0x03, 0x0f, 0x02, 0xff, 0x00,
        ],
    ), // 장갑 다크글러브 (lv10)
    (
        7,
        18,
        [
            0x06, 0x00, 0x88, 0x00, 0x01, 0x06, 0x07, 0x00, 0x09, 0x00, 0x1b, 0x00, 0x03, 0x0a, 0x00, 0x31, 0x02, 0x23, 0x02, 0xff, 0x00,
        ],
    ), // 장갑 빙해의수갑 (lv10)
    (
        7,
        19,
        [
            0x03, 0x00, 0x64, 0x00, 0x01, 0x05, 0x0a, 0x00, 0x0c, 0x00, 0x1b, 0x00, 0x03, 0x16, 0x00, 0x29, 0x03, 0x24, 0x03, 0xff, 0x00,
        ],
    ), // 장갑 메인컴뱃장갑 (lv22)
    (
        7,
        20,
        [
            0x07, 0x00, 0x94, 0x00, 0x01, 0x06, 0x0d, 0x00, 0x0f, 0x00, 0x1b, 0x00, 0x03, 0x16, 0x00, 0x29, 0x03, 0x0e, 0x04, 0xff, 0x00,
        ],
    ), // 장갑 팬텀글러브 (lv22)
    (
        7,
        21,
        [
            0x04, 0x00, 0x70, 0x00, 0x01, 0x05, 0x10, 0x00, 0x12, 0x00, 0x1b, 0x00, 0x03, 0x25, 0x00, 0x1f, 0x08, 0x0e, 0x07, 0xff, 0x00,
        ],
    ), // 장갑 블러디건틀릿 (lv37)
    (
        7,
        22,
        [
            0x08, 0x00, 0xa0, 0x00, 0x01, 0x06, 0x14, 0x00, 0x16, 0x00, 0x1b, 0x00, 0x03, 0x25, 0x00, 0x24, 0x04, 0x20, 0x09, 0xff, 0x00,
        ],
    ), // 장갑 바하무트핸드 (lv37)
    (
        7,
        35,
        [
            0x02, 0x00, 0x58, 0x00, 0x01, 0x05, 0x12, 0x00, 0x14, 0x00, 0x1b, 0x00, 0x03, 0x2b, 0x00, 0x28, 0x05, 0x0f, 0x08, 0xff, 0x00,
        ],
    ), // 장갑 벨핸드 (lv43)
    (
        7,
        36,
        [
            0x06, 0x00, 0x88, 0x00, 0x01, 0x06, 0x17, 0x00, 0x19, 0x00, 0x1b, 0x00, 0x03, 0x2b, 0x00, 0x24, 0x05, 0x20, 0x0b, 0xff, 0x00,
        ],
    ), // 장갑 카이얀건틀릿 (lv43)
    (
        7,
        37,
        [
            0x03, 0x00, 0x64, 0x00, 0x01, 0x05, 0x14, 0x00, 0x16, 0x00, 0x1b, 0x00, 0x03, 0x2f, 0x00, 0x31, 0x05, 0x20, 0x0c, 0xff, 0x00,
        ],
    ), // 장갑 사티라스핸드 (lv47)
    (
        7,
        38,
        [
            0x07, 0x00, 0x94, 0x00, 0x01, 0x06, 0x19, 0x00, 0x1b, 0x00, 0x1b, 0x00, 0x03, 0x2f, 0x00, 0x0f, 0x08, 0x31, 0x06, 0xff, 0x00,
        ],
    ), // 장갑 리트무쇠장갑 (lv47)
    (
        7,
        39,
        [
            0x04, 0x00, 0x70, 0x00, 0x01, 0x05, 0x16, 0x00, 0x17, 0x00, 0x1d, 0x00, 0x03, 0x34, 0x00, 0x28, 0x06, 0x0f, 0x0a, 0xff, 0x00,
        ],
    ), // 장갑 바솔로뮤핸드 (lv52)
    (
        7,
        40,
        [
            0x08, 0x00, 0xa0, 0x00, 0x01, 0x06, 0x1b, 0x00, 0x1d, 0x00, 0x1d, 0x00, 0x03, 0x34, 0x00, 0x20, 0x0b, 0x23, 0x07, 0xff, 0x00,
        ],
    ), // 장갑 바니시건틀릿 (lv52)
    (
        7,
        41,
        [
            0x05, 0x00, 0x7c, 0x00, 0x01, 0x05, 0x17, 0x00, 0x19, 0x00, 0x1d, 0x00, 0x03, 0x39, 0x00, 0x23, 0x06, 0x29, 0x07, 0xff, 0x00,
        ],
    ), // 장갑 티탄전투장갑 (lv57)
    (
        7,
        42,
        [
            0x09, 0x00, 0xac, 0x00, 0x01, 0x06, 0x1d, 0x00, 0x20, 0x00, 0x1d, 0x00, 0x03, 0x39, 0x00, 0x29, 0x06, 0x20, 0x0e, 0xff, 0x00,
        ],
    ), // 장갑 스쿨드건틀릿 (lv57)
    (
        7,
        63,
        [
            0x02, 0x00, 0x58, 0x00, 0x01, 0x05, 0x19, 0x00, 0x1b, 0x00, 0x1d, 0x00, 0x03, 0x3e, 0x00, 0x1f, 0x0d, 0x31, 0x08, 0xff, 0x00,
        ],
    ), // 장갑 브레툰핸드 (lv62)
    (
        7,
        64,
        [
            0x06, 0x00, 0x88, 0x00, 0x01, 0x06, 0x20, 0x00, 0x22, 0x00, 0x1d, 0x00, 0x03, 0x3e, 0x00, 0x20, 0x0d, 0x24, 0x08, 0xff, 0x00,
        ],
    ), // 장갑 릴리스장갑 (lv62)
    (
        7,
        65,
        [
            0x03, 0x00, 0x64, 0x00, 0x01, 0x05, 0x1d, 0x00, 0x1f, 0x00, 0x1f, 0x00, 0x03, 0x49, 0x00, 0x28, 0x08, 0x1f, 0x12, 0xff, 0x00,
        ],
    ), // 장갑 베리엘장갑 (lv73)
    (
        7,
        66,
        [
            0x07, 0x00, 0x94, 0x00, 0x01, 0x06, 0x25, 0x00, 0x27, 0x00, 0x1f, 0x00, 0x03, 0x49, 0x00, 0x31, 0x08, 0x1f, 0x12, 0xff, 0x00,
        ],
    ), // 장갑 발데르건틀릿 (lv73)
    (
        7,
        67,
        [
            0x04, 0x00, 0x70, 0x00, 0x01, 0x05, 0x21, 0x00, 0x23, 0x00, 0x1f, 0x00, 0x03, 0x52, 0x00, 0x20, 0x11, 0x20, 0x14, 0xff, 0x00,
        ],
    ), // 장갑 헤이드장갑 (lv82)
    (
        7,
        68,
        [
            0x08, 0x00, 0xa0, 0x00, 0x01, 0x06, 0x29, 0x00, 0x2b, 0x00, 0x1f, 0x00, 0x03, 0x52, 0x00, 0x24, 0x09, 0x1f, 0x14, 0xff, 0x00,
        ],
    ), // 장갑 플레인건틀릿 (lv82)
    (
        7,
        69,
        [
            0x05, 0x00, 0x7c, 0x00, 0x01, 0x05, 0x26, 0x00, 0x28, 0x00, 0x1f, 0x00, 0x03, 0x5f, 0x00, 0x24, 0x0a, 0x20, 0x17, 0xff, 0x00,
        ],
    ), // 장갑 아시크글러브 (lv95)
    (
        7,
        70,
        [
            0x09, 0x00, 0xac, 0x00, 0x01, 0x06, 0x2f, 0x00, 0x32, 0x00, 0x1f, 0x00, 0x03, 0x5f, 0x00, 0x29, 0x0a, 0x20, 0x17, 0xff, 0x00,
        ],
    ), // 장갑 페르세장갑 (lv95)
    (
        7,
        71,
        [
            0x0b, 0x00, 0xc4, 0x00, 0x01, 0x05, 0x26, 0x00, 0x28, 0x00, 0x1f, 0x00, 0x03, 0x5f, 0x00, 0x24, 0x0a, 0x31, 0x0c, 0xff, 0x00,
        ],
    ), // 장갑 가비두장갑 (lv95)
    (
        7,
        72,
        [
            0x0a, 0x00, 0xb8, 0x00, 0x01, 0x06, 0x2f, 0x00, 0x32, 0x00, 0x1f, 0x00, 0x03, 0x5f, 0x00, 0x20, 0x14, 0x23, 0x0c, 0xff, 0x00,
        ],
    ), // 장갑 티르전쟁장갑 (lv95)
    (
        7,
        73,
        [
            0x2e, 0x00, 0xec, 0x00, 0x01, 0x05, 0x26, 0x00, 0x28, 0x00, 0x1f, 0x00, 0x03, 0x5f, 0x00, 0x1f, 0x14, 0x31, 0x0c, 0xff, 0x00,
        ],
    ), // 장갑 무극위상장갑 (lv95)
    (
        7,
        74,
        [
            0x0b, 0x00, 0xc4, 0x00, 0x01, 0x06, 0x2f, 0x00, 0x32, 0x00, 0x1f, 0x00, 0x03, 0x5f, 0x00, 0x28, 0x0a, 0x24, 0x0c, 0xff, 0x00,
        ],
    ), // 장갑 몰라크장갑 (lv95)
    (
        8,
        17,
        [
            0x02, 0x00, 0x59, 0x00, 0x01, 0x05, 0x09, 0x00, 0x12, 0x00, 0x1b, 0x00, 0x03, 0x0b, 0x00, 0x24, 0x02, 0x29, 0x02, 0xff, 0x00,
        ],
    ), // 신발 다크워커 (lv11)
    (
        8,
        18,
        [
            0x06, 0x00, 0x89, 0x00, 0x01, 0x06, 0x0b, 0x00, 0x17, 0x00, 0x1b, 0x00, 0x03, 0x0b, 0x00, 0x0f, 0x02, 0x11, 0x02, 0xff, 0x00,
        ],
    ), // 신발 빙해의장화 (lv11)
    (
        8,
        19,
        [
            0x03, 0x00, 0x65, 0x00, 0x01, 0x05, 0x10, 0x00, 0x20, 0x00, 0x1b, 0x00, 0x03, 0x17, 0x00, 0x1f, 0x05, 0x1f, 0x06, 0xff, 0x00,
        ],
    ), // 신발 메인컴뱃슈즈 (lv23)
    (
        8,
        20,
        [
            0x07, 0x00, 0x95, 0x00, 0x01, 0x06, 0x14, 0x00, 0x28, 0x00, 0x1b, 0x00, 0x03, 0x17, 0x00, 0x20, 0x05, 0x23, 0x03, 0xff, 0x00,
        ],
    ), // 신발 팬텀그리브 (lv23)
    (
        8,
        21,
        [
            0x04, 0x00, 0x71, 0x00, 0x01, 0x05, 0x18, 0x00, 0x31, 0x00, 0x1b, 0x00, 0x03, 0x26, 0x00, 0x29, 0x04, 0x32, 0x04, 0xff, 0x00,
        ],
    ), // 신발 블러디슈즈 (lv38)
    (
        8,
        22,
        [
            0x08, 0x00, 0xa1, 0x00, 0x01, 0x06, 0x1e, 0x00, 0x3d, 0x00, 0x1b, 0x00, 0x03, 0x26, 0x00, 0x0f, 0x06, 0x29, 0x05, 0xff, 0x00,
        ],
    ), // 신발 바하무트워커 (lv38)
    (
        8,
        35,
        [
            0x02, 0x00, 0x59, 0x00, 0x01, 0x05, 0x1c, 0x00, 0x38, 0x00, 0x1b, 0x00, 0x03, 0x2c, 0x00, 0x28, 0x05, 0x23, 0x06, 0xff, 0x00,
        ],
    ), // 신발 벨슈즈 (lv44)
    (
        8,
        36,
        [
            0x06, 0x00, 0x89, 0x00, 0x01, 0x06, 0x23, 0x00, 0x45, 0x00, 0x1b, 0x00, 0x03, 0x2c, 0x00, 0x28, 0x05, 0x0e, 0x08, 0xff, 0x00,
        ],
    ), // 신발 카이얀그리브 (lv44)
    (
        8,
        37,
        [
            0x03, 0x00, 0x65, 0x00, 0x01, 0x05, 0x1e, 0x00, 0x3c, 0x00, 0x1b, 0x00, 0x03, 0x30, 0x00, 0x23, 0x05, 0x0e, 0x09, 0xff, 0x00,
        ],
    ), // 신발 사티라스부츠 (lv48)
    (
        8,
        38,
        [
            0x07, 0x00, 0x95, 0x00, 0x01, 0x06, 0x26, 0x00, 0x4b, 0x00, 0x1b, 0x00, 0x03, 0x30, 0x00, 0x11, 0x08, 0x11, 0x09, 0xff, 0x00,
        ],
    ), // 신발 리트무쇠신발 (lv48)
    (
        8,
        39,
        [
            0x04, 0x00, 0x71, 0x00, 0x01, 0x05, 0x21, 0x00, 0x42, 0x00, 0x1d, 0x00, 0x03, 0x35, 0x00, 0x29, 0x06, 0x0f, 0x0a, 0xff, 0x00,
        ],
    ), // 신발 바솔로뮤슈즈 (lv53)
    (
        8,
        40,
        [
            0x08, 0x00, 0xa1, 0x00, 0x01, 0x06, 0x29, 0x00, 0x52, 0x00, 0x1d, 0x00, 0x03, 0x35, 0x00, 0x11, 0x08, 0x23, 0x07, 0xff, 0x00,
        ],
    ), // 신발 바니시그리브 (lv53)
    (
        8,
        41,
        [
            0x05, 0x00, 0x7d, 0x00, 0x01, 0x05, 0x24, 0x00, 0x47, 0x00, 0x1d, 0x00, 0x03, 0x3a, 0x00, 0x1f, 0x0c, 0x0f, 0x0b, 0xff, 0x00,
        ],
    ), // 신발 티탄전투신발 (lv58)
    (
        8,
        42,
        [
            0x09, 0x00, 0xad, 0x00, 0x01, 0x06, 0x2d, 0x00, 0x59, 0x00, 0x1d, 0x00, 0x03, 0x3a, 0x00, 0x0f, 0x09, 0x29, 0x07, 0xff, 0x00,
        ],
    ), // 신발 스쿨드그리브 (lv58)
    (
        8,
        63,
        [
            0x02, 0x00, 0x59, 0x00, 0x01, 0x05, 0x27, 0x00, 0x4d, 0x00, 0x1d, 0x00, 0x03, 0x3f, 0x00, 0x32, 0x04, 0x0f, 0x0c, 0xff, 0x00,
        ],
    ), // 신발 브레툰슈즈 (lv63)
    (
        8,
        64,
        [
            0x06, 0x00, 0x89, 0x00, 0x01, 0x06, 0x30, 0x00, 0x60, 0x00, 0x1d, 0x00, 0x03, 0x3f, 0x00, 0x0f, 0x0a, 0x0f, 0x0c, 0xff, 0x00,
        ],
    ), // 신발 릴리스장화 (lv63)
    (
        8,
        65,
        [
            0x03, 0x00, 0x65, 0x00, 0x01, 0x05, 0x2d, 0x00, 0x59, 0x00, 0x1f, 0x00, 0x03, 0x4a, 0x00, 0x11, 0x0c, 0x1f, 0x12, 0xff, 0x00,
        ],
    ), // 신발 베리엘장화 (lv74)
    (
        8,
        66,
        [
            0x07, 0x00, 0x95, 0x00, 0x01, 0x06, 0x38, 0x00, 0x70, 0x00, 0x1f, 0x00, 0x03, 0x4a, 0x00, 0x28, 0x08, 0x1f, 0x12, 0xff, 0x00,
        ],
    ), // 신발 발데르그리브 (lv74)
    (
        8,
        67,
        [
            0x04, 0x00, 0x71, 0x00, 0x01, 0x05, 0x32, 0x00, 0x64, 0x00, 0x1f, 0x00, 0x03, 0x53, 0x00, 0x24, 0x09, 0x20, 0x14, 0xff, 0x00,
        ],
    ), // 신발 헤이드그리브 (lv83)
    (
        8,
        68,
        [
            0x08, 0x00, 0xa1, 0x00, 0x01, 0x06, 0x3e, 0x00, 0x7d, 0x00, 0x1f, 0x00, 0x03, 0x53, 0x00, 0x20, 0x11, 0x0f, 0x0f, 0xff, 0x00,
        ],
    ), // 신발 플레인부츠 (lv83)
    (
        8,
        69,
        [
            0x05, 0x00, 0x7d, 0x00, 0x01, 0x05, 0x39, 0x00, 0x72, 0x00, 0x1f, 0x00, 0x03, 0x60, 0x00, 0x20, 0x14, 0x24, 0x0c, 0xff, 0x00,
        ],
    ), // 신발 아시크슈즈 (lv96)
    (
        8,
        70,
        [
            0x09, 0x00, 0xad, 0x00, 0x01, 0x06, 0x48, 0x00, 0x8f, 0x00, 0x1f, 0x00, 0x03, 0x60, 0x00, 0x28, 0x0a, 0x1f, 0x18, 0xff, 0x00,
        ],
    ), // 신발 페르세워커 (lv96)
    (
        8,
        71,
        [
            0x0b, 0x00, 0xc5, 0x00, 0x01, 0x05, 0x39, 0x00, 0x72, 0x00, 0x1f, 0x00, 0x03, 0x60, 0x00, 0x29, 0x0a, 0x11, 0x12, 0xff, 0x00,
        ],
    ), // 신발 가비두장화 (lv96)
    (
        8,
        72,
        [
            0x0a, 0x00, 0xb9, 0x00, 0x01, 0x06, 0x48, 0x00, 0x8f, 0x00, 0x1f, 0x00, 0x03, 0x60, 0x00, 0x1f, 0x14, 0x20, 0x18, 0xff, 0x00,
        ],
    ), // 신발 티르전쟁신발 (lv96)
    (
        8,
        73,
        [
            0x2e, 0x00, 0xed, 0x00, 0x01, 0x05, 0x39, 0x00, 0x72, 0x00, 0x1f, 0x00, 0x03, 0x60, 0x00, 0x32, 0x04, 0x32, 0x04, 0xff, 0x00,
        ],
    ), // 신발 무극위상장화 (lv96)
    (
        8,
        74,
        [
            0x0b, 0x00, 0xc5, 0x00, 0x01, 0x06, 0x48, 0x00, 0x8f, 0x00, 0x1f, 0x00, 0x03, 0x60, 0x00, 0x1f, 0x14, 0x29, 0x0c, 0xff, 0x00,
        ],
    ), // 신발 몰라크신발 (lv96)
    (
        9,
        17,
        [
            0x03, 0x00, 0xef, 0x01, 0x01, 0x03, 0x0b, 0x00, 0x05, 0x00, 0x1b, 0x00, 0x03, 0x0e, 0x00, 0x29, 0x02, 0x25, 0x02, 0xff, 0x00,
        ],
    ), // 방패 타이탄의원호 (lv14)
    (
        9,
        18,
        [
            0x04, 0x00, 0xf2, 0x01, 0x00, 0x03, 0x05, 0x00, 0x0b, 0x00, 0x1b, 0x00, 0x03, 0x0e, 0x00, 0x23, 0x02, 0x1f, 0x04, 0xff, 0x00,
        ],
    ), // 방패 광기의전도자 (lv14)
    (
        9,
        19,
        [
            0x05, 0x00, 0xf5, 0x01, 0x01, 0x03, 0x12, 0x00, 0x09, 0x00, 0x1b, 0x00, 0x03, 0x1a, 0x00, 0x27, 0x03, 0x27, 0x04, 0xff, 0x00,
        ],
    ), // 방패 데몬실드 (lv26)
    (
        9,
        20,
        [
            0x06, 0x00, 0xf8, 0x01, 0x00, 0x03, 0x09, 0x00, 0x12, 0x00, 0x1b, 0x00, 0x03, 0x1a, 0x00, 0x20, 0x06, 0x28, 0x04, 0xff, 0x00,
        ],
    ), // 방패 본프로텍터 (lv26)
    (
        9,
        21,
        [
            0x07, 0x00, 0xfb, 0x01, 0x01, 0x03, 0x1b, 0x00, 0x0d, 0x00, 0x1b, 0x00, 0x03, 0x29, 0x00, 0x10, 0x07, 0x1f, 0x0a, 0xff, 0x00,
        ],
    ), // 방패 임팩트실드 (lv41)
    (
        9,
        22,
        [
            0x08, 0x00, 0xfe, 0x01, 0x00, 0x03, 0x0d, 0x00, 0x1b, 0x00, 0x1b, 0x00, 0x03, 0x29, 0x00, 0x10, 0x07, 0x25, 0x05, 0xff, 0x00,
        ],
    ), // 방패 아크프로텍터 (lv41)
    (
        9,
        35,
        [
            0x01, 0x00, 0xe9, 0x01, 0x01, 0x03, 0x1e, 0x00, 0x0f, 0x00, 0x1b, 0x00, 0x03, 0x2f, 0x00, 0x28, 0x05, 0x24, 0x06, 0xff, 0x00,
        ],
    ), // 방패 레퀴드디펜더 (lv47)
    (
        9,
        36,
        [
            0x02, 0x00, 0xec, 0x01, 0x00, 0x03, 0x0f, 0x00, 0x1e, 0x00, 0x1b, 0x00, 0x03, 0x2f, 0x00, 0x27, 0x05, 0x25, 0x06, 0xff, 0x00,
        ],
    ), // 방패 그랜드디펜더 (lv47)
    (
        9,
        37,
        [
            0x03, 0x00, 0xef, 0x01, 0x01, 0x03, 0x20, 0x00, 0x10, 0x00, 0x1b, 0x00, 0x03, 0x33, 0x00, 0x11, 0x08, 0x31, 0x07, 0xff, 0x00,
        ],
    ), // 방패 록섀도우 (lv51)
    (
        9,
        38,
        [
            0x04, 0x00, 0xf2, 0x01, 0x00, 0x03, 0x10, 0x00, 0x20, 0x00, 0x1b, 0x00, 0x03, 0x33, 0x00, 0x29, 0x06, 0x29, 0x07, 0xff, 0x00,
        ],
    ), // 방패 파빌리온실드 (lv51)
    (
        9,
        39,
        [
            0x05, 0x00, 0xf5, 0x01, 0x01, 0x03, 0x23, 0x00, 0x12, 0x00, 0x1d, 0x00, 0x03, 0x38, 0x00, 0x11, 0x09, 0x20, 0x0e, 0xff, 0x00,
        ],
    ), // 방패 라이트닝실드 (lv56)
    (
        9,
        40,
        [
            0x06, 0x00, 0xf8, 0x01, 0x00, 0x03, 0x12, 0x00, 0x23, 0x00, 0x1d, 0x00, 0x03, 0x38, 0x00, 0x1f, 0x0c, 0x16, 0x07, 0xff, 0x00,
        ],
    ), // 방패 가이아스실드 (lv56)
    (
        9,
        41,
        [
            0x07, 0x00, 0xfb, 0x01, 0x01, 0x03, 0x26, 0x00, 0x13, 0x00, 0x1d, 0x00, 0x03, 0x3d, 0x00, 0x20, 0x0d, 0x29, 0x08, 0xff, 0x00,
        ],
    ), // 방패 얼티밋실드 (lv61)
    (
        9,
        42,
        [
            0x08, 0x00, 0xfe, 0x01, 0x00, 0x03, 0x13, 0x00, 0x26, 0x00, 0x1d, 0x00, 0x03, 0x3d, 0x00, 0x28, 0x07, 0x20, 0x0f, 0xff, 0x00,
        ],
    ), // 방패 블리다블리크 (lv61)
    (
        9,
        63,
        [
            0x01, 0x00, 0xe9, 0x01, 0x01, 0x03, 0x29, 0x00, 0x15, 0x00, 0x1d, 0x00, 0x03, 0x42, 0x00, 0x25, 0x07, 0x14, 0x14, 0xff, 0x00,
        ],
    ), // 방패 카오스섀도우 (lv66)
    (
        9,
        64,
        [
            0x02, 0x00, 0xec, 0x01, 0x00, 0x03, 0x15, 0x00, 0x29, 0x00, 0x1d, 0x00, 0x03, 0x42, 0x00, 0x12, 0x07, 0x1f, 0x10, 0xff, 0x00,
        ],
    ), // 방패 오르도디펜더 (lv66)
    (
        9,
        65,
        [
            0x03, 0x00, 0xef, 0x01, 0x01, 0x03, 0x30, 0x00, 0x18, 0x00, 0x1f, 0x00, 0x03, 0x4d, 0x00, 0x27, 0x08, 0x25, 0x0a, 0xff, 0x00,
        ],
    ), // 방패 프로스트실드 (lv77)
    (
        9,
        66,
        [
            0x04, 0x00, 0xf2, 0x01, 0x00, 0x03, 0x18, 0x00, 0x30, 0x00, 0x1f, 0x00, 0x03, 0x4d, 0x00, 0x28, 0x08, 0x23, 0x0a, 0xff, 0x00,
        ],
    ), // 방패 가룬제엘실드 (lv77)
    (
        9,
        67,
        [
            0x05, 0x00, 0xf5, 0x01, 0x01, 0x03, 0x35, 0x00, 0x1b, 0x00, 0x1f, 0x00, 0x03, 0x56, 0x00, 0x24, 0x09, 0x13, 0x34, 0xff, 0x00,
        ],
    ), // 방패 아이언파비스 (lv86)
    (
        9,
        68,
        [
            0x06, 0x00, 0xf8, 0x01, 0x00, 0x03, 0x1b, 0x00, 0x35, 0x00, 0x1f, 0x00, 0x03, 0x56, 0x00, 0x12, 0x09, 0x28, 0x0b, 0xff, 0x00,
        ],
    ), // 방패 세라핌디펜더 (lv86)
    (
        9,
        69,
        [
            0x07, 0x00, 0xfb, 0x01, 0x01, 0x03, 0x3d, 0x00, 0x1e, 0x00, 0x1f, 0x00, 0x03, 0x63, 0x00, 0x25, 0x0a, 0x16, 0x0c, 0xff, 0x00,
        ],
    ), // 방패 언노운섀도우 (lv99)
    (
        9,
        70,
        [
            0x08, 0x00, 0xfe, 0x01, 0x00, 0x03, 0x1e, 0x00, 0x3d, 0x00, 0x1f, 0x00, 0x03, 0x63, 0x00, 0x20, 0x14, 0x12, 0x0c, 0xff, 0x00,
        ],
    ), // 방패 아크골렘실더 (lv99)
    (
        9,
        71,
        [
            0x09, 0x00, 0x01, 0x02, 0x01, 0x03, 0x3d, 0x00, 0x1e, 0x00, 0x1f, 0x00, 0x03, 0x63, 0x00, 0x28, 0x0a, 0x16, 0x0c, 0xff, 0x00,
        ],
    ), // 방패 언브레이커블 (lv99)
    (
        9,
        72,
        [
            0x0a, 0x00, 0x04, 0x02, 0x00, 0x03, 0x1e, 0x00, 0x3d, 0x00, 0x1f, 0x00, 0x03, 0x63, 0x00, 0x27, 0x0a, 0x11, 0x12, 0xff, 0x00,
        ],
    ), // 방패 그랜디쉬실드 (lv99)
    (
        9,
        73,
        [
            0x0d, 0x00, 0x0c, 0x02, 0x01, 0x03, 0x3d, 0x00, 0x1e, 0x00, 0x1f, 0x00, 0x03, 0x63, 0x00, 0x27, 0x0a, 0x2f, 0x64, 0xff, 0x00,
        ],
    ), // 방패 가디언즈실드 (lv99)
    (
        9,
        74,
        [
            0x0e, 0x00, 0x0f, 0x02, 0x00, 0x03, 0x1e, 0x00, 0x3d, 0x00, 0x1f, 0x00, 0x03, 0x63, 0x00, 0x20, 0x14, 0x25, 0x0c, 0xff, 0x00,
        ],
    ), // 방패 데크알브실드 (lv99)
];

const HERO5_BOX_LEGENDARY: [(u8, u8, [u8; HERO5_TABLE_STATS]); 81] = [
    (
        0,
        25,
        [
            0x05, 0x00, 0x30, 0x01, 0x01, 0x00, 0x79, 0x00, 0xe1, 0x00, 0x20, 0x00, 0x04, 0x22, 0x00, 0x13, 0x12, 0x16, 0x05, 0x1a, 0x06,
        ],
    ), // 검 수르트:불의검 (lv34)
    (
        0,
        26,
        [
            0x0c, 0x00, 0x45, 0x01, 0x00, 0x00, 0x3d, 0x00, 0x1e, 0x01, 0x20, 0x00, 0x04, 0x22, 0x00, 0x1c, 0x04, 0x16, 0x05, 0x14, 0x0d,
        ],
    ), // 검 페이튼:워액스 (lv34)
    (
        0,
        43,
        [
            0x05, 0x00, 0x30, 0x01, 0x01, 0x00, 0x93, 0x00, 0x10, 0x01, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x1e, 0x05, 0x16, 0x06, 0x2d, 0x11,
        ],
    ), // 검 루스:대지의검 (lv43)
    (
        0,
        44,
        [
            0x0c, 0x00, 0x45, 0x01, 0x00, 0x00, 0x49, 0x00, 0x5a, 0x01, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x10, 0x07, 0x2b, 0x10, 0x31, 0x07,
        ],
    ), // 검 미노:창공의검 (lv43)
    (
        0,
        45,
        [
            0x06, 0x00, 0x33, 0x01, 0x01, 0x00, 0xba, 0x00, 0x5a, 0x01, 0x20, 0x00, 0x04, 0x39, 0x00, 0x12, 0x06, 0x31, 0x07, 0x1a, 0x09,
        ],
    ), // 검 무스펠:광검 (lv57)
    (
        0,
        46,
        [
            0x0d, 0x00, 0x48, 0x01, 0x00, 0x00, 0x5d, 0x00, 0xb8, 0x01, 0x20, 0x00, 0x04, 0x39, 0x00, 0x12, 0x06, 0x19, 0x07, 0x26, 0x09,
        ],
    ), // 검 그리드:거신 (lv57)
    (
        0,
        71,
        [
            0x05, 0x00, 0x30, 0x01, 0x01, 0x00, 0xda, 0x00, 0x94, 0x01, 0x20, 0x00, 0x04, 0x44, 0x00, 0x21, 0x07, 0x12, 0x09, 0x16, 0x0b,
        ],
    ), // 검 후긴:굉뢰의검 (lv68)
    (
        0,
        72,
        [
            0x0b, 0x00, 0x42, 0x01, 0x00, 0x00, 0x6d, 0x00, 0x01, 0x02, 0x20, 0x00, 0x04, 0x44, 0x00, 0x30, 0x01, 0x2b, 0x19, 0x14, 0x1a,
        ],
    ), // 검 무닌:마룡의검 (lv68)
    (
        0,
        73,
        [
            0x06, 0x00, 0x33, 0x01, 0x01, 0x00, 0x02, 0x01, 0xde, 0x01, 0x20, 0x00, 0x04, 0x52, 0x00, 0x1e, 0x09, 0x0e, 0x0f, 0x14, 0x1f,
        ],
    ), // 검 릴리스:마검 (lv82)
    (
        0,
        74,
        [
            0x0d, 0x00, 0x48, 0x01, 0x00, 0x00, 0x81, 0x00, 0x5f, 0x02, 0x20, 0x00, 0x04, 0x52, 0x00, 0x12, 0x09, 0x13, 0x32, 0x16, 0x0d,
        ],
    ), // 검 그람:궁극의힘 (lv82)
    (
        0,
        75,
        [
            0x06, 0x00, 0x34, 0x01, 0x01, 0x00, 0x2c, 0x01, 0x2d, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x29, 0x0a, 0x1a, 0x0c, 0x2d, 0x25,
        ],
    ), // 검 기미르:서리검 (lv97)
    (
        0,
        76,
        [
            0x0d, 0x00, 0x49, 0x01, 0x00, 0x00, 0x96, 0x00, 0xc4, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x29, 0x0a, 0x1a, 0x0c, 0x15, 0x08,
        ],
    ), // 검 쇼픈:열정의힘 (lv97)
    (
        0,
        77,
        [
            0x07, 0x00, 0x36, 0x01, 0x01, 0x00, 0x2c, 0x01, 0x2d, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x30, 0x01, 0x31, 0x0c, 0x1a, 0x0f,
        ],
    ), // 검 스콜:태양의검 (lv97)
    (
        0,
        78,
        [
            0x0f, 0x00, 0x4e, 0x01, 0x00, 0x00, 0x96, 0x00, 0xc4, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x26, 0x0a, 0x16, 0x0c, 0x14, 0x25,
        ],
    ), // 검 오드:황혼의염 (lv97)
    (
        0,
        79,
        [
            0x07, 0x00, 0x37, 0x01, 0x01, 0x00, 0x2c, 0x01, 0x2d, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x30, 0x01, 0x11, 0x12, 0x19, 0x0f,
        ],
    ), // 검 이시르:암영검 (lv97)
    (
        0,
        80,
        [
            0x0f, 0x00, 0x4f, 0x01, 0x00, 0x00, 0x96, 0x00, 0xc4, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x0e, 0x0f, 0x1c, 0x0c, 0x2d, 0x25,
        ],
    ), // 검 알케인:혼돈 (lv97)
    (
        1,
        25,
        [
            0x15, 0x00, 0x60, 0x01, 0x01, 0x01, 0x7e, 0x00, 0xa7, 0x00, 0x20, 0x00, 0x04, 0x22, 0x00, 0x0f, 0x06, 0x30, 0x01, 0x31, 0x06,
        ],
    ), // 단검 발뭉:비탄의검 (lv34)
    (
        1,
        26,
        [
            0x1c, 0x00, 0x75, 0x01, 0x00, 0x01, 0x62, 0x00, 0xc5, 0x00, 0x20, 0x00, 0x04, 0x22, 0x00, 0x0f, 0x06, 0x1c, 0x05, 0x14, 0x0d,
        ],
    ), // 단검 가름:마수의검 (lv34)
    (
        1,
        43,
        [
            0x15, 0x00, 0x60, 0x01, 0x01, 0x01, 0x9a, 0x00, 0xcd, 0x00, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x21, 0x05, 0x0e, 0x08, 0x31, 0x07,
        ],
    ), // 단검 발키리:수호검 (lv43)
    (
        1,
        44,
        [
            0x1c, 0x00, 0x75, 0x01, 0x00, 0x01, 0x78, 0x00, 0xf3, 0x00, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x2d, 0x0b, 0x2b, 0x10, 0x14, 0x11,
        ],
    ), // 단검 녹스:밤의검 (lv43)
    (
        1,
        45,
        [
            0x16, 0x00, 0x63, 0x01, 0x01, 0x01, 0xc6, 0x00, 0x08, 0x01, 0x20, 0x00, 0x04, 0x39, 0x00, 0x30, 0x01, 0x24, 0x07, 0x15, 0x05,
        ],
    ), // 단검 룩스:빛의검 (lv57)
    (
        1,
        46,
        [
            0x1d, 0x00, 0x78, 0x01, 0x00, 0x01, 0x9a, 0x00, 0x39, 0x01, 0x20, 0x00, 0x04, 0x39, 0x00, 0x21, 0x06, 0x31, 0x07, 0x2d, 0x16,
        ],
    ), // 단검 모티르:저주 (lv57)
    (
        1,
        71,
        [
            0x15, 0x00, 0x60, 0x01, 0x01, 0x01, 0xe9, 0x00, 0x37, 0x01, 0x20, 0x00, 0x04, 0x44, 0x00, 0x30, 0x01, 0x1e, 0x09, 0x2b, 0x1f,
        ],
    ), // 단검 발드르:송곳니 (lv68)
    (
        1,
        72,
        [
            0x1b, 0x00, 0x72, 0x01, 0x00, 0x01, 0xb5, 0x00, 0x70, 0x01, 0x20, 0x00, 0x04, 0x44, 0x00, 0x0f, 0x0b, 0x0f, 0x0d, 0x26, 0x0b,
        ],
    ), // 단검 게리:암흑의검 (lv68)
    (
        1,
        73,
        [
            0x16, 0x00, 0x63, 0x01, 0x01, 0x01, 0x15, 0x01, 0x72, 0x01, 0x20, 0x00, 0x04, 0x52, 0x00, 0x0f, 0x0d, 0x31, 0x0a, 0x30, 0x01,
        ],
    ), // 단검 에스가르:파괴 (lv82)
    (
        1,
        74,
        [
            0x1d, 0x00, 0x78, 0x01, 0x00, 0x01, 0xd8, 0x00, 0xb6, 0x01, 0x20, 0x00, 0x04, 0x52, 0x00, 0x30, 0x01, 0x1c, 0x0a, 0x14, 0x1f,
        ],
    ), // 단검 에인헤랴르 (lv82)
    (
        1,
        75,
        [
            0x16, 0x00, 0x64, 0x01, 0x01, 0x01, 0x44, 0x01, 0xb2, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x1c, 0x0a, 0x0e, 0x12, 0x2d, 0x25,
        ],
    ), // 단검 알스비드:비원 (lv97)
    (
        1,
        76,
        [
            0x1d, 0x00, 0x79, 0x01, 0x00, 0x01, 0xfc, 0x00, 0x01, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x26, 0x0a, 0x14, 0x1e, 0x15, 0x08,
        ],
    ), // 단검 아에기르:숙명 (lv97)
    (
        1,
        77,
        [
            0x17, 0x00, 0x66, 0x01, 0x01, 0x01, 0x44, 0x01, 0xb2, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x1c, 0x0a, 0x30, 0x01, 0x15, 0x08,
        ],
    ), // 단검 보르:망자의검 (lv97)
    (
        1,
        78,
        [
            0x21, 0x00, 0x84, 0x01, 0x00, 0x01, 0xfc, 0x00, 0x01, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x1c, 0x0a, 0x30, 0x01, 0x2b, 0x2c,
        ],
    ), // 단검 그랑드:맹세 (lv97)
    (
        1,
        79,
        [
            0x17, 0x00, 0x67, 0x01, 0x01, 0x01, 0x44, 0x01, 0xb2, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x0f, 0x0f, 0x0e, 0x12, 0x2e, 0x1e,
        ],
    ), // 단검 프레키:열풍검 (lv97)
    (
        1,
        80,
        [
            0x21, 0x00, 0x85, 0x01, 0x00, 0x01, 0xfc, 0x00, 0x01, 0x02, 0x20, 0x00, 0x04, 0x61, 0x00, 0x30, 0x01, 0x29, 0x0c, 0x15, 0x08,
        ],
    ), // 단검 펜리르:야성검 (lv97)
    (
        2,
        25,
        [
            0x28, 0x00, 0x99, 0x01, 0x01, 0x02, 0x56, 0x00, 0x69, 0x00, 0x20, 0x00, 0x04, 0x22, 0x00, 0x1d, 0x04, 0x16, 0x05, 0x2d, 0x0d,
        ],
    ), // 총 워락:지옥불 (lv34)
    (
        2,
        26,
        [
            0x2f, 0x00, 0xae, 0x01, 0x00, 0x02, 0x50, 0x00, 0xba, 0x00, 0x20, 0x00, 0x04, 0x22, 0x00, 0x2d, 0x09, 0x30, 0x01, 0x2b, 0x10,
        ],
    ), // 총 인테그란:뇌격 (lv34)
    (
        2,
        43,
        [
            0x28, 0x00, 0x99, 0x01, 0x01, 0x02, 0x68, 0x00, 0x7f, 0x00, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x26, 0x05, 0x22, 0x06, 0x2b, 0x14,
        ],
    ), // 총 엘디르:단죄자 (lv43)
    (
        2,
        44,
        [
            0x2f, 0x00, 0xae, 0x01, 0x00, 0x02, 0x60, 0x00, 0xe1, 0x00, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x1d, 0x05, 0x30, 0x01, 0x2d, 0x11,
        ],
    ), // 총 시엘라:매의눈 (lv43)
    (
        2,
        45,
        [
            0x29, 0x00, 0x9c, 0x01, 0x01, 0x02, 0x84, 0x00, 0xa1, 0x00, 0x20, 0x00, 0x04, 0x39, 0x00, 0x29, 0x06, 0x13, 0x23, 0x14, 0x16,
        ],
    ), // 총 라이오넬:낙뢰 (lv57)
    (
        2,
        46,
        [
            0x30, 0x00, 0xb1, 0x01, 0x00, 0x02, 0x7a, 0x00, 0x1d, 0x01, 0x20, 0x00, 0x04, 0x39, 0x00, 0x1e, 0x06, 0x16, 0x07, 0x14, 0x16,
        ],
    ), // 총 데스락:경계 (lv57)
    (
        2,
        71,
        [
            0x28, 0x00, 0x99, 0x01, 0x01, 0x02, 0x9a, 0x00, 0xbc, 0x00, 0x20, 0x00, 0x04, 0x44, 0x00, 0x1d, 0x07, 0x29, 0x09, 0x17, 0x0b,
        ],
    ), // 총 걀라르호른 (lv68)
    (
        2,
        72,
        [
            0x2e, 0x00, 0xab, 0x01, 0x00, 0x02, 0x8e, 0x00, 0x4c, 0x01, 0x20, 0x00, 0x04, 0x44, 0x00, 0x22, 0x07, 0x2b, 0x19, 0x2b, 0x1f,
        ],
    ), // 총 플로라:가호 (lv68)
    (
        2,
        73,
        [
            0x29, 0x00, 0x9c, 0x01, 0x01, 0x02, 0xb6, 0x00, 0xde, 0x00, 0x20, 0x00, 0x04, 0x52, 0x00, 0x29, 0x09, 0x29, 0x0a, 0x17, 0x0d,
        ],
    ), // 총 디엘룬:계승자 (lv82)
    (
        2,
        74,
        [
            0x30, 0x00, 0xb1, 0x01, 0x00, 0x02, 0xa8, 0x00, 0x89, 0x01, 0x20, 0x00, 0x04, 0x52, 0x00, 0x0f, 0x0d, 0x1d, 0x0a, 0x16, 0x0d,
        ],
    ), // 총 나르비:수호자 (lv82)
    (
        2,
        75,
        [
            0x29, 0x00, 0x9d, 0x01, 0x01, 0x02, 0xd4, 0x00, 0x03, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x2d, 0x19, 0x16, 0x0c, 0x31, 0x0f,
        ],
    ), // 총 페이탈:구원자 (lv97)
    (
        2,
        76,
        [
            0x30, 0x00, 0xb2, 0x01, 0x00, 0x02, 0xc4, 0x00, 0xc9, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x1e, 0x0a, 0x22, 0x0c, 0x31, 0x0f,
        ],
    ), // 총 길티샤인:단죄 (lv97)
    (
        2,
        77,
        [
            0x2a, 0x00, 0x9f, 0x01, 0x01, 0x02, 0xd4, 0x00, 0x03, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x1e, 0x0a, 0x2d, 0x1e, 0x16, 0x0f,
        ],
    ), // 총 브리드:맹약자 (lv97)
    (
        2,
        78,
        [
            0x31, 0x00, 0xb4, 0x01, 0x00, 0x02, 0xc4, 0x00, 0xc9, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x1e, 0x0a, 0x16, 0x0c, 0x17, 0x0f,
        ],
    ), // 총 레딘:인도자 (lv97)
    (
        2,
        79,
        [
            0x2a, 0x00, 0xa0, 0x01, 0x01, 0x02, 0xd4, 0x00, 0x03, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x22, 0x0a, 0x17, 0x0c, 0x30, 0x01,
        ],
    ), // 총 바이드:창조자 (lv97)
    (
        2,
        80,
        [
            0x31, 0x00, 0xb5, 0x01, 0x00, 0x02, 0xc4, 0x00, 0xc9, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x30, 0x01, 0x1e, 0x0c, 0x2d, 0x25,
        ],
    ), // 총 데스크림존 (lv97)
    (
        3,
        25,
        [
            0x37, 0x00, 0xc6, 0x01, 0x01, 0x03, 0x84, 0x00, 0xa2, 0x00, 0x20, 0x00, 0x04, 0x22, 0x00, 0x21, 0x04, 0x16, 0x05, 0x15, 0x03,
        ],
    ), // 창 레이븐:청염 (lv34)
    (
        3,
        26,
        [
            0x3e, 0x00, 0xdb, 0x01, 0x00, 0x03, 0x67, 0x00, 0xbf, 0x00, 0x20, 0x00, 0x04, 0x22, 0x00, 0x0e, 0x06, 0x30, 0x01, 0x2b, 0x10,
        ],
    ), // 창 메이혼:복수 (lv34)
    (
        3,
        43,
        [
            0x37, 0x00, 0xc6, 0x01, 0x01, 0x03, 0xa1, 0x00, 0xc4, 0x00, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x1c, 0x05, 0x26, 0x06, 0x31, 0x07,
        ],
    ), // 창 모르셀:격노 (lv43)
    (
        3,
        44,
        [
            0x3e, 0x00, 0xdb, 0x01, 0x00, 0x03, 0x7d, 0x00, 0xe8, 0x00, 0x20, 0x00, 0x04, 0x2b, 0x00, 0x10, 0x07, 0x16, 0x06, 0x16, 0x07,
        ],
    ), // 창 세라딘:척살 (lv43)
    (
        3,
        45,
        [
            0x38, 0x00, 0xc9, 0x01, 0x01, 0x03, 0xcd, 0x00, 0xfa, 0x00, 0x20, 0x00, 0x04, 0x39, 0x00, 0x29, 0x06, 0x1e, 0x07, 0x2b, 0x1a,
        ],
    ), // 창 제르딘:결속 (lv57)
    (
        3,
        46,
        [
            0x3f, 0x00, 0xde, 0x01, 0x00, 0x03, 0x9f, 0x00, 0x28, 0x01, 0x20, 0x00, 0x04, 0x39, 0x00, 0x0e, 0x09, 0x2d, 0x12, 0x2b, 0x1a,
        ],
    ), // 창 파이썬:투쟁 (lv57)
    (
        3,
        71,
        [
            0x37, 0x00, 0xc6, 0x01, 0x01, 0x03, 0xef, 0x00, 0x25, 0x01, 0x20, 0x00, 0x04, 0x44, 0x00, 0x11, 0x0b, 0x13, 0x29, 0x31, 0x0b,
        ],
    ), // 창 레이븐:정의 (lv68)
    (
        3,
        72,
        [
            0x3d, 0x00, 0xd8, 0x01, 0x00, 0x03, 0xba, 0x00, 0x5a, 0x01, 0x20, 0x00, 0x04, 0x44, 0x00, 0x26, 0x07, 0x30, 0x01, 0x2b, 0x1f,
        ],
    ), // 창 미라펜:저주 (lv68)
    (
        3,
        73,
        [
            0x38, 0x00, 0xc9, 0x01, 0x01, 0x03, 0x1c, 0x01, 0x5b, 0x01, 0x20, 0x00, 0x04, 0x52, 0x00, 0x11, 0x0d, 0x26, 0x0a, 0x2b, 0x25,
        ],
    ), // 창 페르딘:망령 (lv82)
    (
        3,
        74,
        [
            0x3f, 0x00, 0xde, 0x01, 0x00, 0x03, 0xdd, 0x00, 0x9a, 0x01, 0x20, 0x00, 0x04, 0x52, 0x00, 0x13, 0x2a, 0x0e, 0x0f, 0x14, 0x1f,
        ],
    ), // 창 레크벨:사령 (lv82)
    (
        3,
        75,
        [
            0x38, 0x00, 0xca, 0x01, 0x01, 0x03, 0x4b, 0x01, 0x94, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x2d, 0x19, 0x2e, 0x18, 0x14, 0x25,
        ],
    ), // 창 블린카:분쇄 (lv97)
    (
        3,
        76,
        [
            0x3f, 0x00, 0xdf, 0x01, 0x00, 0x03, 0x01, 0x01, 0xde, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x0e, 0x0f, 0x26, 0x0c, 0x15, 0x08,
        ],
    ), // 창 리신:천둥 (lv97)
    (
        3,
        77,
        [
            0x39, 0x00, 0xcc, 0x01, 0x01, 0x03, 0x4b, 0x01, 0x94, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x26, 0x0a, 0x1e, 0x0c, 0x2e, 0x1e,
        ],
    ), // 창 알라스:파괴 (lv97)
    (
        3,
        78,
        [
            0x40, 0x00, 0xe1, 0x01, 0x00, 0x03, 0x01, 0x01, 0xde, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x13, 0x31, 0x13, 0x3b, 0x31, 0x0f,
        ],
    ), // 창 히엘로:냉기 (lv97)
    (
        3,
        79,
        [
            0x39, 0x00, 0xcd, 0x01, 0x01, 0x03, 0x4b, 0x01, 0x94, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x10, 0x0f, 0x2e, 0x18, 0x2e, 0x1e,
        ],
    ), // 창 에테르:결속 (lv97)
    (
        3,
        80,
        [
            0x40, 0x00, 0xe2, 0x01, 0x00, 0x03, 0x01, 0x01, 0xde, 0x01, 0x20, 0x00, 0x04, 0x61, 0x00, 0x1e, 0x0a, 0x13, 0x3b, 0x15, 0x08,
        ],
    ), // 창 궁니르:번개 (lv97)
    (
        5,
        83,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x24, 0x00, 0x04, 0x01, 0x00, 0x6d, 0x05, 0x28, 0x04, 0x29, 0x04,
        ],
    ), // 투구 자수정헤어핀 (lv1)
    (
        5,
        86,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x24, 0x00, 0x04, 0x01, 0x00, 0x6d, 0x0a, 0x28, 0x08, 0x29, 0x07,
        ],
    ), // 투구 문스톤서클릿 (lv1)
    (
        5,
        89,
        [
            0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x24, 0x00, 0x04, 0x01, 0x00, 0x6d, 0x0e, 0x28, 0x0b, 0x29, 0x0a,
        ],
    ), // 투구 오팔헤어핀 (lv1)
    (
        9,
        23,
        [
            0x09, 0x00, 0x00, 0x02, 0x01, 0x03, 0x12, 0x00, 0x09, 0x00, 0x21, 0x00, 0x04, 0x16, 0x00, 0x29, 0x03, 0x10, 0x04, 0x12, 0x04,
        ],
    ), // 방패 아레스실드 (lv22)
    (
        9,
        24,
        [
            0x0a, 0x00, 0x03, 0x02, 0x00, 0x03, 0x09, 0x00, 0x12, 0x00, 0x21, 0x00, 0x04, 0x16, 0x00, 0x2f, 0x17, 0x29, 0x03, 0x28, 0x04,
        ],
    ), // 방패 다크웜실드 (lv22)
    (
        9,
        25,
        [
            0x0b, 0x00, 0x06, 0x02, 0x01, 0x03, 0x1f, 0x00, 0x0f, 0x00, 0x21, 0x00, 0x04, 0x28, 0x00, 0x25, 0x05, 0x1f, 0x0a, 0x25, 0x07,
        ],
    ), // 방패 홀리프로텍터 (lv40)
    (
        9,
        26,
        [
            0x0c, 0x00, 0x09, 0x02, 0x00, 0x03, 0x0f, 0x00, 0x1f, 0x00, 0x21, 0x00, 0x04, 0x28, 0x00, 0x12, 0x05, 0x14, 0x0d, 0x1f, 0x0d,
        ],
    ), // 방패 디멘션월 (lv40)
    (
        9,
        43,
        [
            0x0b, 0x00, 0x06, 0x02, 0x01, 0x03, 0x24, 0x00, 0x12, 0x00, 0x21, 0x00, 0x04, 0x30, 0x00, 0x29, 0x05, 0x0e, 0x09, 0x16, 0x08,
        ],
    ), // 방패 아키루스 (lv48)
    (
        9,
        44,
        [
            0x0c, 0x00, 0x09, 0x02, 0x00, 0x03, 0x12, 0x00, 0x24, 0x00, 0x21, 0x00, 0x04, 0x30, 0x00, 0x23, 0x05, 0x10, 0x09, 0x16, 0x08,
        ],
    ), // 방패 제노홀리즈 (lv48)
    (
        9,
        45,
        [
            0x0d, 0x00, 0x0c, 0x02, 0x01, 0x03, 0x2c, 0x00, 0x16, 0x00, 0x21, 0x00, 0x04, 0x3b, 0x00, 0x2f, 0x3c, 0x16, 0x08, 0x23, 0x09,
        ],
    ), // 방패 제노사이드 (lv59)
    (
        9,
        46,
        [
            0x0e, 0x00, 0x0f, 0x02, 0x00, 0x03, 0x16, 0x00, 0x2c, 0x00, 0x21, 0x00, 0x04, 0x3b, 0x00, 0x12, 0x06, 0x1f, 0x0f, 0x13, 0x2d,
        ],
    ), // 방패 실리홀터 (lv59)
    (
        9,
        75,
        [
            0x0b, 0x00, 0x07, 0x02, 0x01, 0x03, 0x37, 0x00, 0x1b, 0x00, 0x21, 0x00, 0x04, 0x4a, 0x00, 0x2f, 0x4b, 0x31, 0x09, 0x1b, 0x0c,
        ],
    ), // 방패 라그나블로커 (lv74)
    (
        9,
        76,
        [
            0x0c, 0x00, 0x0a, 0x02, 0x00, 0x03, 0x1b, 0x00, 0x37, 0x00, 0x21, 0x00, 0x04, 0x4a, 0x00, 0x2f, 0x4b, 0x24, 0x09, 0x13, 0x38,
        ],
    ), // 방패 블러드호른 (lv74)
    (
        9,
        77,
        [
            0x0d, 0x00, 0x0d, 0x02, 0x01, 0x03, 0x47, 0x00, 0x24, 0x00, 0x21, 0x00, 0x04, 0x62, 0x00, 0x10, 0x0f, 0x25, 0x0c, 0x14, 0x25,
        ],
    ), // 방패 더스크다크 (lv98)
    (
        9,
        78,
        [
            0x0e, 0x00, 0x10, 0x02, 0x00, 0x03, 0x24, 0x00, 0x47, 0x00, 0x21, 0x00, 0x04, 0x62, 0x00, 0x2f, 0x63, 0x14, 0x1e, 0x23, 0x0f,
        ],
    ), // 방패 데일라잇 (lv98)
    (
        9,
        79,
        [
            0x0f, 0x00, 0x11, 0x02, 0x01, 0x03, 0x47, 0x00, 0x24, 0x00, 0x21, 0x00, 0x04, 0x62, 0x00, 0x23, 0x0a, 0x1b, 0x0c, 0x1b, 0x0f,
        ],
    ), // 방패 코락스가디언 (lv98)
    (
        9,
        80,
        [
            0x10, 0x00, 0x14, 0x02, 0x00, 0x03, 0x24, 0x00, 0x47, 0x00, 0x21, 0x00, 0x04, 0x62, 0x00, 0x28, 0x0a, 0x14, 0x1e, 0x1f, 0x1e,
        ],
    ), // 방패 다크프로스트 (lv98)
];

const HERO5_BOX_SETS: [[(u8, u8, [u8; HERO5_TABLE_STATS]); 4]; 13] = [
    // 가시나무 - 등급 5, 제한레벨 20 안팎
    [
        (
            5,
            23,
            [
                0x6f, 0x00, 0x7a, 0x00, 0x01, 0x05, 0x11, 0x00, 0x11, 0x00, 0x21, 0x00, 0x05, 0x15, 0x00, 0x20, 0x05, 0x1f, 0x06, 0xff, 0x00,
            ],
        ), // 투구 가시나무투구
        (
            6,
            23,
            [
                0x05, 0x00, 0x7b, 0x00, 0x01, 0x05, 0x38, 0x00, 0x38, 0x00, 0x21, 0x00, 0x05, 0x14, 0x00, 0x10, 0x04, 0x23, 0x03, 0xff, 0x00,
            ],
        ), // 갑옷 가시나무갑옷
        (
            7,
            23,
            [
                0x05, 0x00, 0x7c, 0x00, 0x01, 0x05, 0x0a, 0x00, 0x0d, 0x00, 0x21, 0x00, 0x05, 0x12, 0x00, 0x24, 0x02, 0x0f, 0x04, 0xff, 0x00,
            ],
        ), // 장갑 가시나무 장갑
        (
            8,
            23,
            [
                0x05, 0x00, 0x7d, 0x00, 0x01, 0x05, 0x10, 0x00, 0x20, 0x00, 0x21, 0x00, 0x05, 0x13, 0x00, 0x0f, 0x03, 0x29, 0x03, 0xff, 0x00,
            ],
        ), // 신발 가시나무 장화
    ],
    // 드레이크 - 등급 6, 제한레벨 20 안팎
    [
        (
            5,
            24,
            [
                0x73, 0x00, 0xaa, 0x00, 0x01, 0x06, 0x16, 0x00, 0x16, 0x00, 0x21, 0x00, 0x06, 0x15, 0x00, 0x28, 0x03, 0x16, 0x03, 0xff, 0x00,
            ],
        ), // 투구 드레이크헬름
        (
            6,
            24,
            [
                0x09, 0x00, 0xab, 0x00, 0x01, 0x06, 0x46, 0x00, 0x46, 0x00, 0x21, 0x00, 0x06, 0x14, 0x00, 0x2f, 0x15, 0x13, 0x0d, 0xff, 0x00,
            ],
        ), // 갑옷 드레이크슈트
        (
            7,
            24,
            [
                0x09, 0x00, 0xac, 0x00, 0x01, 0x06, 0x0d, 0x00, 0x10, 0x00, 0x21, 0x00, 0x06, 0x12, 0x00, 0x20, 0x04, 0x29, 0x03, 0xff, 0x00,
            ],
        ), // 장갑 드레이크장갑
        (
            8,
            24,
            [
                0x09, 0x00, 0xad, 0x00, 0x01, 0x06, 0x14, 0x00, 0x28, 0x00, 0x21, 0x00, 0x06, 0x13, 0x00, 0x24, 0x02, 0x20, 0x05, 0xff, 0x00,
            ],
        ), // 신발 드레이크워커
    ],
    // 피닉스 - 등급 8, 제한레벨 38 안팎
    [
        (
            5,
            26,
            [
                0x74, 0x00, 0xae, 0x00, 0x01, 0x06, 0x25, 0x00, 0x25, 0x00, 0x21, 0x00, 0x08, 0x27, 0x00, 0x12, 0x04, 0x24, 0x05, 0xff, 0x00,
            ],
        ), // 투구 피닉스헬름
        (
            6,
            26,
            [
                0x0a, 0x00, 0xaf, 0x00, 0x01, 0x06, 0x78, 0x00, 0x78, 0x00, 0x21, 0x00, 0x08, 0x26, 0x00, 0x21, 0x04, 0x14, 0x0c, 0xff, 0x00,
            ],
        ), // 갑옷 피닉스아머
        (
            7,
            26,
            [
                0x0a, 0x00, 0xb0, 0x00, 0x01, 0x06, 0x17, 0x00, 0x1a, 0x00, 0x21, 0x00, 0x08, 0x24, 0x00, 0x0f, 0x06, 0x31, 0x05, 0xff, 0x00,
            ],
        ), // 장갑 피닉스건틀릿
        (
            8,
            26,
            [
                0x0a, 0x00, 0xb1, 0x00, 0x01, 0x06, 0x23, 0x00, 0x47, 0x00, 0x21, 0x00, 0x08, 0x25, 0x00, 0x11, 0x06, 0x1f, 0x09, 0xff, 0x00,
            ],
        ), // 신발 피닉스그리브
    ],
    // 엔젤윙 - 등급 9, 제한레벨 46 안팎
    [
        (
            5,
            43,
            [
                0x75, 0x00, 0xbe, 0x00, 0x01, 0x05, 0x23, 0x00, 0x23, 0x00, 0x21, 0x00, 0x09, 0x2f, 0x00, 0x20, 0x0a, 0x16, 0x06, 0xff, 0x00,
            ],
        ), // 투구 엔젤윙헬름
        (
            6,
            43,
            [
                0x0b, 0x00, 0xbf, 0x00, 0x01, 0x05, 0x72, 0x00, 0x72, 0x00, 0x21, 0x00, 0x09, 0x2e, 0x00, 0x1f, 0x0a, 0x20, 0x0c, 0xff, 0x00,
            ],
        ), // 갑옷 엔젤윙아머
        (
            7,
            43,
            [
                0x0b, 0x00, 0xc0, 0x00, 0x01, 0x05, 0x16, 0x00, 0x18, 0x00, 0x21, 0x00, 0x09, 0x2c, 0x00, 0x23, 0x05, 0x0f, 0x08, 0xff, 0x00,
            ],
        ), // 장갑 엔젤윙핸드
        (
            8,
            43,
            [
                0x0b, 0x00, 0xc1, 0x00, 0x01, 0x05, 0x22, 0x00, 0x43, 0x00, 0x21, 0x00, 0x09, 0x2d, 0x00, 0x24, 0x05, 0x23, 0x06, 0xff, 0x00,
            ],
        ), // 신발 엔젤윙슈즈
    ],
    // 드래곤본 - 등급 10, 제한레벨 46 안팎
    [
        (
            5,
            44,
            [
                0x74, 0x00, 0xb2, 0x00, 0x01, 0x06, 0x2c, 0x00, 0x2c, 0x00, 0x21, 0x00, 0x0a, 0x2f, 0x00, 0x12, 0x05, 0x27, 0x06, 0xff, 0x00,
            ],
        ), // 투구 드래곤본헬름
        (
            6,
            44,
            [
                0x0a, 0x00, 0xb3, 0x00, 0x01, 0x06, 0x8f, 0x00, 0x8f, 0x00, 0x21, 0x00, 0x0a, 0x2e, 0x00, 0x21, 0x05, 0x14, 0x0e, 0xff, 0x00,
            ],
        ), // 갑옷 드래곤본아머
        (
            7,
            44,
            [
                0x0a, 0x00, 0xb4, 0x00, 0x01, 0x06, 0x1b, 0x00, 0x1e, 0x00, 0x21, 0x00, 0x0a, 0x2c, 0x00, 0x24, 0x05, 0x0e, 0x08, 0xff, 0x00,
            ],
        ), // 장갑 드래곤본핸드
        (
            8,
            44,
            [
                0x0a, 0x00, 0xb5, 0x00, 0x01, 0x06, 0x2a, 0x00, 0x54, 0x00, 0x21, 0x00, 0x0a, 0x2d, 0x00, 0x23, 0x05, 0x23, 0x06, 0xff, 0x00,
            ],
        ), // 신발 드래곤본부츠
    ],
    // 와이어트 - 등급 11, 제한레벨 57 안팎
    [
        (
            5,
            45,
            [
                0x77, 0x00, 0xd2, 0x00, 0x01, 0x05, 0x2a, 0x00, 0x2a, 0x00, 0x21, 0x00, 0x0b, 0x3a, 0x00, 0x12, 0x06, 0x24, 0x07, 0xff, 0x00,
            ],
        ), // 투구 와이어트햇
        (
            6,
            45,
            [
                0x34, 0x00, 0xd3, 0x00, 0x01, 0x05, 0x8b, 0x00, 0x8b, 0x00, 0x21, 0x00, 0x0b, 0x39, 0x00, 0x20, 0x0c, 0x1f, 0x0e, 0xff, 0x00,
            ],
        ), // 갑옷 와이어트코트
        (
            7,
            45,
            [
                0x2c, 0x00, 0xd4, 0x00, 0x01, 0x05, 0x1b, 0x00, 0x1d, 0x00, 0x21, 0x00, 0x0b, 0x37, 0x00, 0x24, 0x06, 0x24, 0x07, 0xff, 0x00,
            ],
        ), // 장갑 와이어트암
        (
            8,
            45,
            [
                0x2b, 0x00, 0xd5, 0x00, 0x01, 0x05, 0x29, 0x00, 0x52, 0x00, 0x21, 0x00, 0x0b, 0x38, 0x00, 0x11, 0x09, 0x0f, 0x0b, 0xff, 0x00,
            ],
        ), // 신발 와이어트슈즈
    ],
    // 로크 - 등급 12, 제한레벨 57 안팎
    [
        (
            5,
            46,
            [
                0x76, 0x00, 0xc6, 0x00, 0x01, 0x06, 0x35, 0x00, 0x35, 0x00, 0x21, 0x00, 0x0c, 0x3a, 0x00, 0x24, 0x06, 0x1f, 0x0e, 0xff, 0x00,
            ],
        ), // 투구 로크헬름
        (
            6,
            46,
            [
                0x33, 0x00, 0xc7, 0x00, 0x01, 0x06, 0xae, 0x00, 0xae, 0x00, 0x21, 0x00, 0x0c, 0x39, 0x00, 0x10, 0x09, 0x16, 0x07, 0xff, 0x00,
            ],
        ), // 갑옷 로크중갑옷
        (
            7,
            46,
            [
                0x2b, 0x00, 0xc8, 0x00, 0x01, 0x06, 0x22, 0x00, 0x24, 0x00, 0x21, 0x00, 0x0c, 0x37, 0x00, 0x29, 0x06, 0x0e, 0x0a, 0xff, 0x00,
            ],
        ), // 장갑 로크손보호대
        (
            8,
            46,
            [
                0x2b, 0x00, 0xc9, 0x00, 0x01, 0x06, 0x33, 0x00, 0x66, 0x00, 0x21, 0x00, 0x0c, 0x38, 0x00, 0x0f, 0x09, 0x0f, 0x0b, 0xff, 0x00,
            ],
        ), // 신발 로크다리갑주
    ],
    // 워헤드 - 등급 13, 제한레벨 72 안팎
    [
        (
            5,
            75,
            [
                0x77, 0x00, 0xd6, 0x00, 0x01, 0x05, 0x34, 0x00, 0x34, 0x00, 0x21, 0x00, 0x0d, 0x49, 0x00, 0x1f, 0x0f, 0x10, 0x0e, 0xff, 0x00,
            ],
        ), // 투구 워헤드기어
        (
            6,
            75,
            [
                0x34, 0x00, 0xd7, 0x00, 0x01, 0x05, 0xac, 0x00, 0xac, 0x00, 0x21, 0x00, 0x0d, 0x48, 0x00, 0x24, 0x08, 0x1f, 0x12, 0xff, 0x00,
            ],
        ), // 갑옷 워헤드코트
        (
            7,
            75,
            [
                0x2c, 0x00, 0xd8, 0x00, 0x01, 0x05, 0x22, 0x00, 0x24, 0x00, 0x21, 0x00, 0x0d, 0x46, 0x00, 0x31, 0x08, 0x1f, 0x11, 0xff, 0x00,
            ],
        ), // 장갑 워헤드장갑
        (
            8,
            75,
            [
                0x2b, 0x00, 0xd9, 0x00, 0x01, 0x05, 0x33, 0x00, 0x66, 0x00, 0x21, 0x00, 0x0d, 0x47, 0x00, 0x29, 0x08, 0x24, 0x09, 0xff, 0x00,
            ],
        ), // 신발 워헤드슈즈
    ],
    // 드래곤 - 등급 14, 제한레벨 72 안팎
    [
        (
            5,
            76,
            [
                0x76, 0x00, 0xca, 0x00, 0x01, 0x06, 0x42, 0x00, 0x42, 0x00, 0x21, 0x00, 0x0e, 0x49, 0x00, 0x10, 0x0b, 0x1f, 0x12, 0xff, 0x00,
            ],
        ), // 투구 드래곤헬름
        (
            6,
            76,
            [
                0x33, 0x00, 0xcb, 0x00, 0x01, 0x06, 0xd8, 0x00, 0xd8, 0x00, 0x21, 0x00, 0x0e, 0x48, 0x00, 0x1f, 0x0f, 0x2f, 0x57, 0xff, 0x00,
            ],
        ), // 갑옷 드래곤메일
        (
            7,
            76,
            [
                0x2b, 0x00, 0xcc, 0x00, 0x01, 0x06, 0x2a, 0x00, 0x2d, 0x00, 0x21, 0x00, 0x0e, 0x46, 0x00, 0x29, 0x08, 0x1f, 0x11, 0xff, 0x00,
            ],
        ), // 장갑 드래곤건틀릿
        (
            8,
            76,
            [
                0x2b, 0x00, 0xcd, 0x00, 0x01, 0x06, 0x40, 0x00, 0x80, 0x00, 0x21, 0x00, 0x0e, 0x47, 0x00, 0x28, 0x08, 0x10, 0x0d, 0xff, 0x00,
            ],
        ), // 신발 드래곤워커
    ],
    // 아체야키 - 등급 15, 제한레벨 96 안팎
    [
        (
            5,
            77,
            [
                0x79, 0x00, 0xee, 0x00, 0x01, 0x05, 0x45, 0x00, 0x45, 0x00, 0x21, 0x00, 0x0f, 0x61, 0x00, 0x1f, 0x14, 0x20, 0x18, 0xff, 0x00,
            ],
        ), // 투구 아체야키투구
        (
            6,
            77,
            [
                0x36, 0x00, 0xef, 0x00, 0x01, 0x05, 0xe2, 0x00, 0xe2, 0x00, 0x21, 0x00, 0x0f, 0x60, 0x00, 0x28, 0x0a, 0x24, 0x0c, 0xff, 0x00,
            ],
        ), // 갑옷 아체야키갑옷
        (
            7,
            77,
            [
                0x2e, 0x00, 0xf0, 0x00, 0x01, 0x05, 0x2c, 0x00, 0x2f, 0x00, 0x21, 0x00, 0x0f, 0x5e, 0x00, 0x0f, 0x0f, 0x31, 0x0c, 0xff, 0x00,
            ],
        ), // 장갑 아체야키장갑
        (
            8,
            77,
            [
                0x2e, 0x00, 0xf1, 0x00, 0x01, 0x05, 0x43, 0x00, 0x86, 0x00, 0x21, 0x00, 0x0f, 0x5f, 0x00, 0x20, 0x14, 0x1f, 0x17, 0xff, 0x00,
            ],
        ), // 신발 아체야키신발
    ],
    // 푸른비룡 - 등급 16, 제한레벨 96 안팎
    [
        (
            5,
            78,
            [
                0x78, 0x00, 0xe2, 0x00, 0x01, 0x06, 0x56, 0x00, 0x56, 0x00, 0x21, 0x00, 0x10, 0x61, 0x00, 0x20, 0x14, 0x23, 0x0c, 0xff, 0x00,
            ],
        ), // 투구 푸른비룡헬름
        (
            6,
            78,
            [
                0x35, 0x00, 0xe3, 0x00, 0x01, 0x06, 0x1b, 0x01, 0x1b, 0x01, 0x21, 0x00, 0x10, 0x60, 0x00, 0x1f, 0x14, 0x0e, 0x12, 0xff, 0x00,
            ],
        ), // 갑옷 푸른비룡흉갑
        (
            7,
            78,
            [
                0x2d, 0x00, 0xe4, 0x00, 0x01, 0x06, 0x37, 0x00, 0x3a, 0x00, 0x21, 0x00, 0x10, 0x5e, 0x00, 0x24, 0x0a, 0x31, 0x0c, 0xff, 0x00,
            ],
        ), // 장갑 푸른비룡장갑
        (
            8,
            78,
            [
                0x2d, 0x00, 0xe5, 0x00, 0x01, 0x06, 0x54, 0x00, 0xa8, 0x00, 0x21, 0x00, 0x10, 0x5f, 0x00, 0x1f, 0x14, 0x32, 0x04, 0xff, 0x00,
            ],
        ), // 신발 푸른비룡부츠
    ],
    // 폭군쟈칼 - 등급 17, 제한레벨 96 안팎
    [
        (
            5,
            79,
            [
                0x7b, 0x00, 0x02, 0x01, 0x01, 0x05, 0x45, 0x00, 0x45, 0x00, 0x21, 0x00, 0x11, 0x61, 0x00, 0x20, 0x14, 0x24, 0x0c, 0xff, 0x00,
            ],
        ), // 투구 폭군쟈칼투구
        (
            6,
            79,
            [
                0x38, 0x00, 0x03, 0x01, 0x01, 0x05, 0xe2, 0x00, 0xe2, 0x00, 0x21, 0x00, 0x11, 0x60, 0x00, 0x2f, 0x61, 0x28, 0x0c, 0xff, 0x00,
            ],
        ), // 갑옷 폭군쟈칼아머
        (
            7,
            79,
            [
                0x30, 0x00, 0x04, 0x01, 0x01, 0x05, 0x2c, 0x00, 0x2f, 0x00, 0x21, 0x00, 0x11, 0x5e, 0x00, 0x20, 0x13, 0x31, 0x0c, 0xff, 0x00,
            ],
        ), // 장갑 폭군쟈칼핸드
        (
            8,
            79,
            [
                0x30, 0x00, 0x05, 0x01, 0x01, 0x05, 0x43, 0x00, 0x86, 0x00, 0x21, 0x00, 0x11, 0x5f, 0x00, 0x20, 0x14, 0x32, 0x04, 0xff, 0x00,
            ],
        ), // 신발 폭군쟈칼슈즈
    ],
    // 지크문드 - 등급 18, 제한레벨 96 안팎
    [
        (
            5,
            80,
            [
                0x7a, 0x00, 0xfa, 0x00, 0x01, 0x06, 0x56, 0x00, 0x56, 0x00, 0x21, 0x00, 0x12, 0x61, 0x00, 0x20, 0x14, 0x27, 0x0c, 0xff, 0x00,
            ],
        ), // 투구 지크문드헬름
        (
            6,
            80,
            [
                0x37, 0x00, 0xf7, 0x00, 0x01, 0x06, 0x1b, 0x01, 0x1b, 0x01, 0x21, 0x00, 0x12, 0x60, 0x00, 0x21, 0x0a, 0x20, 0x18, 0xff, 0x00,
            ],
        ), // 갑옷 지크문드갑옷
        (
            7,
            80,
            [
                0x2f, 0x00, 0x01, 0x00, 0x01, 0x06, 0x37, 0x00, 0x3a, 0x00, 0x21, 0x00, 0x12, 0x5e, 0x00, 0x20, 0x13, 0x1f, 0x17, 0xff, 0x00,
            ],
        ), // 장갑 지크문드핸드
        (
            8,
            80,
            [
                0x2f, 0x00, 0x02, 0x00, 0x01, 0x06, 0x54, 0x00, 0xa8, 0x00, 0x21, 0x00, 0x12, 0x5f, 0x00, 0x20, 0x14, 0x23, 0x0c, 0xff, 0x00,
            ],
        ), // 신발 지크문드워커
    ],
];

/// Which pool each 유물함 draws from.
/// Which sets each 유물함 draws from.
/// Which stretch of [`HERO5_BOX_LADDER`] each 유물함 draws from.
///
/// The four boxes are products 18 to 21 and their descriptions are all
/// 랜덤 아이템을 획득합니다 - the box is not what a purchase of one is for, so
/// answering 6/3 with the box's own row put a box in the bag and nothing ever
/// opened it. What they hand over is a whole set, which is what 영웅서기4's
/// boxes did - see [`HERO5_BOX_SETS`]. The four box classes the title builds (`0x114468` rows 15 to 18)
/// carry the same three-method vtable as each other and hold no draw of their
/// own, and the archive has no random-box table the way 영웅서기4's did, so the
/// prize was the carrier's server's to pick and it went with the service.
///
/// **How that server weighted a box is not in the archive**, exactly as it is
/// not for [`HERO4_BOX_DRAWS`], so this follows the same shape that one does:
/// the cheapest box draws the low end of the ladder, the dearest the high end,
/// and the two between them span the middle - dearer is further up, which is the
/// one thing the prices themselves say.
///
/// 신비조사회링 is left out of the 장신구: it is level 99 and priced at
/// nothing, which is not something a shop hands over.
///
/// (product id, the pool).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hero5Pool {
    Trinket,
    Heroic,
    Legendary,
    Set,
}

const HERO5_BOX_DRAWS: [(u32, Hero5Pool); 4] = [
    // 작은 유물함, 1500.
    (18, Hero5Pool::Trinket),
    // 유물함, 2000 - 영웅, the lowest grade above 노멀 the tables carry for
    // anything that is not a 장신구.
    (19, Hero5Pool::Heroic),
    // 큰 유물함, 2500.
    (20, Hero5Pool::Legendary),
    // 오래된 유물함, 3000.
    (21, Hero5Pool::Set),
];

/// The 58 bytes of equipment an item record carries past its name.
///
/// `0x333fc` reads three `u64`s, two `u16`s, two `u8`s, three `u16`s, eighteen
/// `u8`s and two `u16`s more when the item table is 10 or under - the grade,
/// the enhancement, the options and the sockets a piece of gear has and a
/// consumable does not. Seven of those bytes are read back as -1 meaning "what
/// the item table says" (`0x35da4` fills record `+0x40` and `+0x43` to `+0x48`
/// out of what `0xdf88` built), so the whole tail is not something a reply has
/// to know.
const HERO5_EQUIPMENT_TAIL: usize = 58;

/// The last item table that is equipment.
///
/// `0x333fc` reads [`HERO5_EQUIPMENT_TAIL`] more bytes for a table at or under
/// this one, and stops at the name for anything over it.
const HERO5_LAST_EQUIPMENT_TABLE: u8 = 10;

/// How many slots 영웅서기5's 창고 listing enumerates.
///
/// The 창고 screen draws a four-by-five grid, and `0x15a54` clears eighty of
/// them before a listing fills it, so eighty is the most the title will take and
/// twenty is what it shows. A deposit past that many still lists - the listing
/// is as long as it needs to be.
const HERO5_WAREHOUSE_SLOTS: usize = 20;

/// The state byte a 창고 slot carries.
///
/// `0x1507c` keeps it at `list + slot + 0x198`, alongside the item pointer and
/// the stack count. What the values mean is in the title's own later Android
/// build, which ships unstripped: `NetStorageItem::GetItemState(short)` is that
/// byte (`this[index + 0x198]`, and `GetItemState(char page, short i)` is
/// `GetItemState(page * 16 + i)`, so the 창고 is sixteen slots a page), and
/// `StateNetMenu::KeyStorage` refuses to move a slot whose state is 1, drawing
/// menu text 170 - 거래중인 아이템입니다. Which is exactly what a deposit came
/// back as when this went over as one.
///
/// Zero is what an item that is simply being held carries. It is also what
/// `NewNetStorageItem` writes for a row it was given nothing for, but a slot is
/// occupied by its item's count rather than by this, so the two do not collide.
const HERO5_SLOT_HELD: u8 = 0;

/// Where 영웅서기5's 창고 is kept between runs.
pub const HERO5_WAREHOUSE_STORE: &str = "hero5_warehouse";

/// What 영웅서기5's 창고 has been given.
///
/// There is no account here for a 창고 to have been left on, so this is the
/// whole of one: the item records 4/1 deposited, in the order it deposited
/// them. A granted 4/1 has the title take the item out of its own bag
/// (`0x362a4` calls the bag's own remove before it asks for the listing again),
/// so a 창고 that forgot a deposit would have eaten the item.
///
/// Brought in from [`HERO5_WAREHOUSE_STORE`] the first time a frame needs it and
/// written back whenever it changes.
static HERO5_WAREHOUSE: spin::Mutex<Hero5Warehouse> = spin::Mutex::new(Hero5Warehouse::new());

/// The records, and whether they have been read in and whether they still match
/// what was written out.
struct Hero5Warehouse {
    rows: Vec<Vec<u8>>,
    /// False until [`load_hero5_warehouse`] has run, whether or not anything was
    /// kept - an empty 창고 is a 창고, and reading it in twice would lose a
    /// deposit made between the two.
    loaded: bool,
    changed: bool,
}

impl Hero5Warehouse {
    const fn new() -> Self {
        Self {
            rows: Vec::new(),
            loaded: false,
            changed: false,
        }
    }
}

/// How long the item record at the front of `body` is.
///
/// The shape `0x333fc` reads: a `u32` of how many - and nothing else at all if
/// that is zero, which is where it gives up - then the item table and the row in
/// it a byte each, then a length and that many bytes of name, and then
/// [`HERO5_EQUIPMENT_TAIL`] more if the table is equipment.
///
/// `None` when `body` is shorter than the record it declares.
fn hero5_record_length(body: &[u8]) -> Option<usize> {
    const COUNT: usize = 4;
    const TABLE_AND_ROW: usize = 2;
    const NAME_LENGTH: usize = 4;

    let count = u32::from_be_bytes(body.get(..COUNT)?.try_into().unwrap());
    if count == 0 {
        return Some(COUNT);
    }

    let table = *body.get(COUNT)?;
    let at = COUNT + TABLE_AND_ROW;
    let name = u32::from_be_bytes(body.get(at..at + NAME_LENGTH)?.try_into().unwrap()) as usize;

    let mut length = at + NAME_LENGTH + name;
    if table <= HERO5_LAST_EQUIPMENT_TABLE {
        length += HERO5_EQUIPMENT_TAIL;
    }

    (length <= body.len()).then_some(length)
}

/// Take into the 창고 what a 4/1 deposit is handing it.
///
/// The frame is the item record `0x32dfc` wrote and then one `u32` of the
/// title's own (`ctx + 0x564`, zero in every capture), so the record is what
/// [`hero5_record_length`] measures off the front and the rest is not the 창고's
/// business.
///
/// A frame this cannot measure a record out of deposits nothing rather than
/// half a record - the listing hands these straight back, and half a record
/// there would desynchronise every row after it.
fn hero5_deposit(body: &[u8]) {
    let Some(length) = hero5_record_length(body) else {
        tracing::debug!("영웅서기5 deposited something this could not measure: {body:02x?}");
        return;
    };

    let mut held = HERO5_WAREHOUSE.lock();
    held.rows.push(body[..length].to_vec());
    held.changed = true;
}

/// Hand back out of the 창고 what a 4/3 withdrawal is asking for.
///
/// The request is two `u32`s: the slot and how many of it. The title's own later
/// build writes them in `NETWORK::PacketWrite_Inventory`'s third case -
/// `writeNet32(cursor + page * 16)` and then `writeNet32` of the count it was
/// asked for - and reads the answer in `ProcPacket_Inventory`'s: a result, a
/// message, and then a whole item, which goes straight to the bag through
/// `BagItem::NewBagNetItem(&item, item.count, 1)` before it asks for the listing
/// again. So the answer is the item leaving, and the count inside that record is
/// what actually arrives.
///
/// Taking fewer than the slot holds leaves the rest behind; taking all of it
/// empties the slot. A slot with nothing in it, or one this has no row for, is
/// answered with a count of zero, which is where `0x333fc` gives up and what
/// `NewBagNetItem` adds nothing for.
fn hero5_withdraw(body: &[u8]) -> Vec<u8> {
    const SLOT: usize = 0;
    const HOW_MANY: usize = 4;

    let field = |at: usize| body.get(at..at + 4).map(|word| u32::from_be_bytes(word.try_into().unwrap()));

    let (Some(slot), Some(asked)) = (field(SLOT), field(HOW_MANY)) else {
        tracing::debug!("영웅서기5 asked its 창고 for something this could not read: {body:02x?}");
        return 0u32.to_be_bytes().to_vec();
    };

    let mut held = HERO5_WAREHOUSE.lock();
    let Some(record) = held.rows.get_mut(slot as usize) else {
        tracing::debug!("영웅서기5 asked its 창고 for slot {slot}, which it is not holding");
        return 0u32.to_be_bytes().to_vec();
    };

    let holding = u32::from_be_bytes(record[..4].try_into().unwrap());
    let taken = asked.min(holding);

    let mut handed = record.clone();
    handed[..4].copy_from_slice(&taken.to_be_bytes());

    if taken >= holding {
        held.rows.remove(slot as usize);
    } else {
        record[..4].copy_from_slice(&(holding - taken).to_be_bytes());
    }
    held.changed = true;

    handed
}

/// The listing 4/6 answers with: what the 창고 is holding.
///
/// A row count and then one row each: the slot it sits in, a state byte, and
/// the record itself, handed back as deposited.
///
/// Every slot is enumerated, not only the ones with something in them, because
/// `0x1507c` has a path for a row whose count is not positive (`0x156ec`, which
/// writes an empty slot) - so a listing that names every slot is what that path
/// is there for, and the count the title is handed (`0x1502c`, which keeps it at
/// `list + 4`) is then how big the 창고 is rather than how full. The first pass
/// sent only the occupied slots and the 창고 drew nothing; this is the other
/// reading of the same two fields.
///
/// The state byte lands in the third of the list's three parallel arrays -
/// `0x1546c` writes the item at `list + slot * 4 + 8`, the stack count at
/// `list + slot + 0x148` and this at `list + slot + 0x198`. It goes over as
/// [`HERO5_SLOT_HELD`] rather than zero because zero is what the title writes
/// there itself for a slot with nothing in it (`0x156ec`, the path `0x1507c`
/// takes when a row's count is not positive), so a zero would leave an occupied
/// slot carrying the mark of an empty one. That is the one value in the row this
/// has no capture of.
///
/// An empty 창고 is a count of zero, which is an empty 창고 rather than a broken
/// listing - `0x36674` compares the row it is on against the count before it
/// reads anything.
fn hero5_warehouse() -> Vec<u8> {
    let held = HERO5_WAREHOUSE.lock();
    let slots = held.rows.len().max(HERO5_WAREHOUSE_SLOTS);

    let mut listing = Vec::new();
    listing.extend_from_slice(&(slots as u32).to_be_bytes());

    for slot in 0..slots {
        listing.extend_from_slice(&(slot as u32).to_be_bytes());

        match held.rows.get(slot) {
            Some(record) => {
                listing.push(HERO5_SLOT_HELD);
                listing.extend_from_slice(record);
            }
            // A slot with nothing in it: `0x333fc` reads the count and gives up
            // on it being zero, and `0x1507c` takes that row to `0x156ec`, which
            // is where it writes an empty slot.
            None => {
                listing.push(0);
                listing.extend_from_slice(&0u32.to_be_bytes());
            }
        }
    }

    listing
}

/// Whether this frame is one 영웅서기5's 창고 is behind, and the 창고 has not
/// been read in yet.
///
/// Three: 4/1 adds to it, 4/3 takes from it and 4/6 answers out of it. Every
/// other frame here, and every other title's, is none of its business.
pub fn hero5_warehouse_needs_loading(request: &[u8]) -> bool {
    const HEADER: usize = 20;
    const SERVICE_AT: usize = 4;
    const SERVICE: &[u8] = b"G1000157";

    if request.len() < HEADER || u32::from_be_bytes([request[0], request[1], request[2], request[3]]) as usize != request.len() {
        return false;
    }

    if &request[SERVICE_AT..SERVICE_AT + SERVICE.len()] != SERVICE {
        return false;
    }

    let field = |at: usize| u32::from_be_bytes([request[at], request[at + 1], request[at + 2], request[at + 3]]);

    matches!((field(12), field(16)), (4, 1) | (4, 3) | (4, 6)) && !HERO5_WAREHOUSE.lock().loaded
}

/// Bring 영웅서기5's 창고 in from what was kept.
///
/// The records back to back, each behind a `u32` of its own length, as
/// [`hero5_warehouse_to_keep`] wrote them. Anything that does not read back that
/// way is dropped rather than half-read: a 창고 short of a row is better than a
/// listing whose rows have slid.
pub fn load_hero5_warehouse(kept: &[u8]) {
    let mut held = HERO5_WAREHOUSE.lock();
    held.loaded = true;

    let mut at = 0;
    while at + 4 <= kept.len() {
        let length = u32::from_be_bytes(kept[at..at + 4].try_into().unwrap()) as usize;
        at += 4;

        let Some(record) = kept.get(at..at + length) else {
            tracing::debug!("영웅서기5's kept 창고 ends inside a row");
            held.rows.clear();
            return;
        };

        held.rows.push(record.to_vec());
        at += length;
    }
}

/// What to keep of 영웅서기5's 창고, or `None` if nothing has changed.
///
/// Each record behind a `u32` of its own length, because the records are not all
/// one size - a piece of equipment carries [`HERO5_EQUIPMENT_TAIL`] more than a
/// consumable, and the name in front of that is as long as the name is.
pub fn hero5_warehouse_to_keep() -> Option<Vec<u8>> {
    let mut held = HERO5_WAREHOUSE.lock();
    if !held.changed {
        return None;
    }
    held.changed = false;

    let mut kept = Vec::new();
    for record in &held.rows {
        kept.extend_from_slice(&(record.len() as u32).to_be_bytes());
        kept.extend_from_slice(record);
    }

    Some(kept)
}

/// The list 6/3 hands the bag, for a purchase of `product`.
///
/// A row count and then one row each, which `0x333fc` reads as a `u32` of how
/// many, the item table and the row in it as one byte apiece, and a name. What
/// `0x35b88` does with a row is not draw it - `0x35ea4` hands it straight to the
/// bag - so this list is the purchase arriving rather than a receipt for it,
/// which is why a count of zero left the 엘릭서 paid for and undelivered.
///
/// The name is left empty. `0x35c1a` draws a row by asking the item table for
/// the name at that row rather than by what the reply carried, and a zero-length
/// blob reads nothing and moves the cursor nowhere.
///
/// The four 유물함 are answered with what they drew rather than with the box -
/// see [`HERO5_BOX_DRAWS`] - which is what the title's own description of them,
/// 랜덤 아이템을 획득합니다, says a purchase of one is for.
///
/// A product this has no row for - and a 6/3 that names no product at all - is
/// answered with an empty list rather than a guessed one. `0x35c1a` compares the
/// row it is on against the count before reading anything, so an empty list is a
/// list.
fn hero5_delivery(product: Option<u32>) -> Vec<u8> {
    let mut list = Vec::new();

    // A 유물함 is bought for what is in it, so it is the set that goes over
    // rather than the box - see [`HERO5_BOX_DRAWS`].
    if let Some(&(_, pool)) = HERO5_BOX_DRAWS.iter().find(|(id, _)| Some(*id) == product) {
        let drawn: &[(u8, u8, [u8; HERO5_TABLE_STATS])] = match pool {
            Hero5Pool::Trinket => &HERO5_BOX_TRINKETS[next_draw(HERO5_BOX_TRINKETS.len() as u32) as usize..][..1],
            Hero5Pool::Heroic => &HERO5_BOX_HEROIC[next_draw(HERO5_BOX_HEROIC.len() as u32) as usize..][..1],
            Hero5Pool::Legendary => &HERO5_BOX_LEGENDARY[next_draw(HERO5_BOX_LEGENDARY.len() as u32) as usize..][..1],
            Hero5Pool::Set => &HERO5_BOX_SETS[next_draw(HERO5_BOX_SETS.len() as u32) as usize],
        };

        list.extend_from_slice(&(drawn.len() as u32).to_be_bytes());
        for (table, row, stats) in drawn {
            list.extend_from_slice(&1u32.to_be_bytes());
            list.push(*table);
            list.push(*row);
            list.extend_from_slice(&0u32.to_be_bytes());
            list.extend_from_slice(&hero5_equipment_tail(stats));
        }

        return list;
    }

    let Some(&(_, row, many)) = HERO5_SHOP_ROWS.iter().find(|(id, _, _)| Some(*id) == product) else {
        list.extend_from_slice(&0u32.to_be_bytes());
        return list;
    };

    list.extend_from_slice(&1u32.to_be_bytes());
    list.extend_from_slice(&many.to_be_bytes());
    list.push(HERO5_ITEM_TABLE);
    list.push(row);
    list.extend_from_slice(&0u32.to_be_bytes());

    list
}

/// The [`HERO5_EQUIPMENT_TAIL`] bytes a piece of equipment carries past its
/// name, for the row of the item table `stats` came out of.
///
/// The tail is the item the title holds, from `+0x134` to `+0x16e`, and
/// `0x32dfc` is what writes it: three `u64`s, then `+0x14c` and `+0x14e` as
/// `u16`s, two bytes, three more `u16`s, then bytes from `+0x158`, `+0x159`,
/// `+0x15a`, `+0x15b`, `+0x15e`, `+0x15c`, `+0x15f`, `+0x15d` and `+0x160`, and
/// finally `+0x161` up to `+0x16d`.
///
/// `+0x14c` to `+0x160` is [`HERO5_TABLE_STATS`] laid down byte for byte, which
/// is what a row of `item_NN.dat` carries: 드루이안워커 is `item_08.dat` row 50
/// and its bytes give 43 at `+0x152`, 43 at `+0x154` and 66 at `+0x159`, which
/// is the 물리방어 43, 마법방어 43, 제한레벨 66 the title prints for it.
///
/// The four that looked out of order in the middle are the three (option,
/// value) pairs: the item keeps its three option ids together and its three
/// values together, and the wire puts them back in pairs - which is the order a
/// row already has them in. So the row's last nine bytes, grade included, go
/// over exactly as they are.
///
/// The three `u64`s stay zero: `0x343ee` writes the clock into the item's
/// `+0x134` and `+0x13c` on finding them zero, so zero is the unset value the
/// title stamps itself. `+0x161` to `+0x164` stay zero and `+0x165` to `+0x169`
/// go over as `0xff`, because that is what `0xdf88` leaves behind after copying
/// a table row - it clears `+0x164` and fills the five slots there with -1.
fn hero5_equipment_tail(stats: &[u8; HERO5_TABLE_STATS]) -> [u8; HERO5_EQUIPMENT_TAIL] {
    /// The empty option slot.
    const NO_OPTION: u8 = 0xff;

    let word = |at: usize| u16::from_le_bytes([stats[at], stats[at + 1]]).to_be_bytes();

    let mut tail = Vec::with_capacity(HERO5_EQUIPMENT_TAIL);

    // `+0x134`, `+0x13c`, `+0x144` - the stamps the title writes itself.
    tail.extend_from_slice(&[0; 24]);

    // `+0x14c` to `+0x157`.
    tail.extend_from_slice(&word(0));
    tail.extend_from_slice(&word(2));
    tail.push(stats[4]);
    tail.push(stats[5]);
    tail.extend_from_slice(&word(6));
    tail.extend_from_slice(&word(8));
    tail.extend_from_slice(&word(10));

    // The grade, the 제한레벨, one more, and the three (option, value) pairs.
    tail.extend_from_slice(&stats[HERO5_STATS_GRADE..]);

    // `+0x161` to `+0x164`, and then the five slots `0xdf88` fills with -1.
    tail.extend_from_slice(&[0; 4]);
    tail.extend_from_slice(&[NO_OPTION; 5]);

    // `+0x16a` and `+0x16c`.
    tail.extend_from_slice(&[0; 4]);

    tail.try_into().expect("the tail is written field by field to its own length")
}

/// Which byte of a row's [`HERO5_TABLE_STATS`] is its grade.
///
/// 0 노멀, 1 레어, 2 에픽, 3 영웅, 4 전설, and 5 to 18 the fourteen sets - see
/// [`HERO5_BOX_DRAWS`]. `ItemInfo::GetGradeColorTag` in the title's own later
/// build is what colours a name by it.
const HERO5_STATS_GRADE: usize = 12;

/// Which byte of a row's [`HERO5_TABLE_STATS`] is its 제한레벨.
const HERO5_STATS_LEVEL: usize = 13;

pub fn lgt_local_hero5_response(request: &[u8]) -> Option<Vec<u8>> {
    /// A length, the service code, a command and a sub-command - and the least
    /// `0x38498` will look at, which drops anything under 20 bytes.
    const HEADER: usize = 20;
    const SERVICE_AT: usize = 4;
    const SERVICE: &[u8] = b"G1000157";
    const COMMAND_AT: usize = 12;
    const SUB_AT: usize = 16;

    /// Each step this has been read for, and the `u32`s its handler takes off
    /// the reply. The first is always the result, of which only zero is not an
    /// error, and the second always the length of the message the error paths
    /// draw, which is zero because there is no error. The rest is that step's
    /// own, and zero unless a zero there would be an answer rather than an
    /// absence.
    const STEPS: [(u32, u32, &[u32]); 12] = [
        (0, 2, &[]),
        (1, 1, &[0, 0, HERO5_PING_SECONDS]),
        (1, 3, &[0, 0]),
        (4, 1, &[0, 0]),
        (4, 3, &[0, 0]),
        (4, 6, &[0, 0]),
        (4, 7, &[0, 0, 0]),
        (5, 1, &[0, 0]),
        (5, 2, &[0, 0, 0]),
        (6, 2, &[0, 0]),
        (6, 3, &[0, 0]),
        (7, 1, &[0, 0, 0]),
    ];

    if request.len() < HEADER {
        return None;
    }

    let field = |at: usize| u32::from_be_bytes([request[at], request[at + 1], request[at + 2], request[at + 3]]);

    if field(0) as usize != request.len() || &request[SERVICE_AT..SERVICE_AT + SERVICE.len()] != SERVICE {
        return None;
    }

    let (command, sub) = (field(COMMAND_AT), field(SUB_AT));
    let (_, _, values) = STEPS.iter().find(|(c, s, _)| *c == command && *s == sub)?;

    // A 1/1 without the field is not a 1/1 that says the number is gone, so an
    // empty read leaves what the last one gave.
    if (command, sub) == (1, 1)
        && let number = hero5_subscriber(request)
        && !number.is_empty()
    {
        *HERO5_SUBSCRIBER.lock() = number;
    }

    let mut response = Vec::with_capacity(HEADER + values.len() * 4);
    response.extend_from_slice(&0u32.to_be_bytes());
    response.extend_from_slice(SERVICE);
    response.extend_from_slice(&command.to_be_bytes());
    response.extend_from_slice(&sub.to_be_bytes());
    for value in *values {
        response.extend_from_slice(&value.to_be_bytes());
    }

    // 1/3's last field is the blob the 창고 greets by name, not a `u32`, and a
    // length of zero there is the greeting with nothing in front of it.
    if (command, sub) == (1, 3) {
        let name = HERO5_SUBSCRIBER.lock();
        response.extend_from_slice(&(name.len() as u32).to_be_bytes());
        response.extend_from_slice(&name);
    }

    // 6/3's last field is a row count and the rows themselves - the purchase
    // being handed over, which is what the request's own field names.
    if (command, sub) == (6, 3) {
        let product = request.get(HEADER..HEADER + 4).map(|id| u32::from_be_bytes(id.try_into().unwrap()));
        response.extend_from_slice(&hero5_delivery(product));
    }

    // A granted 4/1 is the item leaving the title's own bag, so the 창고 takes
    // it here rather than on the listing that follows - by then the bag has
    // already let go of it.
    if (command, sub) == (4, 1) {
        hero5_deposit(&request[HEADER..]);
    }

    // 4/3's last field is the item itself: a withdrawal hands the record back
    // and the title puts that in the bag, so this is the item leaving the 창고.
    if (command, sub) == (4, 3) {
        response.extend_from_slice(&hero5_withdraw(&request[HEADER..]));
    }

    // 4/6's last field is a row count and the rows: what the 창고 is holding.
    if (command, sub) == (4, 6) {
        response.extend_from_slice(&hero5_warehouse());
    }

    let length = response.len() as u32;
    response[..4].copy_from_slice(&length.to_be_bytes());

    Some(response)
}

/// The answer to 오셔너스's cash purchase, whose gateway is the one it opens.
///
/// 오셔너스 (`0002D6C4`) confirms a CASH tab purchase behind a dialogue that
/// says the item costs real money - "실제 현금 %d원의 추가정보이용료 …
/// 구입하시겠습니까?" - and on 예 it dials `203.231.235.183:12343` and writes a
/// 44-byte record behind `WPBill_Write`'s 108-byte header:
///
/// ```text
/// [0..4]    "GLSN"
/// [4..8]    u32  812, the service the title bills against
/// [8..12]   u32  812 again
/// [12..20]  two zero u32s
/// [20..24]  two zero u16s
/// [24..28]  u32  a handset value the record carries verbatim
/// [28..44]  16 bytes taken straight off the handset's model buffer, which is
///           its ten-byte model followed by the first six of its number
/// ```
///
/// The record is little-endian: `0x1f92c` appends each field with a plain
/// `memcpy`, and `0x1f908` reads one back the same way, so what is in memory is
/// what is on the wire. The last field is the title's own sloppiness rather
/// than a protocol - it appends `0x10` bytes from the model buffer at
/// `0x150017c` and the number that follows it simply comes along.
///
/// `0x20708` is the builder, and it sets `[ctx+0xd18]` before it returns. That
/// flag is what the reply parser at `0x20c70` branches on: with it set, state 4
/// reads two `u32`s - the first into `[ctx+0x10]`, the second nowhere - and
/// zero is the only value it accepts. Zero moves the state machine to 8 and
/// sets `[ctx+0xd1a]`, which is the purchase having gone through; one through
/// five raise the carrier's error dialogue, and anything above is dropped.
///
/// So the whole answer is two little-endian `u32`s, the first of them zero.
pub fn lgt_local_oceanus_response(request: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"GLSN";
    /// The record's whole length, header and both handset fields.
    const PURCHASE_REQUEST: usize = 44;
    /// The service id at `[4]`, repeated at `[8]`.
    const SERVICE_AT: usize = 4;
    const SERVICE_REPEAT_AT: usize = 8;
    const SERVICE: u32 = 812;

    /// The one value `[ctx+0x10]` reads as the purchase having gone through.
    const GRANTED: u32 = 0;

    if request.len() != PURCHASE_REQUEST || !request.starts_with(TAG) {
        return None;
    }

    let field = |at: usize| u32::from_le_bytes([request[at], request[at + 1], request[at + 2], request[at + 3]]);
    if field(SERVICE_AT) != SERVICE || field(SERVICE_REPEAT_AT) != SERVICE {
        return None;
    }

    let mut response = Vec::with_capacity(8);
    response.extend_from_slice(&GRANTED.to_le_bytes());
    // The second word is read into a stack slot the parser never looks at
    // again; it only has to be there for the read to complete.
    response.extend_from_slice(&0u32.to_le_bytes());

    Some(response)
}

/// The answer to the record 오셔너스 writes once its purchase is granted.
///
/// With the purchase answered the title writes a second, shorter record on the
/// same socket and then waits to be told the reply has arrived - it registers a
/// read callback with `MC_netSetReadCB` rather than polling, so a gateway that
/// says nothing leaves it on 수신중 forever:
///
/// ```text
/// [0..4]    "GLSN"
/// [4..8]    u32  the message the record carries, 48
/// [8..12]   two zero u16s
/// [12..20]  the caller's own trailer
/// ```
///
/// `0x2080c` builds the first twelve bytes - the tag, the message its caller
/// passes, and the two `u16`s at `[ctx+0]` and `[ctx+2]`, the second of them
/// being zero is what ends the record there - and the wrapper that called it
/// appends the trailer before `0x20e8c` puts it on the wire.
///
/// The answer arrives in three reads, each one armed by `0x20e44(n)` with the
/// count the state before it worked out, and the state machine walks
/// 4 -> 6 -> 7 -> 8:
///
/// - state 4 reads twelve bytes: two `u32`s, then `[ctx+0]` and `[ctx+2]`.
///   `0x20cfe` branches on `[ctx+2]`, which is a body length rather than an
///   error: zero goes to state 6 and arms four bytes, anything else goes to
///   state 5 and arms that many bytes and four more.
/// - state 6 reads those four as a `u32` into `[ctx+0x18]`, moves to state 7
///   and arms a read of exactly that many bytes.
/// - state 7 sets `[ctx+0xd1a]` and moves to state 8, which is the walk being
///   over and the purchase settled.
///
/// So the whole answer is twelve bytes, a length, and a body of that length.
/// The length has to be positive: `0x20e44(0)` would arm a zero-byte read, and
/// `0x20c00` counts a read that reaches nothing as a retry and gives up after
/// 199 of them. Four bytes is the smallest body that avoids it.
///
/// Reaching state 8 is not the end of it. The title polls its own billing
/// context, and `0x21150` dispatches state 8 to `0x21184`, which sees
/// `[ctx+0xd1a]` set and - `[ctx+0xd18]` having been cleared when the poll
/// handed the purchase to `0x222b8` - calls `0x2108c` with `[ctx+0x14] + 1`,
/// the second `u32` of the twelve plus one.
///
/// That sum is the step, and the step is the whole of it. `0x2108c` rereads
/// the first `u16` of state 7's body only to decide whether to stop - `0xffff`
/// runs `0x20fec` and `0x20b24`, which close the carrier's progress dialogue
/// and tear the socket down, and `0x3e9` is the carrier's error - and
/// otherwise hands the step to `0x21e34`, whose switch is the conversation:
/// 2 and 4 resend the ids at `[0x1500204+0x6c]` and `+0x70`, 6 gives up, and
/// `0x31` is `0x21a9c`, which is the purchase being applied.
///
/// So the reply names the message it answers. The title sent 48, the reply
/// carries 48, and the step is 49 - `0x21a9c`. There it rewinds two bytes and
/// rereads that same `u16`: zero sets `[ctx+0xd19]` and takes the grant, which
/// reads the item out of `[0x1509f08+0x150]`, hands it to `0x44020` with the
/// kind its id resolves to, calls `0x20b24` itself to tear the socket down and
/// raises its own completed-purchase dialogue. Anything else there walks the
/// title back out without the item.
///
/// A zero second `u32` makes the step 1, which `0x21e34`'s switch does not
/// cover, so the walk ended with the socket closed and nothing bought - the
/// purchase went through and the item never arrived.
pub fn lgt_local_oceanus_settled_response(request: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"GLSN";
    /// The record's whole length, the builder's twelve and the caller's eight.
    const SETTLED_REQUEST: usize = 20;
    /// The message id at `[4]`, which is what makes this the settlement.
    const MESSAGE_AT: usize = 4;
    const MESSAGE: u32 = 48;

    /// What state 4 reads: a `u32` it discards, the `u32` at `ANSWERS_AT` that
    /// becomes the step, and two `u16`s, the last of them a body length that
    /// stays zero so the walk takes state 6 rather than state 5.
    const WALK: usize = 12;
    /// Where in that header the message being answered goes - the second
    /// `u32`, which `0x21184` runs as `+ 1`.
    const ANSWERS_AT: usize = 4;
    /// The body state 6 asks for, kept to the smallest a zero-byte read rules
    /// out.
    const BODY: u32 = 4;
    /// The `u16` `0x2108c` reads out of that body and `0x21a9c` rereads: zero
    /// is neither the close nor the carrier's error, and it is what `0x21a9c`
    /// takes as the purchase being good for the item.
    const GRANTED: u16 = 0;

    if request.len() != SETTLED_REQUEST || !request.starts_with(TAG) {
        return None;
    }

    let message = u32::from_le_bytes([
        request[MESSAGE_AT],
        request[MESSAGE_AT + 1],
        request[MESSAGE_AT + 2],
        request[MESSAGE_AT + 3],
    ]);
    if message != MESSAGE {
        return None;
    }

    let mut response = alloc::vec![0u8; WALK];
    // The second `u32` is the message this answers. `0x21184` runs it as
    // `+ 1`, so naming 48 here is what reaches `0x21a9c` and applies the item.
    response[ANSWERS_AT..ANSWERS_AT + 4].copy_from_slice(&MESSAGE.to_le_bytes());
    response.extend_from_slice(&BODY.to_le_bytes());
    response.extend_from_slice(&GRANTED.to_le_bytes());
    response.resize(WALK + 4 + BODY as usize, 0);

    Some(response)
}

/// The answer that lets 창세기전3 에피소드4's data-server session move on.
///
/// 에피소드4 (`000323E0`) opens a data server session before its first screen,
/// like the two episodes before it, but writes nothing they would recognise -
/// no magic, and the only number it puts through `MC_utilHtonl` is the length
/// in front:
///
/// ```text
/// [0..4]   u32 big-endian     the whole frame's length
/// [4..8]   u32 little-endian  the message id this exchange is tagged with
/// [8..12]  u32 little-endian  the message, 104 for the opening
/// [12..16] u32 little-endian  34
/// [16..20] u32 little-endian  0
/// [20..40] char[20]           the subscriber's number, zero-padded
/// ```
///
/// The builder is the state machine at `0x291b8`, whose state 2 opens the
/// message with an id of 1, writes the three numbers, writes the number into a
/// twenty-byte field and sends - forty bytes, which is the `MC_utilHtonl(0x28)`
/// the log shows in front of it.
///
/// What the answer has to carry is in the poll loop at `0x296c0`. For the state
/// this opening belongs to it copies **two little-endian `u32`s** off the front
/// of what arrived and requires the second to equal `[0x151b7b4]`, which is the
/// id `0x29514` registered when the message was opened - 1. Only then does it
/// hand the pair on and let the session advance; a mismatch is read and
/// dropped, which is where leaving the frame unanswered leaves it.
///
/// So the answer is that id in both of the two words it reads. Which of them
/// the title lines its buffer up on - the length is stripped or it is not -
/// does not have to be settled to answer it, because either way the word it
/// compares is the id.
///
/// Answering the opening moves the walk on, and the title's next frame is the
/// same header with nothing behind it - a length of twelve, its own id, and
/// message 1 - so the id is not the opening's to hardcode and comes back off
/// whichever frame is in hand.
///
/// `None` for anything that is not one of that walk's frames: it has to declare
/// its own length, carry an id, and be a message the walk is made of.
pub fn lgt_local_genesis3_episode4_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The big-endian length, and the little-endian words behind it.
    const HEADER: usize = 4;
    /// The id and the message, which every frame in this walk carries.
    const LEAST_REQUEST: usize = HEADER + 8;
    /// The messages the walk is made of: 104 opens the session, and 1 is what
    /// the title sends next once the opening is answered.
    const MESSAGES: [u32; 2] = [104, 1];

    if request.len() < LEAST_REQUEST || u32::from_be_bytes([request[0], request[1], request[2], request[3]]) as usize != request.len() {
        return None;
    }

    let word = |at: usize| u32::from_le_bytes([request[at], request[at + 1], request[at + 2], request[at + 3]]);
    let exchange = word(4);
    if exchange == 0 || !MESSAGES.contains(&word(8)) {
        return None;
    }

    // The id in both words: the title compares the second of the two it copies
    // off the front of the answer, and which of them that is depends on whether
    // its buffer starts at the length or behind it. Either way it is the id.
    let mut response = Vec::with_capacity(LEAST_REQUEST);
    response.extend_from_slice(&(LEAST_REQUEST as u32).to_be_bytes());
    response.extend_from_slice(&exchange.to_le_bytes());
    response.extend_from_slice(&exchange.to_le_bytes());

    Some(response)
}

/// The answers 창세기전3 에피소드2's data-server session asks for.
///
/// 에피소드2 (`00029B30`) opens a session before its first screen and will not
/// start without one. Every frame either way is the same ten byte header, all
/// of it big-endian - the title puts each field through `MC_utilHtons` and
/// `MC_utilHtonl` itself, and patches the length in at `0x1ee98` as everything
/// written since the header:
///
/// ```text
/// [0..2]   u16  0xFACB, the magic its reader checks at 0x1ef4c
/// [2..6]   u32  the payload's length, the ten header bytes not counted
/// [6..10]  u32  the message
/// ```
///
/// Behind the header goes one big-endian `u32`, and it has to be there. The
/// title reads a message as `0x1f604` opens it and `0x1f6ac` or `0x1fcdc`
/// finishes it, and between them that is a word taken off the payload - the
/// verdict, zero being the value both of them carry on from. It is also what
/// keeps the stream lined up: the reader takes the frame's declared length out
/// of what it has and then reads the word, so an answer that declares less than
/// it is read for leaves the count short by the difference and every frame
/// after it starts mid-word. Answering the login with a single byte did exactly
/// that - the count went three under, the next answer was never recognised as a
/// frame at all, and the title stopped on "FOUND CORRUPT DATA!!!".
///
/// The walk, as those two readers take it:
///
/// * the title sends **100**, its login - the subscriber's number and the
///   handset model as length-prefixed strings, then 102 and a zero byte;
/// * **101** answers it. `0x1f6ac` moves the session to its third state on a
///   zero verdict and the title draws "접속 성공"; anything else is an error it
///   draws instead;
/// * the title then sends **700**, and **701** answers it the same way, read at
///   `0x1fd10`. That one ends the walk: the title reaches
///   "SMS 수신동의가 완료되었습니다." and its step machine runs out at five,
///   which is where 에피소드4 ends up too;
/// * **1** is the session's own keep-alive, and `0x1efe4` answers one with an
///   empty 1 of its own. Answering the title's keeps the link from going quiet
///   between the steps of the walk.
///
/// `None` for anything that is not one of these: it has to carry the magic,
/// declare its own length, and be a message this walk is made of.
pub fn lgt_local_genesis3_episode2_response(request: &[u8]) -> Option<Vec<u8>> {
    /// The header, and the least a frame can be.
    const HEADER: usize = 10;
    /// The `u16` `0x1ef4c` compares the front of every frame against.
    const MAGIC: u16 = 0xfacb;
    /// The keep-alive, which is answered with itself.
    const KEEP_ALIVE: u32 = 1;
    /// The login, and the message that answers it.
    const LOGIN: u32 = 100;
    const LOGIN_ANSWER: u32 = 101;
    /// The step after the login, and the message that answers it.
    const STEP: u32 = 700;
    const STEP_ANSWER: u32 = 701;
    /// The word both answers are read for. Zero is the verdict the title
    /// carries on from; every other value is an error it draws instead.
    const GOOD: u32 = 0;

    if request.len() < HEADER || u16::from_be_bytes([request[0], request[1]]) != MAGIC {
        return None;
    }

    let length = u32::from_be_bytes([request[2], request[3], request[4], request[5]]) as usize;
    if length.checked_add(HEADER) != Some(request.len()) {
        return None;
    }

    let message = u32::from_be_bytes([request[6], request[7], request[8], request[9]]);
    let (answer, verdict) = match message {
        KEEP_ALIVE => (KEEP_ALIVE, None),
        LOGIN => (LOGIN_ANSWER, Some(GOOD)),
        STEP => (STEP_ANSWER, Some(GOOD)),
        _ => return None,
    };

    let body = verdict.map(u32::to_be_bytes);
    let body = body.as_ref().map(|word| &word[..]).unwrap_or(&[]);

    let mut reply = Vec::with_capacity(HEADER + body.len());
    reply.extend_from_slice(&MAGIC.to_be_bytes());
    reply.extend_from_slice(&(body.len() as u32).to_be_bytes());
    reply.extend_from_slice(&answer.to_be_bytes());
    reply.extend_from_slice(body);

    Some(reply)
}

/// What 서든어택 포켓's cash shop is answered with.
///
/// 서든어택 포켓 (`0002AC6E`) opens `BillSocket://121.78.119.73:18009` around a
/// cash purchase and writes eighteen bytes: a big-endian `u16` command and a
/// sixteen byte payload. Two commands turn up, one after the other:
///
/// ```text
/// 29 10 | 00 00 00 0b | "01046119269" 00                            the purchase
/// 3c 10 | 00 00 00 32 | 00 00 00 32 | 00 00 00 32 | 00 00 00 28     the medals held
/// ```
///
/// The second is four big-endian `u32`s - 50, 50, 50, 40 - which are the four
/// medal counts the shop screen shows, so it reports what the player holds.
///
/// Both are answered the same way: the title reads four single bytes off the
/// stream, makes a `u32` of them big end first, and asks for exactly that many
/// more. Answered with the ez-i SDK's own twenty-byte reply - which is what
/// they got before this, the chain having nothing that knew either command -
/// it read `0c 00 01 00` as a length of 201326848 and waited on a reply that
/// size, which the handset showed as 캐쉬아이템 구매중입니다 and then as a 예
/// button that did nothing.
///
/// What the body has to say is one thing: **nothing went wrong.** The shop is a
/// state machine dispatched at `0x5bbdc`, and its waiting state hands the reply
/// to a parser and then reads one field back:
///
/// ```asm
/// 5d344  bx  ip                  ; the reply parser
/// 5d348  ldr r6, [r4, #8]        ; the shop's fields
/// 5d350  ldr sl, [r6, r3, lsl #2]
/// 5d354  cmp sl, #0
/// 5d358  bne 0x5d7a4             ; anything but zero is a failure
/// ```
///
/// Zero falls through to state 13, which draws 구매 성공 and grants the item;
/// anything else is state 14, 구매 실패하였습니다. So a body of zeros is the
/// granted answer, and a handset run settled it: answered zero, the shop drew
/// 구매 성공, and the medal report that followed carried `00 00 00 96` where it
/// had carried `00 00 00 32` - 화랑메달 50 to 150, the hundred the item is. The
/// three non-zero bodies tried alongside it each drew 구매 실패하였습니다.
///
/// The body's length is not something the title tells us; sixty-four bytes is
/// what that run proved a parse it satisfies, and the parser reads its verdict
/// off the front rather than counting to the end.
fn lgt_local_sudden_attack_response(request: &[u8]) -> Option<Vec<u8>> {
    if request.len() != SUDDEN_ATTACK_FRAME {
        return None;
    }

    let command = u16::from_be_bytes([request[0], request[1]]);
    if command != SUDDEN_ATTACK_PURCHASE && command != SUDDEN_ATTACK_MEDALS {
        return None;
    }

    let mut reply = Vec::with_capacity(4 + SUDDEN_ATTACK_BODY);
    reply.extend_from_slice(&(SUDDEN_ATTACK_BODY as u32).to_be_bytes());
    reply.resize(4 + SUDDEN_ATTACK_BODY, 0);

    Some(reply)
}

/// Bytes in one of 서든어택 포켓's frames: a `u16` command and a sixteen byte
/// payload, whichever command it is.
const SUDDEN_ATTACK_FRAME: usize = 18;

/// 서든어택 포켓's cash purchase.
const SUDDEN_ATTACK_PURCHASE: u16 = 0x2910;

/// 서든어택 포켓 reporting the four medal counts the player holds.
const SUDDEN_ATTACK_MEDALS: u16 = 0x3c10;

/// Bytes in 서든어택 포켓's answer, after the length that describes them.
const SUDDEN_ATTACK_BODY: usize = 64;

/// What 놈ZERO's authentication is answered with.
///
/// 놈ZERO (`0002AC84`, a WIPI-C `Clet`) writes seventy-one bytes through
/// `MC_netBillWrite` before it will start, and waits on `MC_netBillRead` for a
/// reply that never came - the gateway had nothing shaped for it, so the title
/// sat on its authentication screen.
///
/// The request declares its own length and carries what the handset is:
///
/// ```text
/// [0..2]   u16 LE - 71, the frame's own length
/// [2]      6      - the command
/// [3]      1      - its sub-command
/// [4..32]  the subscriber number, NUL-padded
/// [32..48] the handset model, NUL-padded ("Emulator" here)
/// [48..58] the platform version, NUL-padded ("1.0.0" here)
/// [58..62] u32 LE 21873
/// [62..64] u16 LE 1
/// [64..66] u16 LE 96
/// [66]     0
/// [67..69] ff ff
/// [69..71] u16 LE 31
/// ```
///
/// **The reply's framing is settled**, read off the title's own receive at
/// `0x44fac`: it takes a four byte header first - `0x44f98` returns that 4 -
/// and then as many more bytes as `0x44f9c` derives from it,
///
/// ```asm
/// 44f9c  ldr  r3, [r0, #0x40]
/// 44f9e  ldr  r3, [r3]
/// 44fa0  ldrb r0, [r3, #1]
/// 44fa2  ldrb r2, [r3]
/// 44fa4  lsls r0, r0, #8
/// 44fa6  orrs r0, r2      ; the u16 at [0..2]
/// 44fa8  subs r0, #4      ; less the header it has already
/// ```
///
/// so a reply is a `u16` little-endian total length, two more header bytes, and
/// a body - the same shape the request has.
///
/// **The command is not echoed.** The two header bytes after the length are a
/// command id, read big-endian out of `[2..4]`, and the title dispatches on it
/// at `0x422b8` against four values and no others:
///
/// ```asm
/// 422cc  ldrsb r5, [r0, r5]   ; body[0], the status - negative is a refusal
/// 422d0  bge   #0x422e8
/// 422e8  ldrb  r3, [r0, #3]
/// 422ea  ldrb  r2, [r0, #2]
/// 422ec  lsls  r0, r3, #8
/// 422ee  ldr   r3, [pc, #0x78]  ; 0x103
/// 422f0  orrs  r0, r2
/// 422f2  cmp   r0, r3
/// 422f4  beq   #0x42310
/// ...                            ; 0x101, 0x107 -> 0x42310, 0x201 -> 0x42354
/// 4230e  b     #0x42362          ; anything else is dropped where it stands
/// ```
///
/// Echoing the request's own `0x106` fell off that end: the frame was read,
/// accepted as a success, and then discarded without ever reaching a handler,
/// which is exactly what the title looked like from outside - a request
/// answered, and an authentication screen that never moved.
///
/// A reply's id is its request's plus one. The two the title sends are
/// [`NOMZERO_AUTHENTICATE`] from state 4 (`0x424b8`, `r1 = 0x83 << 1`) and
/// [`NOMZERO_PURCHASE`] from state 7 (`0x4250c`, `r1 = 0x80 << 2`), and the
/// four ids it accepts are their answers.
///
/// **The body is settled too**, by what `0x42310` does with it:
///
/// ```asm
/// 42310  ldr   r5, [r1, #8]     ; the body
/// 42316  adds  r1, r5, #1
/// 4231a  bl    #0x41738         ; saves body[1..0x29] to "audio.adt"
/// 42322  beq   #0x4234c
/// 42326  adds  r3, #0x29
/// 42328  ldrb  r3, [r3]         ; body[0x29]
/// 42332  cmp   r3, #0
/// 42334  bne   #0x42340
/// 4233c  movs  r3, #0xc         ; zero  - ask the player to buy (state 12)
/// 42346  movs  r3, #9           ; other - say so and go in (state 9)
/// ```
///
/// so the body is forty-two bytes: a status, forty that are only ever written
/// through to the title's own cache file, and a flag. `0x41738` does not read
/// the forty - it writes them out and answers whether the file took them - and
/// the two fields the cache is later checked on (`0x416e0`) are filled from the
/// session rather than from us, so zeros there cost nothing.
///
/// The flag is answered non-zero: state 9 shows what it has to show and returns
/// 2 from the tick, which is what `0xbd54` takes as "go in". Zero would instead
/// raise the purchase prompt, and that only leads back to the same place by way
/// of a second exchange.
fn lgt_local_nomzero_response(request: &[u8]) -> Option<Vec<u8>> {
    if request.len() < NOMZERO_HEADER || u16::from_le_bytes([request[0], request[1]]) as usize != request.len() {
        return None;
    }

    // The id the title dispatches on, the way `0x422e8` assembles it.
    let command = u16::from_be_bytes([request[3], request[2]]);

    let mut body = match command {
        NOMZERO_AUTHENTICATE if request.len() == NOMZERO_AUTHENTICATE_FRAME => {
            let mut body = vec![0u8; NOMZERO_AUTH_BODY];
            // Not zero, so the title says the player is already entitled and
            // goes in, rather than raising a prompt to buy.
            body[NOMZERO_ENTITLED] = 1;
            body
        }
        // 0x42354 only moves the state on; it never looks at what came with it.
        NOMZERO_PURCHASE => vec![0u8; NOMZERO_ACK_BODY],
        _ => return None,
    };

    // A reply's id is its request's plus one.
    let answer = command + 1;

    let mut reply = Vec::with_capacity(NOMZERO_HEADER + body.len());
    reply.extend_from_slice(&((NOMZERO_HEADER + body.len()) as u16).to_le_bytes());
    reply.push(answer as u8);
    reply.push((answer >> 8) as u8);
    reply.append(&mut body);

    Some(reply)
}

/// Bytes of 놈ZERO's reply that come before its body: the `u16` length the
/// title reads first, and the command id after it.
const NOMZERO_HEADER: usize = 4;

/// 놈ZERO asking whether the player may play, sent from state 4.
const NOMZERO_AUTHENTICATE: u16 = 0x0106;

/// Bytes in that request, which is the same every time it is sent.
const NOMZERO_AUTHENTICATE_FRAME: usize = 71;

/// 놈ZERO asking to be charged, sent from state 7 once a player accepts the
/// prompt. Reachable only through the answer this does not give.
const NOMZERO_PURCHASE: u16 = 0x0200;

/// Bytes of body in the answer to [`NOMZERO_AUTHENTICATE`]: a status, forty
/// that go to the title's cache file, and the flag below.
const NOMZERO_AUTH_BODY: usize = 0x2a;

/// Where in that body the title reads whether the player is already entitled.
const NOMZERO_ENTITLED: usize = 0x29;

/// Bytes of body in the answer to [`NOMZERO_PURCHASE`]. Only the status at
/// `[0]` is read, and only for its sign.
const NOMZERO_ACK_BODY: usize = 4;

/// The answer to a billing request, whichever of these protocols it is in.
///
/// Tried in order of how specific each shape is: the `0xffff`-framed message,
/// then the pipe-delimited cash record, then the GAMEVIL packet, then the
/// big-endian record, then the length-prefixed command, then the text record,
/// then the tagged record. `None` when a request is none of them, which is not
/// something to answer with a guess.
///
/// The three packet shapes cannot be mistaken for one another. Each declares its
/// own length, and no two of them read that length the same way: a length that
/// is the record in hand as a `u16` one end first is thousands the other way
/// round, and a `u32` that is the record in hand has the command in its high
/// half read as a `u16`.
/// 게임빌2010슈퍼사커's purchase, answered the way its own reader reads it.
///
/// The title buys G포인트 over a billing socket and, unanswered, sits on
/// `접속중 입니다` until it gives up with `에러가 발생했습니다 [에러코드: -1]`.
/// Two frames carry the purchase, both `[u16 LE length][u16 LE command]` with
/// the reply's command one above the request's:
///
/// ```text
/// 15 00 06 01  0b 43 00 00  "010419325556" 00  00   the purchase
/// 0c 00 04 01  0b 43 00 00  00 00 00 00              what follows it
/// ```
///
/// Its own receiver is not the one its certificate exchange uses. `0x7c9a0`
/// reads the byte behind the header as a **signed status and requires it above
/// zero** - `0x7c9da` takes anything at or below it to `OnError`, which is
/// where the `-1` on screen comes from - where the certificate's receiver
/// (`0x83844`) only refuses a negative one. A reply that satisfies the one is
/// refused by the other, which is why these need answering here rather than by
/// the 제노니아 packet matcher that was claiming them.
///
/// Each command's reader then takes a fixed body, and a reply that comes up
/// short leaves it reading whatever was in the buffer already:
///
/// - `0x0107` (`0x7cae6`) takes sixteen bytes, then a `u16`. `0x8020` keeps the
///   sixteen as a string and the `u16` beside it.
/// - `0x0105` (`0x7cb9a`) allocates two hundred bytes and copies that many. Its
///   first field is the NUL-terminated EUC-KR line the title shows in the
///   dialog that ends the purchase, which is what `구매가 완료되었습니다` is
///   doing here.
///
/// What the service granted went with the service: the balance the shop shows
/// is `[[0x1400054] + 0x1134]`, and nothing in either reply reaches it - the
/// title credits it on a `0x0301` it is not sent here. So this takes the
/// purchase off the error and says so, and leaves the balance where it was.
///
/// `None` for anything that is not one of those two: the length has to be the
/// frame in hand, the command one of the two, and the frame has to carry this
/// title's own id where both of them carry it.
pub fn lgt_local_supersoccer_response(request: &[u8]) -> Option<Vec<u8>> {
    /// Little end first, like the rest of this title's fields.
    const TITLE_ID: u32 = 0x0000_430b;
    const TITLE_ID_OFFSET: usize = 4;

    /// Above zero, which is what `0x7c9da` wants.
    const GRANTED: u8 = 1;

    const PURCHASE_REQUEST: u16 = 0x0106;
    const PURCHASE_SIZE: usize = 21;
    /// Sixteen bytes and a `u16`.
    const PURCHASE_BODY: usize = 18;

    const SETTLE_REQUEST: u16 = 0x0104;
    const SETTLE_SIZE: usize = 12;
    const SETTLE_BODY: usize = 200;

    /// 구매가 완료되었습니다. in EUC-KR, which is the encoding the title draws.
    const DONE: &[u8] = b"\xb1\xb8\xb8\xc5\xb0\xa1 \xbf\xcf\xb7\xe1\xb5\xc7\xbe\xfa\xbd\xc0\xb4\xcf\xb4\xd9.";

    if request.len() < TITLE_ID_OFFSET + 4 || u16::from_le_bytes([request[0], request[1]]) as usize != request.len() {
        return None;
    }

    let id = u32::from_le_bytes([request[4], request[5], request[6], request[7]]);
    if id != TITLE_ID {
        return None;
    }

    let (answer, body) = match (u16::from_le_bytes([request[2], request[3]]), request.len()) {
        (PURCHASE_REQUEST, PURCHASE_SIZE) => (PURCHASE_REQUEST + 1, PURCHASE_BODY),
        (SETTLE_REQUEST, SETTLE_SIZE) => (SETTLE_REQUEST + 1, SETTLE_BODY),
        _ => return None,
    };

    let length = 4 + 1 + body;
    let mut response = vec![0u8; length];
    response[0..2].copy_from_slice(&(length as u16).to_le_bytes());
    response[2..4].copy_from_slice(&answer.to_le_bytes());
    response[4] = GRANTED;

    if answer == SETTLE_REQUEST + 1 {
        response[5..5 + DONE.len()].copy_from_slice(DONE);
    }

    Some(response)
}

/// 디스트로이어's 정식사용자 인증.
///
/// 디스트로이어 (`00030A1A`) sets `BILL_GW_IP` to `wipigw.ez-i.co.kr:20000`,
/// resolves it, and writes one frame before its 인증을 진행중입니다. spinner:
///
/// ```text
/// 01 | 37 | 0b 00 "01020663410" | 03 00 "LGT" | 05 00 "WIPIC"
///         | 0c 00 <디스트로이어, EUC-KR> | 05 00 "1.0.0"
///         | 0d 00 "62LN 20101020" | 08 00 "Emulator"
///         | 02 | 00 | 08 00 "00030A1A" | 05 00 "BASIC" | 00
/// ```
///
/// The first two bytes are the framing, not payload. `binary.mod`'s reader at
/// `0x6abe0` takes a signed byte as the number of messages that follow, then
/// per message a signed byte id which it turns into `id - 1` and runs through a
/// sixty-entry jump table at `0x93128`. Entry `0x36` - id `0x37` - is the one
/// this title's builder at `0x67c0c` writes and then arms with
/// `set_expected(0x37)`.
///
/// That handler, at `0x67b18`, reads **one signed byte** and nothing else, and
/// hands it to the dispatcher at `0x67848`, which stores it and moves the
/// network state from 2 to 3. The 인증 screen at `0x11700` then reads it back
/// through `get_result` and decides:
///
/// ```asm
/// 11708  bl   get_result          ; (s16) of the byte the reply carried
/// 11710  cmp  r0, #0
/// 11712  beq  0x11718              ; 사용자인증에 성공하였습니다.
/// 11714  cmp  r0, #2
/// 11716  bne  0x1171e              ; 사용자인증에 실패하였습니다.
/// ```
///
/// So zero and two are the two verdicts it carries on from, and anything else
/// is the failure screen. Unanswered it retries every five seconds and gives up
/// after five - which is the spinner that never ends, since nothing here knew
/// the frame.
///
/// The reply is therefore the same three-byte shape the request opens with: one
/// message, id `0x37`, and the verdict byte.
fn lgt_local_destroyer_response(request: &[u8]) -> Option<Vec<u8>> {
    // The framing bytes and the first field's own length.
    if request.len() < 4 {
        return None;
    }

    if request[0] != DESTROYER_MESSAGE_COUNT || request[1] != DESTROYER_AUTHENTICATE {
        return None;
    }

    // The subscriber number leads the payload; its length has to fit what
    // arrived, or this is some other title's frame that opens the same way.
    let length = u16::from_le_bytes([request[2], request[3]]) as usize;
    if request.len() < 4 + length {
        return None;
    }

    // And the platform it names is what tells the frame apart for certain.
    if !request.windows(DESTROYER_PLATFORM.len()).any(|window| window == DESTROYER_PLATFORM) {
        return None;
    }

    Some(vec![DESTROYER_MESSAGE_COUNT, DESTROYER_AUTHENTICATE, DESTROYER_GRANTED])
}

/// One message in the frame, which is all 인증 ever sends or expects.
const DESTROYER_MESSAGE_COUNT: u8 = 1;

/// The 정식사용자 인증 message id, both ways.
const DESTROYER_AUTHENTICATE: u8 = 0x37;

/// The verdict `0x11710` falls through on. Two passes as well; zero is the
/// plainer of the two.
const DESTROYER_GRANTED: u8 = 0;

/// The platform name the ez-i request carries, third field in.
const DESTROYER_PLATFORM: &[u8] = b"WIPIC";

/// 알바타이쿤2's 최초 인증.
///
/// 알바타이쿤2 (`0002D4D0`) writes one record before its
/// 최초 게임실행 시 서버접속을 통한 인증 notice can clear:
///
/// ```text
/// 00 00 00 28 | "IR\t01799998888\talba2\t1.0.2\t4730084\tWIPIC"
/// ```
///
/// A big-endian `u32` length and then a tab-separated ASCII record, built
/// from the format string at `0xbfeb4` - `"IR\t%s\t%s\t%s\t4730084\tWIPIC"` -
/// with the subscriber, the title's own name and its version filled in. Its
/// siblings in the same table are `SG`, `GG`, `AP`, `SM` and the `CASH`/`CREG`
/// records, so `IR` is one command of a family the title speaks.
///
/// What the answer has to say is: nothing. `binary.mod` dispatches a reply on
/// the request type it is answering - a jump table at `0xb6e5c` indexed by
/// `type - 0x11` - and the builder at `0x226da` shows `IR` is type `0x1f`,
/// whose entry is `0x22bf8`:
///
/// ```asm
/// 22bf8  movs r3, #4
/// 22bfa  ldr  r0, [pc]        ; 정상적으로 인증처리 되었습니다. 감사합니다
/// 22c04  bl   0x17920         ; put it on the screen
/// ```
///
/// It reads nothing out of the reply. Every other command in the table
/// compares the record against a token first (`SGOK`, `GGOK`, `APOK`, `SMOK`);
/// this one does not, so the only thing missing was a reply at all - without
/// one the title sits on its notice and the gateway logged the record
/// unanswered.
///
/// Answered in the shape the request itself uses, and with the token this
/// family would carry.
fn lgt_local_albatycoon2_response(request: &[u8]) -> Option<Vec<u8>> {
    let record = request.get(ALBATYCOON2_LENGTH_SIZE..)?;

    if u32::from_be_bytes([request[0], request[1], request[2], request[3]]) as usize != record.len() {
        return None;
    }

    if !record.starts_with(ALBATYCOON2_AUTHENTICATE) {
        return None;
    }

    // The platform the record names, as in every other title's frame here: the
    // command alone is two letters and would claim too much.
    if !record.windows(ALBATYCOON2_PLATFORM.len()).any(|window| window == ALBATYCOON2_PLATFORM) {
        return None;
    }

    let mut reply = Vec::with_capacity(ALBATYCOON2_LENGTH_SIZE + ALBATYCOON2_GRANTED.len());
    reply.extend_from_slice(&(ALBATYCOON2_GRANTED.len() as u32).to_be_bytes());
    reply.extend_from_slice(ALBATYCOON2_GRANTED);

    Some(reply)
}

/// The big-endian `u32` in front of every record this title sends.
const ALBATYCOON2_LENGTH_SIZE: usize = 4;

/// The 인증 command, with the separator its record uses.
const ALBATYCOON2_AUTHENTICATE: &[u8] = b"IR\t";

/// The platform field the record carries, last of the six.
const ALBATYCOON2_PLATFORM: &[u8] = b"\tWIPIC";

/// What the reply says. The `0x1f` handler reads none of it; this is the token
/// its siblings (`SGOK`, `GGOK`, `APOK`, `SMOK`) are shaped like.
const ALBATYCOON2_GRANTED: &[u8] = b"IROK";

pub fn response(request: &[u8]) -> Option<Vec<u8>> {
    lgt_local_granted_response(request)
        .or_else(|| lgt_local_cash_response(request))
        // Before the 제노니아 packet matcher, which claims these by their length
        // and answers with a status this title's purchase receiver refuses.
        .or_else(|| lgt_local_supersoccer_response(request))
        .or_else(|| lgt_local_gamevil_packet_response(request))
        .or_else(|| lgt_local_subscriber_record_response(request))
        .or_else(|| lgt_local_command_tag_response(request))
        .or_else(|| lgt_local_fixed_block_response(request))
        .or_else(|| lgt_local_ens_record_response(request))
        .or_else(|| lgt_local_big_endian_record_response(request))
        .or_else(|| lgt_local_major_minor_response(request))
        .or_else(|| lgt_local_text_record_response(request))
        .or_else(|| lgt_local_tagged_record_response(request))
        .or_else(|| lgt_local_opcode_header_response(request))
        .or_else(|| lgt_local_marked_command_response(request))
        .or_else(|| lgt_local_biochronicle_response(request))
        .or_else(|| lgt_local_destinia_response(request))
        .or_else(|| lgt_local_blademaster3_response(request))
        .or_else(|| lgt_local_id_framed_response(request))
        .or_else(|| lgt_local_tera_response(request))
        .or_else(|| lgt_local_hero5_response(request))
        .or_else(|| lgt_local_oceanus_response(request))
        .or_else(|| lgt_local_oceanus_settled_response(request))
        .or_else(|| lgt_local_genesis3_episode4_response(request))
        .or_else(|| lgt_local_genesis3_episode2_response(request))
        .or_else(|| lgt_local_sudden_attack_response(request))
        .or_else(|| lgt_local_nomzero_response(request))
        .or_else(|| lgt_local_destroyer_response(request))
        .or_else(|| lgt_local_albatycoon2_response(request))
}

#[cfg(test)]
mod tests {
    use alloc::{vec, vec::Vec};

    use super::*;

    /// 알바타이쿤2's 인증 record, captured off its billing socket.
    const ALBATYCOON2_AUTH: [u8; 44] = [
        0x00, 0x00, 0x00, 0x28, // the record's length, big end first
        0x49, 0x52, 0x09, // "IR" and a tab
        0x30, 0x31, 0x37, 0x39, 0x39, 0x39, 0x39, 0x38, 0x38, 0x38, 0x38, 0x09, // the subscriber
        0x61, 0x6c, 0x62, 0x61, 0x32, 0x09, // "alba2"
        0x31, 0x2e, 0x30, 0x2e, 0x32, 0x09, // "1.0.2"
        0x34, 0x37, 0x33, 0x30, 0x30, 0x38, 0x34, 0x09, // the service id
        0x57, 0x49, 0x50, 0x49, 0x43, // "WIPIC"
    ];

    /// The record is answered in its own framing, and the `0x1f` handler reads
    /// none of the body - what it needed was a reply at all.
    #[test]
    fn 알바타이쿤2s_인증_is_answered_in_its_own_framing() {
        let reply = response(&ALBATYCOON2_AUTH).expect("the 인증 record is answered");

        assert_eq!(u32::from_be_bytes([reply[0], reply[1], reply[2], reply[3]]) as usize, reply.len() - 4);
        assert_eq!(&reply[4..], b"IROK");
    }

    /// A record whose length word disagrees with what arrived, or that names
    /// another platform, keeps falling through the chain.
    #[test]
    fn a_record_that_is_not_알바타이쿤2s_is_not_claimed() {
        let mut short = ALBATYCOON2_AUTH;
        short[3] = 0x27;
        assert_eq!(lgt_local_albatycoon2_response(&short), None);

        let mut other = ALBATYCOON2_AUTH;
        other[39..44].copy_from_slice(b"XXXXX");
        assert_eq!(lgt_local_albatycoon2_response(&other), None);

        assert_eq!(lgt_local_albatycoon2_response(&ALBATYCOON2_AUTH[..4]), None);
    }

    /// 디스트로이어's 인증 frame, captured off its billing socket.
    const DESTROYER_AUTH: [u8; 93] = [
        0x01, 0x37, // one message, the 인증 id
        0x0b, 0x00, 0x30, 0x31, 0x30, 0x32, 0x30, 0x36, 0x36, 0x33, 0x34, 0x31, 0x30, // the subscriber
        0x03, 0x00, 0x4c, 0x47, 0x54, // "LGT"
        0x05, 0x00, 0x57, 0x49, 0x50, 0x49, 0x43, // "WIPIC"
        0x0c, 0x00, 0xb5, 0xf0, 0xbd, 0xba, 0xc6, 0xae, 0xb7, 0xce, 0xc0, 0xcc, 0xbe, 0xee, // 디스트로이어
        0x05, 0x00, 0x31, 0x2e, 0x30, 0x2e, 0x30, // "1.0.0"
        0x0d, 0x00, 0x36, 0x32, 0x4c, 0x4e, 0x20, 0x32, 0x30, 0x31, 0x30, 0x31, 0x30, 0x32, 0x30, // "62LN 20101020"
        0x08, 0x00, 0x45, 0x6d, 0x75, 0x6c, 0x61, 0x74, 0x6f, 0x72, // "Emulator"
        0x02, 0x00, // a constant and a flag
        0x08, 0x00, 0x30, 0x30, 0x30, 0x33, 0x30, 0x41, 0x31, 0x41, // the aid
        0x05, 0x00, 0x42, 0x41, 0x53, 0x49, 0x43, // "BASIC"
        0x00,
    ];

    /// One message, the same id, and a verdict `0x11710` carries on from.
    #[test]
    fn 디스트로이어s_인증_is_answered_with_the_verdict_its_screen_reads() {
        let reply = response(&DESTROYER_AUTH).expect("the 인증 frame is answered");

        assert_eq!(reply, vec![0x01, 0x37, 0x00]);
        // `0x11710` takes zero or two and draws the failure screen on anything
        // else, so the verdict has to be one of the two.
        assert!(reply[2] == 0 || reply[2] == 2);
    }

    /// The framing bytes alone are not enough to claim a frame: another title's
    /// message that opens the same way keeps falling through the chain.
    #[test]
    fn a_frame_without_디스트로이어s_platform_is_not_claimed() {
        let mut other = DESTROYER_AUTH;
        other[22..27].copy_from_slice(b"XXXXX");

        assert_eq!(lgt_local_destroyer_response(&other), None);
        assert_eq!(lgt_local_destroyer_response(&DESTROYER_AUTH[..3]), None);
    }

    /// 슈퍼사커's two purchase frames, captured off its socket.
    const SUPERSOCCER_PURCHASE: [u8; 21] = [
        0x15, 0x00, // u16 LE length
        0x06, 0x01, // u16 LE command
        0x0b, 0x43, 0x00, 0x00, // the title's own id
        0x30, 0x31, 0x30, 0x34, 0x31, 0x39, 0x33, 0x32, 0x35, 0x35, 0x36, 0x00, // the subscriber
        0x00,
    ];

    const SUPERSOCCER_SETTLE: [u8; 12] = [0x0c, 0x00, 0x04, 0x01, 0x0b, 0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

    /// Both readers take a fixed body and a status above zero; the 제노니아
    /// matcher that was claiming these gave them thirty-two bytes and a zero,
    /// which is the `에러코드: -1` the shop was ending on.
    #[test]
    fn 슈퍼사커s_purchase_is_answered_at_the_length_its_reader_takes() {
        let purchase = response(&SUPERSOCCER_PURCHASE).expect("the purchase is answered");
        assert_eq!(u16::from_le_bytes([purchase[0], purchase[1]]) as usize, purchase.len());
        assert_eq!(u16::from_le_bytes([purchase[2], purchase[3]]), 0x0107);
        assert!(purchase[4] as i8 > 0, "0x7c9da refuses a status at or below zero");
        // Sixteen bytes and a u16 behind the status.
        assert_eq!(purchase.len(), 4 + 1 + 18);

        let settle = response(&SUPERSOCCER_SETTLE).expect("what follows it is answered");
        assert_eq!(u16::from_le_bytes([settle[0], settle[1]]) as usize, settle.len());
        assert_eq!(u16::from_le_bytes([settle[2], settle[3]]), 0x0105);
        assert!(settle[4] as i8 > 0);
        assert_eq!(settle.len(), 4 + 1 + 200);

        // Its first field is the line the closing dialog shows, in the EUC-KR
        // the title draws rather than in UTF-8.
        let line = &settle[5..];
        let line = &line[..line.iter().position(|&x| x == 0).expect("the line is terminated")];
        assert_eq!(
            line,
            b"\xb1\xb8\xb8\xc5\xb0\xa1 \xbf\xcf\xb7\xe1\xb5\xc7\xbe\xfa\xbd\xc0\xb4\xcf\xb4\xd9."
        );
    }

    /// Keyed on the title's own id where both frames carry it, so the matcher
    /// cannot claim another title's frame of the same length.
    #[test]
    fn a_frame_without_슈퍼사커s_id_is_left_alone() {
        let mut other = SUPERSOCCER_PURCHASE;
        other[4] = 0x0c;
        assert!(lgt_local_supersoccer_response(&other).is_none());

        let mut short = SUPERSOCCER_SETTLE;
        short[0] = 0x0d;
        assert!(lgt_local_supersoccer_response(&short).is_none());
    }

    /// The eighteen bytes 서든어택 포켓 writes when a cash purchase is
    /// confirmed, captured off its socket.
    const SUDDEN_ATTACK_PURCHASE_FRAME: [u8; 18] = [
        0x29, 0x10, // u16 BE command
        0x00, 0x00, 0x00, 0x0b, // u32 BE, the subscriber number's length
        0x30, 0x31, 0x30, 0x34, 0x36, 0x31, 0x31, 0x39, 0x32, 0x36, 0x39, // the subscriber number
        0x00, // the rest of the sixteen byte payload
    ];

    /// The eighteen that follow it, the four medal counts the shop shows.
    const SUDDEN_ATTACK_MEDAL_FRAME: [u8; 18] = [
        0x3c, 0x10, // u16 BE command
        0x00, 0x00, 0x00, 0x32, // 50
        0x00, 0x00, 0x00, 0x32, // 50
        0x00, 0x00, 0x00, 0x32, // 50
        0x00, 0x00, 0x00, 0x28, // 40
    ];

    /// The length the title reads off the front of a reply, the way it reads it:
    /// four bytes, big end first.
    fn framed_length(reply: &[u8]) -> usize {
        u32::from_be_bytes([reply[0], reply[1], reply[2], reply[3]]) as usize
    }

    #[test]
    fn 서든어택s_frames_are_answered_with_a_length_they_can_read() {
        // Both exchanges, because answering only the purchase leaves the shop
        // stopped on the medal report that follows it - which is what the
        // handset showed when only the first was answered.
        for frame in [SUDDEN_ATTACK_PURCHASE_FRAME, SUDDEN_ATTACK_MEDAL_FRAME] {
            let reply = response(&frame).unwrap();

            // The length is the body that follows it, so the read the title
            // issues next is one this can satisfy. The 201326848 it asked for
            // before was the ez-i answer's first four bytes read as one.
            assert_eq!(framed_length(&reply), reply.len() - 4);
        }
    }

    #[test]
    fn 서든어택s_purchase_is_answered_the_way_it_reads_as_granted() {
        let reply = response(&SUDDEN_ATTACK_PURCHASE_FRAME).unwrap();

        // `0x5d354` compares the field its parser filled against zero and takes
        // 구매 실패하였습니다 for anything else, so the whole body is zero.
        assert!(reply[4..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn 서든어택_answers_only_its_own_frames() {
        // A command neither of the two.
        let mut other = SUDDEN_ATTACK_PURCHASE_FRAME;
        other[0] = 0x29;
        other[1] = 0x11;
        assert!(lgt_local_sudden_attack_response(&other).is_none());

        // The right command in a frame that is not eighteen bytes.
        assert!(lgt_local_sudden_attack_response(&SUDDEN_ATTACK_PURCHASE_FRAME[..17]).is_none());
        assert!(lgt_local_sudden_attack_response(&SUDDEN_ATTACK_PURCHASE_FRAME[..4]).is_none());
    }

    /// The 71 bytes 놈ZERO writes before it will start, captured off its socket.
    fn nomzero_auth() -> Vec<u8> {
        let mut request = vec![0u8; 71];
        request[..2].copy_from_slice(&71u16.to_le_bytes());
        request[2] = 6;
        request[3] = 1;
        request[4..15].copy_from_slice(b"01041228783");
        request[32..40].copy_from_slice(b"Emulator");
        request[48..53].copy_from_slice(b"1.0.0");
        request[58..62].copy_from_slice(&21873u32.to_le_bytes());
        request[62..64].copy_from_slice(&1u16.to_le_bytes());
        request[64..66].copy_from_slice(&96u16.to_le_bytes());
        request[67..69].copy_from_slice(&[0xff, 0xff]);
        request[69..71].copy_from_slice(&31u16.to_le_bytes());

        request
    }

    /// The command id the title dispatches on, the way `0x422e8` reads it: the
    /// two header bytes after the length, big end last.
    fn nomzero_command(frame: &[u8]) -> u16 {
        u16::from_be_bytes([frame[3], frame[2]])
    }

    #[test]
    fn 놈zero_is_answered_in_the_shape_its_receive_reads() {
        let reply = response(&nomzero_auth()).unwrap();

        // `0x44f98` takes four bytes first and `0x44f9c` reads the u16 at [0..2]
        // as the whole frame, less those four. A length that did not describe
        // the reply would leave it waiting on bytes that never come.
        assert_eq!(u16::from_le_bytes([reply[0], reply[1]]) as usize, reply.len());
        assert!(reply.len() > NOMZERO_HEADER);
    }

    #[test]
    fn 놈zeros_answer_carries_an_id_its_dispatch_knows() {
        let reply = response(&nomzero_auth()).unwrap();

        // The request's own id back was the whole of why the title never moved:
        // `0x422b8` compares against 0x101, 0x103, 0x107 and 0x201, and drops
        // what matches none of them without telling anyone. 0x106 matched none.
        assert_ne!(nomzero_command(&reply), nomzero_command(&nomzero_auth()));
        assert!([0x101, 0x103, 0x107, 0x201].contains(&nomzero_command(&reply)));
    }

    #[test]
    fn 놈zero_is_told_the_player_is_already_entitled() {
        let reply = response(&nomzero_auth()).unwrap();
        let body = &reply[NOMZERO_HEADER..];

        // `0x422cc` reads the status signed and takes anything negative for a
        // refusal, and `0x42328` needs a body long enough to hold the flag.
        assert_eq!(body.len(), NOMZERO_AUTH_BODY);
        assert!((body[0] as i8) >= 0);

        // Zero there raises the purchase prompt instead of going in.
        assert_ne!(body[NOMZERO_ENTITLED], 0);
    }

    #[test]
    fn 놈zeros_purchase_is_acknowledged_too() {
        // Only reachable if the prompt is ever raised, but `0x42354` moves the
        // state on when it arrives and stops where it stands when it does not.
        let mut purchase = vec![0u8; 16];
        purchase[..2].copy_from_slice(&16u16.to_le_bytes());
        purchase[2] = 0x00;
        purchase[3] = 0x02;

        let reply = response(&purchase).unwrap();
        assert_eq!(nomzero_command(&reply), 0x201);
        assert!((reply[NOMZERO_HEADER] as i8) >= 0);
    }

    #[test]
    fn 놈zero_answers_only_its_own_frames() {
        // A frame whose length field disagrees with the frame.
        let mut wrong_length = nomzero_auth();
        wrong_length[0] = 70;
        assert!(lgt_local_nomzero_response(&wrong_length).is_none());

        // The right length and a command that is neither of the two it sends.
        let mut other_command = nomzero_auth();
        other_command[2] = 7;
        assert!(lgt_local_nomzero_response(&other_command).is_none());

        // Its authentication is one fixed size, so a frame that is not that
        // size is not it however its header reads.
        let mut short = nomzero_auth()[..70].to_vec();
        short[..2].copy_from_slice(&70u16.to_le_bytes());
        assert!(lgt_local_nomzero_response(&short).is_none());
    }

    /// The 36-byte record 짜요짜요타이쿤4 opens with, captured off its socket.
    /// The 36-byte record 짜요짜요타이쿤4 opens with, captured off its socket.
    const ZZT4_OPENING: [u8; 36] = [
        0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x01, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    /// The 111-byte record 테라-영원의혼돈 writes when a purchase is confirmed.
    fn tera_purchase_request() -> Vec<u8> {
        let mut request = vec![0u8; 111];
        request[..3].copy_from_slice(b"LGT");
        request[3..14].copy_from_slice(&[0x14, 0x01, 0x1d, 0x00, 0x00, 0x00, 0x01, 0x84, 0x03, 0x00, 0x00]);
        request[14..25].copy_from_slice(b"0002B76D012");
        // 흡수링, the item bought, in EUC-KR.
        request[25..31].copy_from_slice(&[0xc8, 0xed, 0xbc, 0xf6, 0xb8, 0xb5]);
        request[65..70].copy_from_slice(b"41001");
        // 테라.
        request[70..74].copy_from_slice(&[0xc5, 0xd7, 0xb6, 0xf3]);

        request
    }

    #[test]
    fn a_purchase_is_answered_with_a_length_and_an_accepted_body() {
        let reply = lgt_local_tera_response(&tera_purchase_request()).unwrap();

        // The four bytes its state 4 reads, as the u32 its state 5 makes of them.
        assert_eq!(u32::from_le_bytes([reply[0], reply[1], reply[2], reply[3]]) as usize, 2);
        // The request byte come back, and the status `0x11a30c` takes.
        assert_eq!(&reply[4..], &[0x14, 0x01]);
        assert_eq!(response(&tera_purchase_request()), Some(reply));
    }

    #[test]
    fn a_record_that_is_not_that_purchase_is_left_unanswered() {
        // The tag, the length, and an app id where this record names one.
        let mut untagged = tera_purchase_request();
        untagged[0] = b'K';
        assert_eq!(lgt_local_tera_response(&untagged), None);

        assert_eq!(lgt_local_tera_response(&tera_purchase_request()[..110]), None);

        let mut unnamed = tera_purchase_request();
        unnamed[14] = 0;
        assert_eq!(lgt_local_tera_response(&unnamed), None);

        // Another transaction of this title's, whose handler takes a different
        // first byte than the one this answers with.
        let mut other = tera_purchase_request();
        other[3] = 0x1e;
        assert_eq!(lgt_local_tera_response(&other), None);
    }

    /// The 80-byte frame 영웅서기5 writes when its shop opens, captured off its
    /// billing socket.
    fn hero5_shop_request() -> Vec<u8> {
        let mut request = Vec::new();
        request.extend_from_slice(&80u32.to_be_bytes());
        request.extend_from_slice(b"G1000157");
        request.extend_from_slice(&1u32.to_be_bytes());
        request.extend_from_slice(&1u32.to_be_bytes());
        request.extend_from_slice(&16u32.to_be_bytes());
        request.extend_from_slice(b"01064256416\0\xb8\x0e\x40\x01");
        request.extend_from_slice(&8u32.to_be_bytes());
        request.extend_from_slice(b"Emulator");
        request.extend_from_slice(&300u32.to_be_bytes());
        request.extend_from_slice(&101u32.to_be_bytes());
        request.extend_from_slice(&16u32.to_be_bytes());
        request.extend_from_slice(b"HERO5.ALL.SS.000");

        assert_eq!(request.len(), 80);
        request
    }

    /// One of this title's frames: a header and nothing else, or with fields.
    fn hero5_frame(command: u32, sub: u32, tail: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(&((20 + tail.len()) as u32).to_be_bytes());
        frame.extend_from_slice(b"G1000157");
        frame.extend_from_slice(&command.to_be_bytes());
        frame.extend_from_slice(&sub.to_be_bytes());
        frame.extend_from_slice(tail);
        frame
    }

    #[test]
    fn each_step_is_answered_with_its_own_command_and_a_zero_result() {
        // The steps both flows walk: 1/1 on connecting, then the 창고's 1/3,
        // 5/1-5/2 and 창고관리's 4/1 and 4/7, the shop's 6/2-6/3, the 창고's
        // closing 7/1, and the 0/2 keep-alive that runs alongside all of it.
        // The field counts are what each handler reads off the reply; the two
        // whose last field is a list of its own have tests of their own.
        for (command, sub, fields) in [(0, 2, 0), (1, 1, 3), (4, 1, 2), (4, 7, 3), (5, 1, 2), (5, 2, 3), (6, 2, 2), (7, 1, 3)] {
            let request = hero5_frame(command, sub, &[]);
            let reply = lgt_local_hero5_response(&request).unwrap_or_else(|| panic!("{command}/{sub} unanswered"));

            // The length has to be the reply in hand, or `0x385b8` drops it, and
            // it has to clear the 20 bytes `0x38498` needs to read a command.
            assert_eq!(u32::from_be_bytes(reply[0..4].try_into().unwrap()) as usize, reply.len());
            assert_eq!(reply.len(), 20 + fields * 4);

            assert_eq!(&reply[4..12], b"G1000157");
            // The command and sub-command `0x38498` reads at [12] and [16].
            assert_eq!(u32::from_be_bytes(reply[12..16].try_into().unwrap()), command);
            assert_eq!(u32::from_be_bytes(reply[16..20].try_into().unwrap()), sub);
            // The result every handler reads first, then an empty message, then
            // whatever else that step takes - all zero but 1/1's interval.
            let zeroed = if (command, sub) == (1, 1) { &reply[20..28] } else { &reply[20..] };
            assert!(zeroed.iter().all(|&byte| byte == 0));

            assert_eq!(response(&request), Some(reply));
        }
    }

    /// 1/1's third field is the seconds `0x37cec` keeps as `value * 1000` and
    /// `0x392b8` halves to decide when to ping. Zero there is a ping per tick.
    #[test]
    fn the_first_step_hands_back_a_keep_alive_interval_of_its_own() {
        let reply = lgt_local_hero5_response(&hero5_shop_request()).unwrap();

        assert_eq!(u32::from_be_bytes(reply[28..32].try_into().unwrap()), HERO5_PING_SECONDS);
        assert_ne!(HERO5_PING_SECONDS, 0);
    }

    /// The keep-alive is a frame the title sends on a timer rather than one it
    /// waits on, so its answer is the bare header `0x33e58` reads nothing out of.
    #[test]
    fn the_keep_alive_is_answered_with_a_bare_header() {
        let ping = hero5_frame(0, 2, &[]);

        assert_eq!(lgt_local_hero5_response(&ping), Some(ping));
    }

    /// 영웅서기5's 창고 is one static, so the tests that put things in it take
    /// turns rather than racing each other for what it is holding.
    static HERO5_WAREHOUSE_TEST: spin::Mutex<()> = spin::Mutex::new(());

    /// The 4/1 frame the 창고 deposit capture carried: one 얇은 가죽, item table
    /// 13 row 38, and the `u32` of the title's own that follows the record.
    fn hero5_deposit_request() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes());
        body.push(13);
        body.push(38);
        body.extend_from_slice(&9u32.to_be_bytes());
        body.extend_from_slice(b"\xbe\xe3\xc0\xba\x20\xb0\xa1\xc1\xd7");
        body.extend_from_slice(&0u32.to_be_bytes());

        let request = hero5_frame(4, 1, &body);
        assert_eq!(request.len(), 43);
        request
    }

    /// A deposit is the item leaving the bag, so the 창고 has to hand it back -
    /// and hand it back as it took it, or every row after it has slid.
    #[test]
    fn the_warehouse_hands_back_what_it_was_deposited() {
        let _turn = HERO5_WAREHOUSE_TEST.lock();
        load_hero5_warehouse(&[]);
        HERO5_WAREHOUSE.lock().rows.clear();

        // Nothing deposited still enumerates every slot, each of them empty.
        let empty = lgt_local_hero5_response(&hero5_frame(4, 6, &[])).unwrap();
        assert_eq!(u32::from_be_bytes(empty[28..32].try_into().unwrap()) as usize, HERO5_WAREHOUSE_SLOTS);
        assert_eq!(empty.len(), 32 + HERO5_WAREHOUSE_SLOTS * 9);
        assert!(
            empty[32..]
                .chunks(9)
                .enumerate()
                .all(|(slot, row)| { row[..4] == (slot as u32).to_be_bytes() && row[4] == 0 && row[5..] == [0; 4] })
        );

        // The deposit itself is answered with a result and a message, and
        // nothing else - `0x362a4` reads no more than that.
        let granted = lgt_local_hero5_response(&hero5_deposit_request()).unwrap();
        assert_eq!(granted.len(), 28);
        assert!(granted[20..].iter().all(|&byte| byte == 0));

        // And now slot zero carries it, byte for byte without the `u32` that
        // followed it, and every other slot is still empty.
        let listing = lgt_local_hero5_response(&hero5_frame(4, 6, &[])).unwrap();
        assert_eq!(u32::from_be_bytes(listing[28..32].try_into().unwrap()) as usize, HERO5_WAREHOUSE_SLOTS);
        assert_eq!(&listing[32..36], 0u32.to_be_bytes());
        assert_eq!(listing[36], HERO5_SLOT_HELD);
        // 1 is 거래중, which the title will not let out of the 창고.
        assert_ne!(HERO5_SLOT_HELD, 1);
        assert_eq!(&listing[37..56], &hero5_deposit_request()[20..39]);
        assert_eq!(&listing[56..60], 1u32.to_be_bytes());
        assert_eq!(listing[60], 0);
        assert_eq!(listing.len(), 32 + 5 + 19 + (HERO5_WAREHOUSE_SLOTS - 1) * 9);
        assert_eq!(u32::from_be_bytes(listing[0..4].try_into().unwrap()) as usize, listing.len());

        HERO5_WAREHOUSE.lock().rows.clear();
    }

    /// A record is as long as what it declares, and equipment carries the tail
    /// a consumable does not.
    #[test]
    fn a_record_is_measured_by_what_it_declares() {
        // The captured deposit: table 13, a nine-byte name, no tail.
        assert_eq!(hero5_record_length(&hero5_deposit_request()[20..]), Some(19));

        // The same row in an equipment table carries 58 bytes more.
        let mut gear = hero5_deposit_request()[20..].to_vec();
        gear[4] = HERO5_LAST_EQUIPMENT_TABLE;
        assert_eq!(hero5_record_length(&gear), None);
        gear.resize(19 + HERO5_EQUIPMENT_TAIL, 0);
        assert_eq!(hero5_record_length(&gear), Some(19 + HERO5_EQUIPMENT_TAIL));

        // A count of zero is where `0x333fc` gives up, so that is the record.
        assert_eq!(hero5_record_length(&[0, 0, 0, 0]), Some(4));
        assert_eq!(hero5_record_length(&[0, 0, 0]), None);
    }

    /// What is kept reads back as what was held, and a truncated store is
    /// dropped rather than half-read.
    #[test]
    fn the_warehouse_survives_being_written_out_and_read_back() {
        let _turn = HERO5_WAREHOUSE_TEST.lock();
        HERO5_WAREHOUSE.lock().rows.clear();
        lgt_local_hero5_response(&hero5_deposit_request()).unwrap();
        lgt_local_hero5_response(&hero5_deposit_request()).unwrap();

        let kept = hero5_warehouse_to_keep().expect("a deposit changes the 창고");
        // Nothing more to write until something else changes.
        assert_eq!(hero5_warehouse_to_keep(), None);

        let listing = lgt_local_hero5_response(&hero5_frame(4, 6, &[])).unwrap();
        HERO5_WAREHOUSE.lock().rows.clear();
        load_hero5_warehouse(&kept);
        assert_eq!(lgt_local_hero5_response(&hero5_frame(4, 6, &[])).unwrap(), listing);

        // A store that ends inside a row is no 창고 at all rather than a 창고
        // whose rows have slid.
        HERO5_WAREHOUSE.lock().rows.clear();
        load_hero5_warehouse(&kept[..kept.len() - 1]);
        assert!(HERO5_WAREHOUSE.lock().rows.is_empty());

        HERO5_WAREHOUSE.lock().rows.clear();
    }

    /// 6/3 is the purchase arriving, not a receipt for it: `0x35ea4` hands each
    /// row of its list to the bag. An empty list is what left the 엘릭서 paid
    /// for and never delivered.
    #[test]
    fn a_purchase_is_answered_with_the_item_it_bought() {
        // The id the first shop capture's 6/2 and 6/3 carried: 엘릭서(20), which
        // the catalogues put at row 34 of `item_18.dat`, twenty of it.
        let reply = lgt_local_hero5_response(&hero5_frame(6, 3, &4u32.to_be_bytes())).unwrap();

        assert_eq!(u32::from_be_bytes(reply[0..4].try_into().unwrap()) as usize, reply.len());
        // Result, then an empty message, then one row.
        assert_eq!(&reply[20..28], [0; 8]);
        assert_eq!(u32::from_be_bytes(reply[28..32].try_into().unwrap()), 1);

        // How many, the item table and the row in it, and no name.
        assert_eq!(u32::from_be_bytes(reply[32..36].try_into().unwrap()), 20);
        assert_eq!(reply[36], HERO5_ITEM_TABLE);
        assert_eq!(reply[37], 34);
        assert_eq!(&reply[38..], 0u32.to_be_bytes());
    }

    /// Every product has one row, and one row only, and every row is in the one
    /// table `0xe748` numbers to 0x12.
    #[test]
    fn each_product_names_one_row_of_the_one_table_the_shop_sells_out_of() {
        for (id, row, many) in HERO5_SHOP_ROWS {
            assert!(many > 0 && row <= 42, "{id}");
            assert_eq!(HERO5_SHOP_ROWS.iter().filter(|(other, _, _)| *other == id).count(), 1, "{id}");

            // The four 유물함 hand over a set instead - they have their own test.
            if HERO5_BOX_DRAWS.iter().any(|(box_id, _)| *box_id == id) {
                continue;
            }

            let reply = lgt_local_hero5_response(&hero5_frame(6, 3, &id.to_be_bytes())).unwrap();

            assert_eq!(u32::from_be_bytes(reply[28..32].try_into().unwrap()), 1, "{id}");
            assert_eq!(u32::from_be_bytes(reply[32..36].try_into().unwrap()), many, "{id}");
            assert_eq!(reply[36], HERO5_ITEM_TABLE, "{id}");
            assert_eq!(reply[37], row, "{id}");
        }
    }

    /// A 유물함 is bought for what is in it: the reply carries what it drew out
    /// of its own pool, and the box's own row never goes over.
    #[test]
    fn a_box_delivers_what_it_drew_and_never_the_box() {
        for (id, pool) in HERO5_BOX_DRAWS {
            let mut seen = Vec::new();

            for _ in 0..4000 {
                let reply = lgt_local_hero5_response(&hero5_frame(6, 3, &id.to_be_bytes())).unwrap();

                assert_eq!(u32::from_be_bytes(reply[0..4].try_into().unwrap()) as usize, reply.len());
                assert_eq!(&reply[20..28], [0; 8], "{id}");

                // A set is four pieces; everything else is one.
                let rows = u32::from_be_bytes(reply[28..32].try_into().unwrap()) as usize;
                assert_eq!(rows, if pool == Hero5Pool::Set { 4 } else { 1 }, "{id}");

                let mut drew = Vec::new();
                let mut at = 32;
                for _ in 0..rows {
                    assert_eq!(u32::from_be_bytes(reply[at..at + 4].try_into().unwrap()), 1, "{id}");
                    let (table, row) = (reply[at + 4], reply[at + 5]);
                    // Equipment, which the 유물함's own table never is.
                    assert!(table <= HERO5_LAST_EQUIPMENT_TABLE, "{id}");
                    assert_ne!(table, HERO5_ITEM_TABLE, "{id}");
                    assert_eq!(&reply[at + 6..at + 10], 0u32.to_be_bytes(), "{id}");

                    let stats = hero5_pool_row(pool, table, row).unwrap_or_else(|| panic!("{id} drew {table}/{row}"));
                    assert_eq!(&reply[at + 10..at + 10 + HERO5_EQUIPMENT_TAIL], hero5_equipment_tail(&stats), "{id}");
                    // The grade the box was asked for.
                    match pool {
                        Hero5Pool::Trinket => assert!((2..=4).contains(&stats[HERO5_STATS_GRADE]), "{id}"),
                        Hero5Pool::Heroic => assert_eq!(stats[HERO5_STATS_GRADE], 3, "{id}"),
                        Hero5Pool::Legendary => assert_eq!(stats[HERO5_STATS_GRADE], 4, "{id}"),
                        Hero5Pool::Set => assert!((5..=18).contains(&stats[HERO5_STATS_GRADE]), "{id}"),
                    }

                    drew.push((table, row));
                    at += 10 + HERO5_EQUIPMENT_TAIL;
                }
                assert_eq!(at, reply.len(), "{id}");

                // A set arrives whole, in the order the set names it.
                if pool == Hero5Pool::Set {
                    assert!(
                        HERO5_BOX_SETS
                            .iter()
                            .any(|set| set.iter().map(|(t, r, _)| (*t, *r)).eq(drew.iter().copied())),
                        "{id} drew {drew:?}"
                    );
                }

                if !seen.contains(&drew) {
                    seen.push(drew);
                }
            }

            // Every row of the pool comes up.
            let pool_size = match pool {
                Hero5Pool::Trinket => HERO5_BOX_TRINKETS.len(),
                Hero5Pool::Heroic => HERO5_BOX_HEROIC.len(),
                Hero5Pool::Legendary => HERO5_BOX_LEGENDARY.len(),
                Hero5Pool::Set => HERO5_BOX_SETS.len(),
            };
            assert_eq!(seen.len(), pool_size, "{id}");
        }
    }

    /// The row of `pool` at that table and index, if it holds one.
    fn hero5_pool_row(pool: Hero5Pool, table: u8, row: u8) -> Option<[u8; HERO5_TABLE_STATS]> {
        let rows: Vec<(u8, u8, [u8; HERO5_TABLE_STATS])> = match pool {
            Hero5Pool::Trinket => HERO5_BOX_TRINKETS.to_vec(),
            Hero5Pool::Heroic => HERO5_BOX_HEROIC.to_vec(),
            Hero5Pool::Legendary => HERO5_BOX_LEGENDARY.to_vec(),
            Hero5Pool::Set => HERO5_BOX_SETS.iter().flatten().copied().collect(),
        };

        rows.iter().find(|(t, r, _)| (*t, *r) == (table, row)).map(|(_, _, stats)| *stats)
    }

    /// Every pool is what its grade says it is, and no pool is empty.
    #[test]
    fn every_pool_is_the_grade_it_is_drawn_for() {
        assert!(HERO5_BOX_TRINKETS.iter().all(|(table, _, _)| *table == 10));
        assert!(HERO5_BOX_HEROIC.iter().all(|(_, _, stats)| stats[HERO5_STATS_GRADE] == 3));
        assert!(HERO5_BOX_LEGENDARY.iter().all(|(_, _, stats)| stats[HERO5_STATS_GRADE] == 4));

        for set in HERO5_BOX_SETS {
            // 투구, 갑옷, 장갑, 신발 of one set, so one grade and one row.
            assert_eq!(set.map(|(table, _, _)| table), [5, 6, 7, 8]);
            assert!(set.iter().all(|(_, row, _)| *row == set[0].1));
            assert!(set.iter().all(|(_, _, stats)| stats[HERO5_STATS_GRADE] == set[0].2[HERO5_STATS_GRADE]));
            assert!((5..=18).contains(&set[0].2[HERO5_STATS_GRADE]));
        }

        for (id, _) in HERO5_BOX_DRAWS {
            assert!(HERO5_SHOP_ROWS.iter().any(|(product, _, _)| *product == id), "{id}");
        }
    }

    /// The tail is the item table's own bytes, in the order `0x32dfc` writes
    /// them - which is what a 껍데기 was missing.
    #[test]
    fn an_equipment_tail_carries_the_item_table_s_own_numbers() {
        // 드루이안워커, `item_08.dat` row 50: 물리방어 43 at `+0x152`, 마법방어
        // 43 at `+0x154`, 제한레벨 66 at `+0x159`.
        let stats: [u8; HERO5_TABLE_STATS] = [
            0x07, 0x00, 0x8d, 0x00, 0x01, 0x06, 0x2b, 0x00, 0x2b, 0x00, 0x15, 0x00, 0x00, 0x42, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        ];
        let tail = hero5_equipment_tail(&stats);

        assert_eq!(tail.len(), HERO5_EQUIPMENT_TAIL);
        assert_eq!(&tail[..24], [0; 24]);
        assert_eq!(u16::from_be_bytes(tail[0x152 - 0x134..0x154 - 0x134].try_into().unwrap()), 43);
        assert_eq!(u16::from_be_bytes(tail[0x154 - 0x134..0x156 - 0x134].try_into().unwrap()), 43);
        assert_eq!(tail[0x159 - 0x134], 66);
        // The grade and the three (option, value) pairs, straight off the row.
        assert_eq!(&tail[0x158 - 0x134..0x161 - 0x134], &stats[HERO5_STATS_GRADE..]);
        assert_eq!(&tail[0x165 - 0x134..0x16a - 0x134], [0xff; 5]);
        assert_eq!(&tail[0x16a - 0x134..], [0; 4]);
    }

    /// 1/3's last field is a blob, and the 창고 draws 15 bytes of it in front of
    /// "님 반갑습니다." - so it carries the number 1/1 came in with.
    #[test]
    fn the_greeting_step_carries_the_number_the_first_step_came_in_with() {
        lgt_local_hero5_response(&hero5_shop_request()).unwrap();
        let reply = lgt_local_hero5_response(&hero5_frame(1, 3, &[])).unwrap();

        // Result, message length, then the name's own length and the name.
        assert_eq!(&reply[20..28], [0; 8]);
        assert_eq!(u32::from_be_bytes(reply[28..32].try_into().unwrap()) as usize, reply.len() - 32);
        assert_eq!(&reply[32..], b"01064256416");

        // Never more than `0x37ef6` copies to `ctx + 0x351`.
        assert!(reply.len() - 32 <= HERO5_NICKNAME);
        assert_eq!(u32::from_be_bytes(reply[0..4].try_into().unwrap()) as usize, reply.len());
    }

    /// The frame the shop actually writes, captured off its billing socket, is
    /// the 1/1 step - with its own fields, which the answer does not read.
    #[test]
    fn the_captured_shop_frame_is_the_first_step() {
        let request = hero5_shop_request();
        assert_eq!(lgt_local_hero5_response(&request), lgt_local_hero5_response(&hero5_frame(1, 1, &[])));
        assert_eq!(lgt_local_hero5_response(&request).unwrap().len(), 32);
    }

    #[test]
    fn a_frame_that_is_not_that_shop_step_is_left_unanswered() {
        // A length that is not the frame in hand.
        let mut mislength = hero5_shop_request();
        mislength[3] = 0x51;
        assert_eq!(lgt_local_hero5_response(&mislength), None);

        // Another title's frame that happens to be big-endian framed.
        let mut unnamed = hero5_shop_request();
        unnamed[4] = b'X';
        assert_eq!(lgt_local_hero5_response(&unnamed), None);

        // A sub-command of a command that is answered, but which is not - 4/2
        // through 4/5 are 창고 steps whose handlers have not been read.
        assert_eq!(lgt_local_hero5_response(&hero5_frame(4, 2, &[])), None);
        assert_eq!(lgt_local_hero5_response(&hero5_frame(3, 3, &[])), None);

        // A sub-command of a command that is answered, but which is not.
        assert_eq!(lgt_local_hero5_response(&hero5_frame(1, 5, &[])), None);
        assert_eq!(lgt_local_hero5_response(&hero5_frame(6, 1, &[])), None);
        assert_eq!(lgt_local_hero5_response(&hero5_frame(0, 1, &[])), None);
        assert_eq!(lgt_local_hero5_response(&hero5_frame(7, 2, &[])), None);

        // Too short to carry a command at all.
        assert_eq!(lgt_local_hero5_response(&hero5_shop_request()[..19]), None);
    }

    #[test]
    fn the_opening_id_frame_is_answered_with_its_own_id_and_nothing_else() {
        assert_eq!(
            lgt_local_id_framed_response(&ZZT4_OPENING),
            Some(vec![0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x01])
        );
        assert_eq!(
            response(&ZZT4_OPENING).as_deref(),
            Some(&[0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x01][..])
        );
    }

    #[test]
    fn a_frame_that_is_not_that_opening_is_left_unanswered() {
        // Another id in the same protocol: its handler reads fields out of the
        // frame, so an id alone is not an answer to it.
        let mut other = ZZT4_OPENING;
        other[4..8].copy_from_slice(&0x01F0_0002u32.to_le_bytes());
        assert_eq!(lgt_local_id_framed_response(&other), None);

        // The length has to be the record in hand.
        let mut mislabelled = ZZT4_OPENING;
        mislabelled[0] = 0x28;
        assert_eq!(lgt_local_id_framed_response(&mislabelled), None);

        // The revision every request in this protocol carries.
        let mut revised = ZZT4_OPENING;
        revised[8] = 0x34;
        assert_eq!(lgt_local_id_framed_response(&revised), None);

        assert_eq!(lgt_local_id_framed_response(&ZZT4_OPENING[..8]), None);
    }

    /// The request's own fields carry its arguments, and the emulator's run of
    /// the title sends the same request with one of them set.
    #[test]
    fn the_opening_is_answered_whatever_its_arguments_say() {
        let mut with_argument = ZZT4_OPENING;
        with_argument[12] = 1;
        assert_eq!(
            lgt_local_id_framed_response(&with_argument),
            Some(vec![0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x01])
        );
        assert_eq!(lgt_local_id_framed_response(&ZZT4_OPENING[..8]), None);
    }

    #[test]
    fn a_frame_s_own_length_says_which_end_of_its_fields_comes_first() {
        use super::{BillFrame, BillFrameOrder};

        // The 19-byte purchase request 붉은보석 actually writes: length and type
        // little end first, then the subscriber number as ASCII.
        let captured = [
            0xff, 0xff, 0x13, 0x00, 0x68, 0x00, b'0', b'1', b'0', b'5', b'5', b'4', b'5', b'2', b'3', b'8', b'3', 0x00, 0x01,
        ];
        let frame = BillFrame::parse(&captured).unwrap();
        assert_eq!(frame.order, BillFrameOrder::Little);
        assert_eq!(frame.message_type, 0x68);

        // The same frame written the other way round reads as itself too.
        let big_endian = [
            0xff, 0xff, 0x00, 0x13, 0x00, 0x68, b'0', b'1', b'0', b'5', b'5', b'4', b'5', b'2', b'3', b'8', b'3', 0x00, 0x01,
        ];
        let frame = BillFrame::parse(&big_endian).unwrap();
        assert_eq!(frame.order, BillFrameOrder::Big);
        assert_eq!(frame.message_type, 0x68);

        // And so does a header-complete prefix of either, where the shorter of
        // the two readings is the one that could still be this frame.
        assert_eq!(BillFrame::parse(&captured[..10]).unwrap().order, BillFrameOrder::Little);
        assert_eq!(BillFrame::parse(&big_endian[..10]).unwrap().order, BillFrameOrder::Big);
    }

    #[test]
    fn a_granted_reply_echoes_the_request_type_and_says_nothing_is_owed() {
        use super::lgt_local_granted_response;

        // The purchase transaction, which already had an answer of its own:
        // request 0x68 is answered by response 0x69 with a zero status.
        let reply = lgt_local_granted_response(&[0xff, 0xff, 0x00, 0x0a, 0x00, 0x68, 1, 2, 3, 4]).unwrap();
        assert_eq!(reply, vec![0xff, 0xff, 0x00, 0x07, 0x00, 0x69, 0x00]);

        // Every other request is answered the same way, which is what mode 1
        // had no peer to do before.
        let reply = lgt_local_granted_response(&[0xff, 0xff, 0x00, 0x06, 0x00, 0x20]).unwrap();
        assert_eq!(reply, vec![0xff, 0xff, 0x00, 0x07, 0x00, 0x21, 0x00]);
    }

    #[test]
    fn a_granted_reply_is_shaped_only_for_a_frame_this_reads() {
        use super::lgt_local_granted_response;

        // No marker, and too short to carry a header at all.
        assert!(lgt_local_granted_response(&[0x00, 0x00, 0x00, 0x06, 0x00, 0x68]).is_none());
        assert!(lgt_local_granted_response(&[0xff, 0xff, 0x00]).is_none());

        // A length that cannot hold a header whichever end is read first.
        assert!(lgt_local_granted_response(&[0xff, 0xff, 0x00, 0x00, 0x00, 0x68]).is_none());

        // A slice longer than either reading of the length it declares: 0x0304
        // one way, 0x0403 the other, both short of the bytes in hand.
        let overlong = vec![0xffu8, 0xff, 0x03, 0x04, 0x00, 0x68]
            .into_iter()
            .chain(core::iter::repeat_n(0u8, 2000))
            .collect::<Vec<u8>>();
        assert!(lgt_local_granted_response(&overlong).is_none());
    }

    /// 제노니아1's own 72-byte purchase record, captured off the title.
    fn zenonia_purchase_request() -> Vec<u8> {
        let mut request = vec![0u8; 72];
        request[0..2].copy_from_slice(&72u16.to_le_bytes());
        request[2..4].copy_from_slice(&0x0700u16.to_le_bytes());
        request[4..15].copy_from_slice(b"01055145031");
        // 생명의 근원(10개), EUC-KR, in its 40-byte field.
        request[16..33].copy_from_slice(&[
            0xbb, 0xfd, 0xb8, 0xed, 0xc0, 0xc7, 0x20, 0xb1, 0xd9, 0xbf, 0xf8, 0x28, 0x31, 0x30, 0xb0, 0xb3, 0x29,
        ]);
        request[56..60].copy_from_slice(&700u32.to_le_bytes());
        request[60..71].copy_from_slice(b"00027BAA002");

        request
    }

    /// 제노니아2's 93-byte record: the same, with a flag and the handset model
    /// behind the item code, and its own command.
    fn zenonia2_purchase_request() -> Vec<u8> {
        let mut request = vec![0u8; 93];
        request[0..2].copy_from_slice(&93u16.to_le_bytes());
        request[2..4].copy_from_slice(&0x0400u16.to_le_bytes());
        request[4..15].copy_from_slice(b"01055452383");
        request[56..60].copy_from_slice(&100u32.to_le_bytes());
        request[60..71].copy_from_slice(b"0002C004001");
        request[72] = 1;
        request[73..81].copy_from_slice(b"Emulator");

        request
    }

    #[test]
    fn a_gamevil_purchase_is_answered_the_way_onrecvdone_reads_it() {
        use super::lgt_local_gamevil_packet_response;

        // Each title is answered with its own command plus one: 제노니아1 buys
        // with 0x0700, 2 and 3 with 0x0400.
        for (request, expected) in [(zenonia_purchase_request(), 0x0701u16), (zenonia2_purchase_request(), 0x0401)] {
            let response = lgt_local_gamevil_packet_response(&request).unwrap();

            // `tagNetHeader`: a length at [0] and a command at [2], both u16
            // little end first, four bytes of it.
            assert_eq!(u16::from_le_bytes([response[0], response[1]]) as usize, response.len());
            assert_eq!(u16::from_le_bytes([response[2], response[3]]), expected);

            // The status `OnRecvDone` reads straight after the header. Below -1
            // goes to OnError instead of the command switch.
            assert!(response[4] as i8 >= 0);

            // The buy handler reads nothing behind it.
            assert!(response[5..].iter().all(|&byte| byte == 0));

            // And the same answer every time - the sweeps are over.
            assert_eq!(lgt_local_gamevil_packet_response(&request), Some(response));
        }
    }

    #[test]
    fn only_a_record_that_declares_itself_is_answered_as_a_purchase() {
        use super::lgt_local_gamevil_packet_response;

        // A length that is not the record in hand.
        let mut wrong_length = zenonia_purchase_request();
        wrong_length[0] = 0x47;
        assert_eq!(lgt_local_gamevil_packet_response(&wrong_length), None);

        // An odd command is one a title is answered with, not one it sends, so
        // answering it would be answering an answer.
        let mut answer_shaped = zenonia_purchase_request();
        answer_shaped[2..4].copy_from_slice(&0x0701u16.to_le_bytes());
        assert_eq!(lgt_local_gamevil_packet_response(&answer_shaped), None);

        // Too short to be carrying a purchase, however well it declares itself.
        let mut stub = vec![0u8; 8];
        stub[0..2].copy_from_slice(&8u16.to_le_bytes());
        stub[2..4].copy_from_slice(&0x0400u16.to_le_bytes());
        assert_eq!(lgt_local_gamevil_packet_response(&stub), None);

        // And the other protocols answered here are not mistaken for it.
        assert_eq!(
            lgt_local_gamevil_packet_response(b"CASH|0|demon|05590091|00029B60004|500|2034517541"),
            None
        );
        assert_eq!(lgt_local_gamevil_packet_response(&[0xff, 0xff, 0x00, 0x06, 0x00, 0x68]), None);
        assert_eq!(lgt_local_gamevil_packet_response(b""), None);
    }

    #[test]
    fn a_cash_request_is_answered_with_the_word_its_sender_reads_as_paid() {
        // The record 데몬헌터 actually writes, captured off the title, and the
        // length its two-step receive reads before the body it counts.
        let request = b"CASH|0|demon|05590091|00029B60004|500|2034517541";
        assert_eq!(lgt_local_cash_response(request).as_deref(), Some(b"\x00\x04SASH".as_slice()));

        // Every item in the title's own price table is the same request.
        assert_eq!(
            lgt_local_cash_response(b"CASH|0|demon|05590091|0002B640007|2900|1").as_deref(),
            Some(b"\x00\x04SASH".as_slice())
        );

        // Nothing else is one of these records.
        assert_eq!(lgt_local_cash_response(b"SASH"), None);
        assert_eq!(lgt_local_cash_response(b"CASH"), None);
        assert_eq!(lgt_local_cash_response(b""), None);
        assert_eq!(lgt_local_cash_response(&[0xff, 0xff, 0x00, 0x06, 0x00, 0x68]), None);
    }

    /// The 55-byte record 레전드오브마스터 writes to buy a 최상급강화석 for
    /// 500원, captured off the title.
    fn legend_of_master_purchase_request() -> Vec<u8> {
        let mut request = vec![0u8; 55];
        request[0..2].copy_from_slice(&55u16.to_be_bytes());
        request[2..4].copy_from_slice(&0x0836u16.to_be_bytes());
        request[18] = 0x64;
        request[29] = 0x12;
        request[30..32].copy_from_slice(&500u16.to_be_bytes());
        // 최상급강화석, EUC-KR.
        request[32..44].copy_from_slice(&[0xc3, 0xd6, 0xbb, 0xf3, 0xb1, 0xde, 0xb0, 0xad, 0xc8, 0xad, 0xbc, 0xae]);
        request[53..55].copy_from_slice(&0xc8d1u16.to_be_bytes());

        request
    }

    #[test]
    fn a_big_endian_record_is_answered_the_way_its_read_state_reads_it() {
        use super::lgt_local_big_endian_record_response;

        let request = legend_of_master_purchase_request();
        let response = lgt_local_big_endian_record_response(&request).unwrap();

        // The four byte header the read state takes first, big end first both
        // fields - and a length that counts the header and the tail as six.
        let length = i16::from_be_bytes([response[0], response[1]]);
        assert!(length > 0);
        assert_eq!(length as usize, response.len() - 2);

        // The command has to be above 1000 or the thread drops the connection,
        // and it is the request's own plus one so the dispatcher reaches the
        // purchase handler.
        let command = i16::from_be_bytes([response[2], response[3]]);
        assert!(command > 1000);
        assert_eq!(command, 0x0837);

        // Which reads one signed byte off the body: zero is granted.
        assert_eq!(response[4] as i8, 0);

        // A body long enough to read past, and a four byte tail behind it.
        assert_eq!(response.len(), 4 + (length as usize - 6) + 4);

        // And the same answer every time.
        assert_eq!(lgt_local_big_endian_record_response(&request), Some(response));
    }

    /// 레전드오브마스터2's shop opens on a record of the same shape under a
    /// command of exactly a thousand, which the bound used to turn away.
    #[test]
    fn a_record_at_the_bound_is_answered_above_it() {
        use super::lgt_local_big_endian_record_response;

        let mut request = [0u8; 45];
        request[0..2].copy_from_slice(&45u16.to_be_bytes());
        request[2..4].copy_from_slice(&1000u16.to_be_bytes());
        request[26..30].copy_from_slice(&100u32.to_le_bytes());
        request[30..34].copy_from_slice(&1u32.to_le_bytes());

        let response = lgt_local_big_endian_record_response(&request).expect("the shop record is answered");

        // 1002, which the dispatcher at 0x25758 reaches three comparisons in
        // and which branches to the block that writes the item shop's screen.
        // The plus-one, 1001, is not in that chain at all - it is what drew
        // `엉뚱한 패킷날라옴 / 인덱스:1001`.
        assert_eq!(u16::from_be_bytes([response[2], response[3]]), 1002);
        assert_eq!(response[4] as i8, 0);

        // Answered that way the title asks the next thing, a frame with no body
        // at all, and that is answered the same way: 1202 is the arm that takes
        // a count off the reply and allocates the list behind it.
        let mut bodyless = [0u8; 4];
        bodyless[0..2].copy_from_slice(&4u16.to_be_bytes());
        bodyless[2..4].copy_from_slice(&1200u16.to_be_bytes());

        let next = lgt_local_big_endian_record_response(&bodyless).expect("a frame with no body is still a frame");
        assert_eq!(u16::from_be_bytes([next[2], next[3]]), 1202);

        // 영웅서기5 shares the gateway and not the shape: it asks under 0x0836
        // and reads its reply at 0x0837, so the plus-two must not reach it.
        let hero5 = legend_of_master_purchase_request();
        let hero5_response = lgt_local_big_endian_record_response(&hero5).expect("영웅서기5's record is answered");
        assert_eq!(u16::from_be_bytes([hero5_response[2], hero5_response[3]]), 0x0837);

        // A request below the bound is still turned away: its answer would be
        // dropped, and answering it would only cost the title its socket.
        let mut below = request;
        below[2..4].copy_from_slice(&998u16.to_be_bytes());
        assert_eq!(lgt_local_big_endian_record_response(&below), None);
    }

    #[test]
    fn only_a_big_endian_record_that_declares_itself_is_answered_as_one() {
        use super::lgt_local_big_endian_record_response;

        // A length that is not the record in hand.
        let mut wrong_length = legend_of_master_purchase_request();
        wrong_length[1] = 0x38;
        assert_eq!(lgt_local_big_endian_record_response(&wrong_length), None);

        // A command the title is answered with rather than one it sends.
        let mut answer_shaped = legend_of_master_purchase_request();
        answer_shaped[2..4].copy_from_slice(&0x0837u16.to_be_bytes());
        assert_eq!(lgt_local_big_endian_record_response(&answer_shaped), None);

        // A command the read state would drop the connection over rather than
        // dispatch, so answering it would only cost the title its socket.
        let mut too_low = legend_of_master_purchase_request();
        too_low[2..4].copy_from_slice(&0x0064u16.to_be_bytes());
        assert_eq!(lgt_local_big_endian_record_response(&too_low), None);

        // And the other protocols answered here are not mistaken for it - each
        // declares its own length, and the GAMEVIL packet's reads as thousands
        // the other end first.
        assert_eq!(lgt_local_big_endian_record_response(&zenonia_purchase_request()), None);
        assert_eq!(
            lgt_local_big_endian_record_response(b"CASH|0|demon|05590091|00029B60004|500|2034517541"),
            None
        );
        assert_eq!(lgt_local_big_endian_record_response(&[0xff, 0xff, 0x00, 0x06, 0x00, 0x68]), None);
        assert_eq!(lgt_local_big_endian_record_response(b""), None);

        // Nor is it mistaken for one of them.
        assert_eq!(lgt_local_gamevil_packet_response(&legend_of_master_purchase_request()), None);
        assert_eq!(lgt_local_granted_response(&legend_of_master_purchase_request()), None);
    }

    /// The 7-byte frame 영웅서기4 writes to open its online 상점, captured off
    /// the title, and the 6-byte keep-alive it writes five seconds later.
    fn hero_lore_frame(major: u8, minor: u8, body: &[u8]) -> Vec<u8> {
        let length = 6 + body.len();
        let mut frame = Vec::with_capacity(length);
        frame.extend_from_slice(&(length as u32).to_le_bytes());
        frame.push(major);
        frame.push(minor);
        frame.extend_from_slice(body);

        frame
    }

    /// The sixteen prices [`hero4_catalogue`] lays out, read back out of the rows
    /// it writes rather than restated here.
    fn hero4_catalogue_prices() -> Vec<u32> {
        const ROW: usize = 37;

        let body = super::hero4_catalogue();
        let rows = u16::from_le_bytes([body[2], body[3]]) as usize;

        (0..rows)
            .map(|index| {
                let row = &body[4 + index * ROW..4 + (index + 1) * ROW];
                u32::from_le_bytes([row[21], row[22], row[23], row[24]])
            })
            .collect()
    }

    #[test]
    fn a_length_prefixed_command_is_answered_the_way_its_dispatcher_reads_it() {
        use super::lgt_local_major_minor_response;

        // The record the title actually writes on opening 상점.
        assert_eq!(hero_lore_frame(1, 1, &[4]), [0x07, 0x00, 0x00, 0x00, 0x01, 0x01, 0x04]);

        // The login the title walks: each step is answered with its own command
        // back, and each handler reads nothing else out of the reply.
        for (major, minor) in [(1u8, 0x01u8), (1, 0x3d), (1, 0x3e)] {
            let request = hero_lore_frame(major, minor, &[4]);
            let response = lgt_local_major_minor_response(&request).unwrap();

            // Six bytes: under that the dispatcher drops the frame unread.
            assert_eq!(response.len(), 6);
            assert_eq!(
                u32::from_le_bytes([response[0], response[1], response[2], response[3]]) as usize,
                response.len()
            );
            assert_eq!((response[4], response[5]), (major, minor));
        }

        // Both halves of a purchase, which carry the row's handle and its price.
        let purchase = hero_lore_frame(5, 0x42, &[0, 0, 0, 0, 0, 0, 0, 0, 0xf4, 0x01, 0x00, 0x00]);
        assert_eq!(purchase.len(), 18);
        let response = lgt_local_major_minor_response(&purchase).unwrap();
        assert_eq!((response[4], response[5]), (5, 0x42));
        // Granted, and an empty message where the error box would read one.
        assert_eq!(response[6], 1);
        assert_eq!(response[8], 0);

        // The delivery, which carries the row's own `+0x254` and the price this
        // side priced it at.
        let delivered = |price: u32| {
            let mut body = [0u8; 12];
            body[8..].copy_from_slice(&price.to_le_bytes());
            let response = lgt_local_major_minor_response(&hero_lore_frame(5, 0x40, &body)).unwrap();
            assert_eq!((response[4], response[5]), (5, 0x40));
            assert_eq!(response[6], 1);
            // The item byte sits `[7]` past the message.
            let offset = response[7] as usize;
            assert_eq!(response[8], 0);
            response[8 + offset]
        };

        // The four box rows draw inside their own stretch of the table, and
        // nothing outside it.
        for (paid, from, to) in super::HERO4_BOX_DRAWS {
            for _ in 0..64 {
                let drawn = delivered(paid);
                assert!((from..to).contains(&drawn), "{paid} drew {drawn}, outside {from}..{to}");
            }
        }

        // Every other row hands over what the shop said it was selling. Answering
        // these with a draw as well turned every purchase into a box.
        for (index, price) in hero4_catalogue_prices().into_iter().enumerate() {
            if HERO4_BOX_DRAWS.iter().any(|(paid, _, _)| *paid == price) {
                continue;
            }
            assert_eq!(delivered(price), 0xff, "row {index} at {price}");
        }

        // A frame that is not the length this message is.
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(5, 0x40, &[0; 8])), None);

        // And the catalogue.
        let response = lgt_local_major_minor_response(&hero_lore_frame(5, 0x3f, &[0])).unwrap();
        assert_eq!(
            u32::from_le_bytes([response[0], response[1], response[2], response[3]]) as usize,
            response.len()
        );
        assert_eq!((response[4], response[5]), (5, 0x3f));
    }

    #[test]
    fn the_warehouse_keeps_what_it_is_handed_and_lists_it_back() {
        use super::{hero4_warehouse_needs_loading, hero4_warehouse_to_keep, lgt_local_major_minor_response, load_hero4_warehouse};

        const ROW: usize = 37;

        // The upload, which `0x5e4f0` reads nothing of - the command alone.
        let mut character = vec![0u8; 1132];
        character[1] = 0x67;
        let response = lgt_local_major_minor_response(&hero_lore_frame(0x14, 0x46, &character)).unwrap();
        assert_eq!(response, [0x06, 0x00, 0x00, 0x00, 0x14, 0x46]);
        // And which the 창고 is not behind, so it does not read one in.
        assert!(!hero4_warehouse_needs_loading(&hero_lore_frame(0x14, 0x46, &character)));

        // The listing is, and nothing has been kept for it.
        let asks = hero_lore_frame(5, 0x3d, &[0, 0]);
        assert!(hero4_warehouse_needs_loading(&asks));
        load_hero4_warehouse(&[]);
        // Read in once, whether or not anything was there.
        assert!(!hero4_warehouse_needs_loading(&asks));
        assert_eq!(hero4_warehouse_to_keep(), None);

        // Which is the catalogue's own shape one byte further along: a granted
        // status, the two bytes `0x1566b1a` takes, and a count of no rows.
        let listing = lgt_local_major_minor_response(&asks).unwrap();
        assert_eq!(
            u32::from_le_bytes([listing[0], listing[1], listing[2], listing[3]]) as usize,
            listing.len()
        );
        assert_eq!((listing[4], listing[5]), (5, 0x3d));
        assert_eq!(listing[6], 1);
        assert_eq!(&listing[7..9], &[0, 1]);
        assert_eq!(u16::from_le_bytes([listing[9], listing[10]]), 0);
        assert_eq!(listing.len(), 11);

        // The deposit the 예 on 대전 창고에 옮기겠습니까 writes, as it came off
        // the wire: the kind at `[16]`, the grade, and 300 for its price.
        let deposit: [u8; 36] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x85, 0xb8, 0xed, 0x7f, 0xa0, 0x01, 0x00, 0x00, 0x0b, 0x00, 0x01, 0x00, 0x2c, 0x01, 0x00,
            0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let identity = &deposit[..16];
        let request = hero_lore_frame(5, 0x41, &deposit);
        assert_eq!(request.len(), 42);
        // Granted, which is what runs the move at `0x5e658`.
        let response = lgt_local_major_minor_response(&request).unwrap();
        assert_eq!(response, [0x07, 0x00, 0x00, 0x00, 0x05, 0x41, 0x01]);

        // And it is what has to be kept, once - the item is out of the bag now.
        let kept = hero4_warehouse_to_keep().unwrap();
        assert_eq!(kept, deposit);
        assert_eq!(hero4_warehouse_to_keep(), None);

        // It is in the listing behind a row byte of its own.
        let listing = lgt_local_major_minor_response(&asks).unwrap();
        assert_eq!(u16::from_le_bytes([listing[9], listing[10]]), 1);
        let row = &listing[11..11 + ROW];
        assert_eq!(row[0], 0);
        assert_eq!(&row[1..], &deposit);
        // Which is where its reader takes the kind and the price.
        assert_eq!(row[17], 0x0b);
        assert_eq!(u32::from_le_bytes([row[21], row[22], row[23], row[24]]), 300);

        // And what was kept is the same 창고 read back after a restart.
        load_hero4_warehouse(&kept);
        let listing = lgt_local_major_minor_response(&asks).unwrap();
        assert_eq!(u16::from_le_bytes([listing[9], listing[10]]), 1);
        assert_eq!(&listing[11..11 + ROW][1..], &deposit);
        // Reading it in is not a change to write back out.
        assert_eq!(hero4_warehouse_to_keep(), None);

        // 싱글 창고에 옮기겠습니까 takes it back out, naming the row by the
        // first sixteen bytes of its record. `0x5ec20` reads none of the
        // answer, so the answer is the command back.
        let takes = hero_lore_frame(5, 0x04, identity);
        assert_eq!(takes.len(), 22);
        let response = lgt_local_major_minor_response(&takes).unwrap();
        assert_eq!(response, [0x06, 0x00, 0x00, 0x00, 0x05, 0x04]);

        // The item is in the bag now, so the 창고 must not still be holding it -
        // the listing `0x5de8c` asks for next would be a second one.
        let listing = lgt_local_major_minor_response(&asks).unwrap();
        assert_eq!(u16::from_le_bytes([listing[9], listing[10]]), 0);
        assert_eq!(hero4_warehouse_to_keep().unwrap(), Vec::<u8>::new());

        // A withdrawal naming nothing held takes nothing and writes nothing.
        let response = lgt_local_major_minor_response(&takes).unwrap();
        assert_eq!(response, [0x06, 0x00, 0x00, 0x00, 0x05, 0x04]);
        assert_eq!(hero4_warehouse_to_keep(), None);
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(5, 0x04, &[0; 8])), None);

        // A trailing part-record is dropped rather than guessed at.
        load_hero4_warehouse(&kept[..HERO4_ITEM_RECORD - 1]);
        let listing = lgt_local_major_minor_response(&asks).unwrap();
        assert_eq!(u16::from_le_bytes([listing[9], listing[10]]), 0);

        // A frame that is not the record this message carries.
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(5, 0x41, &[0; 12])), None);

        // And the four bytes behind the listing, which `0x5eb04` stores and no
        // instruction in the archive reads back.
        let response = lgt_local_major_minor_response(&hero_lore_frame(5, 0x14, &[])).unwrap();
        assert_eq!(response, [0x0a, 0x00, 0x00, 0x00, 0x05, 0x14, 0, 0, 0, 0]);
    }

    #[test]
    fn the_catalogue_lays_out_the_sixteen_rows_its_reader_walks() {
        use super::lgt_local_major_minor_response;

        const ROW: usize = 37;

        let response = lgt_local_major_minor_response(&hero_lore_frame(5, 0x3f, &[0])).unwrap();
        let body = &response[6..];

        // One page, and the count the reader takes before the rows.
        assert_eq!((body[0], body[1]), (0, 1));
        assert_eq!(u16::from_le_bytes([body[2], body[3]]), 16);

        // Every row is walked at its full stride, so the frame has to carry all
        // sixteen of them - the reader reads to the last one's 35th byte.
        assert_eq!(body.len(), 4 + 16 * ROW);
        assert_eq!(response.len(), 6 + 4 + 16 * ROW);

        // The ids, grades and prices the 보물함 build builds its own sixteen
        // from: the grade in a packed byte's low nibble, the price its high
        // nibble by fifty for the first twelve rows and by five hundred for the
        // last four - and the first row's grade spelled 0x14 instead.
        let expected: [(u8, u8, u32); 16] = [
            (0x0f, 0x14, 250),
            (0x05, 0x0a, 250),
            (0x10, 0x05, 200),
            (0x14, 0x01, 100),
            (0x13, 0x01, 500),
            (0x18, 0x0a, 500),
            (0x15, 0x05, 500),
            (0x16, 0x01, 500),
            (0x11, 0x01, 300),
            (0x12, 0x01, 300),
            (0x1d, 0x01, 500),
            (0x17, 0x01, 500),
            (0x19, 0x01, 1500),
            (0x1a, 0x01, 2000),
            (0x1b, 0x01, 2500),
            (0x1c, 0x01, 3000),
        ];

        for (index, (item, grade, price)) in expected.into_iter().enumerate() {
            let row = &body[4 + index * ROW..4 + (index + 1) * ROW];

            // The kind the local item table is keyed by, then the id in it.
            assert_eq!(row[17], 8, "row {index} kind");
            assert_eq!(row[18], item, "row {index} item");
            assert_eq!(row[19], grade, "row {index} grade");
            assert_eq!(u32::from_le_bytes([row[21], row[22], row[23], row[24]]), price, "row {index} price");

            // Everything the row does not name is left for the title's own
            // factory to have set, and the server's two handles are nothing
            // this side has to invent.
            assert!(row[..17].iter().all(|&byte| byte == 0), "row {index} handles");
            assert_eq!(row[20], 0, "row {index}");
            assert!(row[25..].iter().all(|&byte| byte == 0), "row {index} tail");
        }
    }

    /// Every draw names a record the table actually has, and consecutive draws
    /// are not the table in order.
    #[test]
    fn a_box_draw_stays_inside_the_table_and_moves_around_it() {
        use super::next_box_draw;

        let drawn: Vec<u8> = (0..200).map(|_| next_box_draw(23)).collect();

        assert!(drawn.iter().all(|&index| index < 23));
        assert!(drawn.windows(2).any(|pair| pair[1] != (pair[0] + 1) % 23));
        // Over two hundred draws a twenty-three record table is well covered.
        let seen = drawn.iter().collect::<alloc::collections::BTreeSet<_>>();
        assert!(seen.len() > 18, "{} of 23", seen.len());
    }

    #[test]
    fn only_a_command_pair_whose_answer_is_known_is_answered() {
        use super::lgt_local_major_minor_response;

        // The keep-alive: the title's own dispatcher drops major 0, so an answer
        // to it would be an answer to nothing.
        assert_eq!(hero_lore_frame(0, 0x0a, &[]), [0x06, 0x00, 0x00, 0x00, 0x00, 0x0a]);
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(0, 0x0a, &[])), None);

        // A command pair this cannot shape a reply to. Answering it would put
        // the title through a branch meant for a different exchange.
        // The notice a deposit sends behind itself. The title's own dispatcher
        // takes `minor - 4`, so a `5/0x03` reply is dropped unread anyway.
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(5, 0x03, &[0; 36])), None);
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(5, 0x70, &[])), None);
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(5, 0x71, &[])), None);
        // The 창고's other minor: `0x5e3a0` takes `0x47` as well, and that one
        // deserialises a record this has none of.
        assert_eq!(lgt_local_major_minor_response(&hero_lore_frame(0x14, 0x47, &[1])), None);

        // A length that is not the frame in hand.
        let mut wrong_length = hero_lore_frame(1, 1, &[4]);
        wrong_length[0] = 0x08;
        assert_eq!(lgt_local_major_minor_response(&wrong_length), None);

        // Too short for the dispatcher to read a command out of.
        assert_eq!(lgt_local_major_minor_response(&[0x05, 0x00, 0x00, 0x00, 0x01]), None);
        assert_eq!(lgt_local_major_minor_response(b""), None);

        // And the other protocols answered here are not mistaken for it - the
        // GAMEVIL packet's length is the record in hand as a u16, which as a u32
        // carries the command in its high half.
        assert_eq!(lgt_local_major_minor_response(&zenonia_purchase_request()), None);
        assert_eq!(lgt_local_major_minor_response(&legend_of_master_purchase_request()), None);
        assert_eq!(lgt_local_major_minor_response(b"CASH|0|demon|05590091|00029B60004|500|2034517541"), None);
        assert_eq!(lgt_local_major_minor_response(&[0xff, 0xff, 0x00, 0x06, 0x00, 0x68]), None);

        // Nor is it mistaken for one of them.
        let hello = hero_lore_frame(1, 1, &[4]);
        assert_eq!(lgt_local_granted_response(&hello), None);
        assert_eq!(lgt_local_cash_response(&hello), None);
        assert_eq!(lgt_local_gamevil_packet_response(&hello), None);
        assert_eq!(lgt_local_big_endian_record_response(&hello), None);
    }

    /// The forty-byte record 아니마 writes to buy a 부활마법서 for 3000원,
    /// captured off the title.
    fn anima_purchase_request() -> Vec<u8> {
        let mut request = Vec::from(*b"AM40    191111222210SB_");
        // 부활마법서, EUC-KR.
        request.extend_from_slice(&[0xba, 0xce, 0xc8, 0xb0, 0xb8, 0xb6, 0xb9, 0xfd, 0xbc, 0xad]);
        request.extend_from_slice(b"_3000_M");

        request
    }

    #[test]
    fn a_text_record_is_answered_in_the_shape_its_framing_reads() {
        use super::lgt_local_text_record_response;

        let request = anima_purchase_request();
        assert_eq!(request.len(), 40);

        let response = lgt_local_text_record_response(&request).unwrap();

        // The tag the framing compares as a u16 before anything else, then the
        // record's own length as text - which is what it waits for.
        assert_eq!(&response[..2], b"AM");
        assert_eq!(super::atoi(&response[2..8]), Some(response.len()));

        // The body starts at [8], and its two characters are the ones the
        // command was sent under.
        assert_eq!(&response[8..], b"SB");
        assert_eq!(response, b"AM10    SB");
    }

    #[test]
    fn only_a_text_record_that_declares_itself_is_answered() {
        use super::lgt_local_text_record_response;

        // A length that is not the record in hand.
        let mut wrong_length = anima_purchase_request();
        wrong_length[2..4].copy_from_slice(b"41");
        assert_eq!(lgt_local_text_record_response(&wrong_length), None);

        // No tag, and no length behind one.
        assert_eq!(lgt_local_text_record_response(b"XX40    191111222210SB_x"), None);
        assert_eq!(lgt_local_text_record_response(b"AM      191111222210SB_x"), None);

        // Too short to carry a command behind its header.
        assert_eq!(lgt_local_text_record_response(b"AM20    191111222210"), None);
        assert_eq!(lgt_local_text_record_response(b""), None);

        // And the other protocols answered here are not mistaken for it.
        assert_eq!(lgt_local_text_record_response(&zenonia_purchase_request()), None);
        assert_eq!(lgt_local_text_record_response(&legend_of_master_purchase_request()), None);
        assert_eq!(lgt_local_text_record_response(&hero_lore_frame(1, 1, &[4])), None);
        assert_eq!(lgt_local_text_record_response(b"CASH|0|demon|05590091|00029B60004|500|2034517541"), None);

        // Nor is it mistaken for one of them - "AM" is 0x4d41 one end first and
        // 0x414d the other, and neither is this record's forty bytes.
        let request = anima_purchase_request();
        assert_eq!(lgt_local_granted_response(&request), None);
        assert_eq!(lgt_local_cash_response(&request), None);
        assert_eq!(lgt_local_gamevil_packet_response(&request), None);
        assert_eq!(lgt_local_big_endian_record_response(&request), None);
        assert_eq!(lgt_local_major_minor_response(&request), None);
    }

    #[test]
    fn a_length_field_is_read_the_way_atoi_reads_one() {
        use super::atoi;

        // Digits, stopping at the first byte that is not one - which is how the
        // title's own left-justified `%-6d` is read back.
        assert_eq!(atoi(b"40    "), Some(40));
        assert_eq!(atoi(b"    40"), Some(40));
        assert_eq!(atoi(b"1000"), Some(1000));

        // A field with no number in it is not a zero.
        assert_eq!(atoi(b"      "), None);
        assert_eq!(atoi(b"SB_xxx"), None);
        assert_eq!(atoi(b""), None);
    }

    /// The 36-byte record 와일드프론티어 writes to buy a 1000원 item, captured
    /// off the title.
    fn wild_frontier_purchase_request() -> Vec<u8> {
        let mut request = Vec::from(*b"KP");
        request.extend_from_slice(&36u16.to_le_bytes());
        request.extend_from_slice(&7u16.to_le_bytes());
        // A purchase, and the byte behind it.
        request.push(9);
        request.push(0);
        request.extend_from_slice(b"0002CB52004\0");
        request.extend_from_slice(b"01055452383\0");
        request.extend_from_slice(&1000u32.to_le_bytes());

        request
    }

    /// The 40-byte record 와일드프론티어2 writes to buy a 500원 item, captured
    /// off the title: the same header in shape 27, with the subscriber ahead of
    /// the item and a word between it and the price.
    fn wild_frontier_2_purchase_request() -> Vec<u8> {
        let mut request = Vec::from(*b"KP");
        request.extend_from_slice(&40u16.to_le_bytes());
        request.extend_from_slice(&27u16.to_le_bytes());
        request.push(3);
        request.push(0);
        request.extend_from_slice(b"01085300848\0");
        request.extend_from_slice(b"0003535F004\0");
        request.extend_from_slice(&0x0d00u32.to_le_bytes());
        request.extend_from_slice(&500u32.to_le_bytes());

        request
    }

    #[test]
    fn a_tagged_record_is_answered_the_way_its_reader_reads_it() {
        use super::lgt_local_tagged_record_response;

        let request = wild_frontier_purchase_request();
        assert_eq!(request.len(), 36);

        let response = lgt_local_tagged_record_response(&request).unwrap();

        // Four bytes are read before anything else, and [2] is the whole
        // record's length - which is what the reader then waits for.
        assert_eq!(&response[..2], b"KP");
        assert_eq!(u16::from_le_bytes([response[2], response[3]]) as usize, response.len());

        // The shape it was asked in.
        assert_eq!(u16::from_le_bytes([response[4], response[5]]), 7);

        // The record byte it asked under, which chooses the handler, and a
        // status that is not an error.
        assert_eq!(response[6], 9);
        assert_eq!(response[7], 0);

        // The body's first byte, which is all the purchase is granted on.
        assert_eq!(response[8], 1);
        assert_eq!(response.len(), 9);
    }

    #[test]
    fn the_second_title_s_shape_is_answered_with_the_body_its_own_reader_takes() {
        use super::lgt_local_tagged_record_response;

        let request = wild_frontier_2_purchase_request();
        assert_eq!(request.len(), 40);

        let response = lgt_local_tagged_record_response(&request).unwrap();

        // The same header, in the shape it was asked in and under the record
        // byte that chooses the handler.
        assert_eq!(&response[..2], b"KP");
        assert_eq!(u16::from_le_bytes([response[2], response[3]]) as usize, response.len());
        assert_eq!(u16::from_le_bytes([response[4], response[5]]), 27);
        assert_eq!(response[6], 3);
        assert_eq!(response[7], 0);

        // Its purchase waits for twelve bytes and takes a u32 at [8], granting
        // on the low byte - so the body is a word, not the byte the first
        // title's reader takes.
        assert_eq!(response.len(), 12);
        assert_eq!(u32::from_le_bytes([response[8], response[9], response[10], response[11]]), 1);
    }

    #[test]
    fn only_a_tagged_record_that_declares_itself_is_answered() {
        use super::lgt_local_tagged_record_response;

        // A length that is not the record in hand.
        let mut wrong_length = wild_frontier_purchase_request();
        wrong_length[2] = 37;
        assert_eq!(lgt_local_tagged_record_response(&wrong_length), None);

        // A shape whose reader is not known, so there is no body to size.
        let mut wrong_shape = wild_frontier_purchase_request();
        wrong_shape[4] = 8;
        assert_eq!(lgt_local_tagged_record_response(&wrong_shape), None);

        // No tag, and too short to carry a header.
        assert_eq!(lgt_local_tagged_record_response(b"XP\x24\x00\x07\x00\x09\x00"), None);
        assert_eq!(lgt_local_tagged_record_response(b"KP\x04\x00"), None);
        assert_eq!(lgt_local_tagged_record_response(b""), None);

        // And the other protocols answered here are not mistaken for it.
        assert_eq!(lgt_local_tagged_record_response(&zenonia_purchase_request()), None);
        assert_eq!(lgt_local_tagged_record_response(&legend_of_master_purchase_request()), None);
        assert_eq!(lgt_local_tagged_record_response(&hero_lore_frame(1, 1, &[4])), None);
        assert_eq!(lgt_local_tagged_record_response(&anima_purchase_request()), None);
        assert_eq!(
            lgt_local_tagged_record_response(b"CASH|0|demon|05590091|00029B60004|500|2034517541"),
            None
        );

        // Nor is it mistaken for one of them.
        let request = wild_frontier_purchase_request();
        assert_eq!(lgt_local_granted_response(&request), None);
        assert_eq!(lgt_local_cash_response(&request), None);
        assert_eq!(lgt_local_gamevil_packet_response(&request), None);
        assert_eq!(lgt_local_big_endian_record_response(&request), None);
        assert_eq!(lgt_local_major_minor_response(&request), None);
        assert_eq!(lgt_local_text_record_response(&request), None);
    }

    /// The 16 bytes 이노티아연대기 writes when the shop is entered.
    fn inotia_shop_request() -> Vec<u8> {
        let mut request = vec![0u8; 2];
        request.push(0x1e);
        request.push(11);
        request.extend_from_slice(b"01055930906");
        request.push(0);
        let length = request.len() as u16;
        request[0..2].copy_from_slice(&length.to_be_bytes());

        request
    }

    /// What the shop writes to buy a row: the subscriber number, the row's own
    /// name as the title's table spells it, and the quantity.
    fn inotia_purchase_request(name: &[u8], quantity: u8) -> Vec<u8> {
        let mut request = vec![0u8; 2];
        request.push(0x1f);
        request.push(11);
        request.extend_from_slice(b"01055930906");
        request.push(name.len() as u8);
        request.extend_from_slice(name);
        request.push(quantity);
        let length = request.len() as u16;
        request[0..2].copy_from_slice(&length.to_be_bytes());

        request
    }

    #[test]
    fn a_catalogue_request_is_answered_with_the_page_it_asked_for() {
        use super::{INOTIA_SHOP_ROWS, lgt_local_subscriber_record_response};

        // Byte for byte what the capture shows going out.
        assert_eq!(
            inotia_shop_request(),
            vec![
                0x00, 0x10, 0x1e, 0x0b, 0x30, 0x31, 0x30, 0x35, 0x35, 0x39, 0x33, 0x30, 0x39, 0x30, 0x36, 0x00
            ]
        );

        let response = lgt_local_subscriber_record_response(&inotia_shop_request()).unwrap();

        // The length counts its own two bytes, the command comes back as it was
        // asked under, and the status is the one every handler reads as success.
        assert_eq!(u16::from_be_bytes([response[0], response[1]]) as usize, response.len());
        assert_eq!(&response[2..4], &[0x1e, 0x01]);
        // One page, the page that was asked for, and every row on it.
        assert_eq!(&response[4..7], &[1, 0, INOTIA_SHOP_ROWS.len() as u8]);

        // Which walks as rows of a name, a quantity and a big-endian price, and
        // accounts for the record exactly.
        let mut rest = &response[7..];
        for (name, count, price) in INOTIA_SHOP_ROWS {
            assert_eq!(rest[0] as usize, name.len());
            assert_eq!(&rest[1..1 + name.len()], name);
            assert_eq!(rest[1 + name.len()], count);
            assert_eq!(&rest[2 + name.len()..6 + name.len()], &price.to_be_bytes());
            rest = &rest[6 + name.len()..];
        }
        assert!(rest.is_empty());

        // 축복받은 부활주문서, EUC-KR, as `inotia.bar` spells it.
        assert_eq!(
            INOTIA_SHOP_ROWS[0].0,
            &[
                0xc3, 0xe0, 0xba, 0xb9, 0xb9, 0xde, 0xc0, 0xba, 0x20, 0xba, 0xce, 0xc8, 0xb0, 0xc1, 0xd6, 0xb9, 0xae, 0xbc, 0xad
            ]
        );
    }

    #[test]
    fn a_purchase_is_answered_with_a_status_and_nothing_else() {
        use super::{INOTIA_SHOP_ROWS, lgt_local_subscriber_record_response};

        // The handler reads no further than the status, so neither does this.
        let request = inotia_purchase_request(INOTIA_SHOP_ROWS[0].0, INOTIA_SHOP_ROWS[0].1);
        assert_eq!(lgt_local_subscriber_record_response(&request).unwrap(), vec![0x00, 0x04, 0x1f, 0x01]);
    }

    #[test]
    fn a_record_that_is_not_the_shop_s_is_left_to_whatever_sent_it() {
        use super::{lgt_local_big_endian_record_response, lgt_local_subscriber_record_response};

        // A length that does not describe the record in hand.
        let mut short = inotia_shop_request();
        short.pop();
        assert_eq!(lgt_local_subscriber_record_response(&short), None);

        // A subscriber number that is not digits.
        let mut lettered = inotia_shop_request();
        lettered[4] = b'x';
        assert_eq!(lgt_local_subscriber_record_response(&lettered), None);

        // A prefix that does not account for the rest of the record.
        let mut mismeasured = inotia_shop_request();
        mismeasured[3] = 10;
        assert_eq!(lgt_local_subscriber_record_response(&mismeasured), None);

        // A page past the one the catalogue declares, which the screen's own
        // arrows will not ask for.
        let mut second_page = inotia_shop_request();
        *second_page.last_mut().unwrap() = 1;
        assert_eq!(lgt_local_subscriber_record_response(&second_page), None);

        // A purchase whose name field does not account for the rest.
        let mut ragged = inotia_purchase_request(b"\xc7\xe0\xbf\xee\xc0\xc7 \xbf\xad\xbc\xe8", 1);
        ragged[15] = 3;
        assert_eq!(lgt_local_subscriber_record_response(&ragged), None);

        // And 레전드오브마스터's record, which is big-endian length-first too,
        // still reaches the handler that reads it rather than this one.
        let legend = legend_of_master_purchase_request();
        assert_eq!(lgt_local_subscriber_record_response(&legend), None);
        assert!(lgt_local_big_endian_record_response(&legend).is_some());
        assert_eq!(response(&legend), lgt_local_big_endian_record_response(&legend));
    }

    /// The 18 bytes 이노티아연대기2 writes when its shop is entered.
    fn inotia_2_session_request(command: u16) -> Vec<u8> {
        let mut request = vec![0u8; 2];
        request.extend_from_slice(&command.to_be_bytes());
        request.extend_from_slice(&2u16.to_be_bytes());
        request.extend_from_slice(b"01046119269\0");
        let body = (request.len() - 2) as u16;
        request[0..2].copy_from_slice(&body.to_be_bytes());

        request
    }

    #[test]
    fn a_session_hello_is_answered_with_a_code_and_no_message() {
        use super::lgt_local_command_tag_response;

        // Byte for byte what the capture shows going out.
        assert_eq!(
            inotia_2_session_request(0x0000),
            vec![
                0x00, 0x10, 0x00, 0x00, 0x00, 0x02, 0x30, 0x31, 0x30, 0x34, 0x36, 0x31, 0x31, 0x39, 0x32, 0x36, 0x39, 0x00
            ]
        );

        // The length counts the body alone, the command comes back as it was
        // asked under, the tag is echoed as the code the next step carries, the
        // status is the nonzero the handler needs, and the message is empty.
        assert_eq!(
            lgt_local_command_tag_response(&inotia_2_session_request(0x0000)).unwrap(),
            vec![0x00, 0x07, 0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x00]
        );

        // The step behind it reads a word it discards and a status of exactly 1.
        assert_eq!(
            lgt_local_command_tag_response(&inotia_2_session_request(0x014a)).unwrap(),
            vec![0x00, 0x05, 0x01, 0x4a, 0x00, 0x02, 0x01]
        );
    }

    /// What the shop writes for one of its tabs: the tab's code, the first row
    /// it wants and how many. Byte for byte what the capture shows.
    fn inotia_2_list_request(tab: u8, first: u8, wanted: u8) -> Vec<u8> {
        vec![0x00, 0x07, 0x01, 0x0f, 0x00, 0x02, tab, first, wanted]
    }

    #[test]
    fn a_shop_list_is_answered_with_the_tab_s_own_rows() {
        use super::{INOTIA_2_SHOP_TABS, lgt_local_command_tag_response};

        let request = inotia_2_list_request(1, 0, 100);
        assert_eq!(u16::from_be_bytes([request[0], request[1]]) as usize, request.len() - 2);

        let response = lgt_local_command_tag_response(&request).unwrap();
        assert_eq!(u16::from_be_bytes([response[0], response[1]]) as usize, response.len() - 2);
        // The command, the tag, the two bytes the screen reads past, and the
        // whole of the first tab.
        assert_eq!(
            &response[2..9],
            &[0x01, 0x0f, 0x00, 0x02, 0x00, 0x00, INOTIA_2_SHOP_TABS[0].1.len() as u8]
        );

        // Which walks as rows of an item, an empty string, a count, a price and
        // one more empty string, and accounts for the record exactly.
        let mut rest = &response[9..];
        for (item, count, price) in INOTIA_2_SHOP_TABS[0].1 {
            assert_eq!(&rest[0..4], &item.to_be_bytes());
            assert_eq!(rest[4], 0);
            assert_eq!(rest[5], *count);
            assert_eq!(&rest[6..10], &price.to_be_bytes());
            assert_eq!(rest[10], 0);
            rest = &rest[11..];
        }
        assert!(rest.is_empty());

        // Every tab the title asks for is one this knows, and each item is one
        // the title's own 970-entry table has.
        for tab in 1..=7 {
            let response = lgt_local_command_tag_response(&inotia_2_list_request(tab, 0, 100)).unwrap();
            assert!(response[8] > 0);
        }
        assert!(INOTIA_2_SHOP_TABS.iter().flat_map(|(_, rows)| *rows).all(|(item, ..)| *item < 970));

        // A tab that is not one of the seven is left unanswered.
        assert_eq!(lgt_local_command_tag_response(&inotia_2_list_request(8, 0, 100)), None);
    }

    #[test]
    fn a_shop_list_gives_back_only_the_rows_that_were_asked_for() {
        use super::{INOTIA_2_SHOP_TABS, lgt_local_command_tag_response};

        // Two rows from the second, and nothing past the end of the tab.
        let response = lgt_local_command_tag_response(&inotia_2_list_request(1, 1, 2)).unwrap();
        assert_eq!(response[8], 2);
        assert_eq!(&response[9..13], &INOTIA_2_SHOP_TABS[0].1[1].0.to_be_bytes());

        let response = lgt_local_command_tag_response(&inotia_2_list_request(1, 200, 100)).unwrap();
        assert_eq!(response[8], 0);
    }

    /// What the shop writes to buy a row: the subscriber number, the byte the
    /// builder fixes at 9, the row's name as the title's own table spells it,
    /// and a u16 quantity. Byte for byte what the capture shows for
    /// 용사의 인장, EUC-KR.
    fn inotia_2_buy_request(command: u16) -> Vec<u8> {
        let mut request = vec![0u8; 2];
        request.extend_from_slice(&command.to_be_bytes());
        request.extend_from_slice(&2u16.to_be_bytes());
        request.extend_from_slice(b"01046119269\0");
        request.push(9);
        let name = [0xbf, 0xeb, 0xbb, 0xe7, 0xc0, 0xc7, 0x20, 0xc0, 0xce, 0xc0, 0xe5];
        request.push(name.len() as u8);
        request.extend_from_slice(&name);
        request.extend_from_slice(&1u16.to_be_bytes());
        let body = (request.len() - 2) as u16;
        request[0..2].copy_from_slice(&body.to_be_bytes());

        request
    }

    #[test]
    fn a_purchase_is_answered_under_the_command_that_asked() {
        use super::lgt_local_command_tag_response;

        assert_eq!(
            inotia_2_buy_request(0x010e),
            vec![
                0x00, 0x1f, 0x01, 0x0e, 0x00, 0x02, 0x30, 0x31, 0x30, 0x34, 0x36, 0x31, 0x31, 0x39, 0x32, 0x36, 0x39, 0x00, 0x09, 0x0b, 0xbf, 0xeb,
                0xbb, 0xe7, 0xc0, 0xc7, 0x20, 0xc0, 0xce, 0xc0, 0xe5, 0x00, 0x01
            ]
        );

        // A refusal would come back as `0x0000` with a message, so answering
        // under the command that was asked is what grants it.
        assert_eq!(
            lgt_local_command_tag_response(&inotia_2_buy_request(0x010e)).unwrap(),
            vec![0x00, 0x05, 0x01, 0x0e, 0x00, 0x02, 0x01]
        );
        // And the commit the screen sends behind it, in the same shape.
        assert_eq!(
            lgt_local_command_tag_response(&inotia_2_buy_request(0x0112)).unwrap(),
            vec![0x00, 0x05, 0x01, 0x12, 0x00, 0x02, 0x01]
        );

        // A name field that does not account for the rest is not this request.
        let mut ragged = inotia_2_buy_request(0x010e);
        ragged[19] = 3;
        assert_eq!(lgt_local_command_tag_response(&ragged), None);
    }

    #[test]
    fn a_record_that_is_not_the_session_s_is_left_unanswered() {
        use super::{lgt_local_command_tag_response, lgt_local_subscriber_record_response};

        // A command that is not one of the handshake's.
        let mut unknown = inotia_2_session_request(0x0000);
        unknown[3] = 0x0c;
        assert_eq!(lgt_local_command_tag_response(&unknown), None);

        // A length that counts itself, which is the other game's convention.
        let mut inclusive = inotia_2_session_request(0x0000);
        let whole = inclusive.len() as u16;
        inclusive[0..2].copy_from_slice(&whole.to_be_bytes());
        assert_eq!(lgt_local_command_tag_response(&inclusive), None);

        // A subscriber number that is not digits.
        let mut lettered = inotia_2_session_request(0x0000);
        lettered[6] = b'x';
        assert_eq!(lgt_local_command_tag_response(&lettered), None);

        // And a command carrying a payload that is not the one it carries.
        assert_eq!(
            lgt_local_command_tag_response(&[0x00, 0x07, 0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x64]),
            None
        );

        // And neither record is mistaken for the other game's, either way.
        assert_eq!(lgt_local_subscriber_record_response(&inotia_2_session_request(0x0000)), None);
        assert_eq!(lgt_local_command_tag_response(&inotia_shop_request()), None);
        assert_eq!(
            response(&inotia_shop_request()),
            lgt_local_subscriber_record_response(&inotia_shop_request())
        );
    }

    /// The 1024-byte block 라그나로크 바이올렛 writes when its shop is entered,
    /// byte for byte as the capture shows it, zero padding included.
    fn ragnarok_violet_hello_block() -> Vec<u8> {
        let mut block = vec![0u8; 1024];
        block[0..4].copy_from_slice(&0x66u32.to_be_bytes());
        block[4..8].copy_from_slice(&0xc9u32.to_be_bytes());
        block[8..12].copy_from_slice(&0x33u32.to_be_bytes());
        block[16..20].copy_from_slice(&4u32.to_be_bytes());

        let mut at = 20;
        for field in [b"1046119269".as_slice(), b"Emulator".as_slice(), b"ver 1.0.2".as_slice()] {
            block[at..at + 4].copy_from_slice(&(field.len() as u32).to_be_bytes());
            block[at + 4..at + 4 + field.len()].copy_from_slice(field);
            at += 4 + field.len();
        }
        block[at..at + 4].copy_from_slice(&4u32.to_be_bytes());
        block[at + 4..at + 8].copy_from_slice(&2u32.to_be_bytes());

        block
    }

    #[test]
    fn a_fixed_block_is_answered_with_the_step_that_follows_it() {
        use super::{RAGNAROK_VIOLET_SHOP_ROWS, lgt_local_fixed_block_response};

        let request = ragnarok_violet_hello_block();
        assert_eq!(
            &request[..24],
            &[0, 0, 0, 0x66, 0, 0, 0, 0xc9, 0, 0, 0, 0x33, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0x0a]
        );
        // What [8] counts is the block from [16] on: the step's own word, the
        // three fields and the two words behind them.
        assert_eq!(u32::from_be_bytes([request[8], request[9], request[10], request[11]]), 0x33);

        // A reply is one block of the same size, under the same screen, at the
        // step that answers the one asked, and that step is the shop's list.
        let response = lgt_local_fixed_block_response(&request).unwrap();
        assert_eq!(response.len(), 1024);
        assert_eq!(&response[0..8], &[0, 0, 0, 0x66, 0, 0, 0, 0xca]);

        // What it says of itself is the count and its rows, measured from [16]
        // the way the request's own length is.
        let rows = RAGNAROK_VIOLET_SHOP_ROWS.len();
        assert_eq!(
            u32::from_be_bytes([response[8], response[9], response[10], response[11]]) as usize,
            4 + rows * 12
        );
        assert_eq!(
            u32::from_be_bytes([response[16], response[17], response[18], response[19]]) as usize,
            rows
        );

        // Which walks as an item, a quantity and a price, and each item is one
        // the title's own 540-entry table has.
        for (index, (item, count, price)) in RAGNAROK_VIOLET_SHOP_ROWS.iter().enumerate() {
            let at = 20 + index * 12;
            assert!(*item < 540);
            assert_eq!(&response[at..at + 4], &item.to_be_bytes());
            assert_eq!(&response[at + 4..at + 8], &count.to_be_bytes());
            assert_eq!(&response[at + 8..at + 12], &price.to_be_bytes());
        }
        assert!(response[20 + rows * 12..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn the_step_behind_the_hello_is_answered_with_an_empty_message() {
        use super::lgt_local_fixed_block_response;

        // What the capture shows going out once the hello is answered: the same
        // block with the step moved on and a zero length over the body the send
        // buffer still holds.
        let mut request = ragnarok_violet_hello_block();
        request[4..8].copy_from_slice(&0xcbu32.to_be_bytes());
        request[8..12].copy_from_slice(&0u32.to_be_bytes());
        request[16..20].copy_from_slice(&0u32.to_be_bytes());

        // The message the screen draws, answered as none.
        let response = lgt_local_fixed_block_response(&request).unwrap();
        assert_eq!(response.len(), 1024);
        assert_eq!(&response[0..12], &[0, 0, 0, 0x66, 0, 0, 0, 0xcd, 0, 0, 0, 0]);
        assert!(response[12..].iter().all(|&byte| byte == 0));
    }

    /// The block the shop writes to buy a row, and the one behind it. Neither
    /// carries a body of its own; the send buffer still holds the shop's.
    fn ragnarok_violet_purchase_block(step: u32) -> Vec<u8> {
        let mut block = ragnarok_violet_hello_block();
        block[0..4].copy_from_slice(&0x6bu32.to_be_bytes());
        block[4..8].copy_from_slice(&step.to_be_bytes());
        block[8..12].copy_from_slice(&0u32.to_be_bytes());
        block[16..20].copy_from_slice(&0u32.to_be_bytes());

        block
    }

    #[test]
    fn a_purchase_is_answered_with_the_one_code_that_grants_it() {
        use super::lgt_local_fixed_block_response;

        // The screen opens with nothing to say, and is answered the same way.
        let response = lgt_local_fixed_block_response(&ragnarok_violet_purchase_block(0xc9)).unwrap();
        assert_eq!(&response[0..12], &[0, 0, 0, 0x6b, 0, 0, 0, 0xca, 0, 0, 0, 0]);
        assert!(response[12..].iter().all(|&byte| byte == 0));

        // The row it chose comes behind that, and the result is a word past the
        // step's own word rather than the step's word itself.
        let response = lgt_local_fixed_block_response(&ragnarok_violet_purchase_block(0xcb)).unwrap();
        assert_eq!(response.len(), 1024);
        assert_eq!(&response[0..12], &[0, 0, 0, 0x6b, 0, 0, 0, 0xcd, 0, 0, 0, 8]);
        assert_eq!(u32::from_be_bytes([response[20], response[21], response[22], response[23]]), 301);
        assert!(response[24..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn only_a_block_this_knows_the_step_of_is_answered() {
        use super::lgt_local_fixed_block_response;

        // A block that is not the full 1024 the title reads.
        let mut short = ragnarok_violet_hello_block();
        short.truncate(1023);
        assert_eq!(lgt_local_fixed_block_response(&short), None);

        // A screen outside the table `0x38e78` indexes.
        let mut offscreen = ragnarok_violet_hello_block();
        offscreen[0..4].copy_from_slice(&0x76u32.to_be_bytes());
        assert_eq!(lgt_local_fixed_block_response(&offscreen), None);

        // A step this does not answer.
        let mut unknown = ragnarok_violet_hello_block();
        unknown[4..8].copy_from_slice(&0xccu32.to_be_bytes());
        assert_eq!(lgt_local_fixed_block_response(&unknown), None);

        // A subscriber number that is not digits.
        let mut lettered = ragnarok_violet_hello_block();
        lettered[24] = b'x';
        assert_eq!(lgt_local_fixed_block_response(&lettered), None);

        // A length that does not fit the block.
        let mut overlong = ragnarok_violet_hello_block();
        overlong[8..12].copy_from_slice(&2000u32.to_be_bytes());
        assert_eq!(lgt_local_fixed_block_response(&overlong), None);
    }

    /// The 67 bytes 블레이드마스터4 writes to buy 4000하트 for 2900원, byte
    /// for byte as the capture shows them.
    fn blade_master_4_purchase_record() -> Vec<u8> {
        let mut record = Vec::from(*b"ENSLGT");
        record.push(0x11);
        record.extend_from_slice(&0x79u16.to_le_bytes());
        record.extend_from_slice(&0x31u16.to_le_bytes());
        record.extend_from_slice(&0x36u16.to_le_bytes());

        let mut subscriber = [0u8; 21];
        subscriber[..11].copy_from_slice(b"01046119269");
        record.extend_from_slice(&subscriber);

        let mut handset = [0u8; 10];
        handset[..8].copy_from_slice(b"Emulator");
        record.extend_from_slice(&handset);

        record.extend_from_slice(b"100");
        let mut product = [0u8; 20];
        product[..11].copy_from_slice(b"0002BA50004");
        product[16..].copy_from_slice(&2900u32.to_le_bytes());
        record.extend_from_slice(&product);

        record
    }

    #[test]
    fn an_ens_record_is_answered_with_the_result_that_grants_it() {
        use super::lgt_local_ens_record_response;

        let request = blade_master_4_purchase_record();
        assert_eq!(request.len(), 67);
        assert_eq!(
            &request[..24],
            &[
                0x45, 0x4e, 0x53, 0x4c, 0x47, 0x54, 0x11, 0x79, 0x00, 0x31, 0x00, 0x36, 0x00, 0x30, 0x31, 0x30, 0x34, 0x36, 0x31, 0x31, 0x39, 0x32,
                0x36, 0x39
            ]
        );
        // The price the screen names, little-endian behind the product code.
        assert_eq!(&request[63..], &2900u32.to_le_bytes());

        // The reply is found by its own magic, and carries the exchange it was
        // asked under, a length, and the one result that is not an error.
        assert_eq!(
            lgt_local_ens_record_response(&request).unwrap(),
            vec![0x45, 0x4e, 0x53, 0x31, 0x00, 0x02, 0x00, 0x00, 0x00]
        );

        // The step behind it, which the capture shows going out once the first
        // is granted, and the two behind that.
        let mut step = blade_master_4_purchase_record();
        step[9..11].copy_from_slice(&0x32u16.to_le_bytes());
        assert_eq!(
            lgt_local_ens_record_response(&step).unwrap(),
            vec![0x45, 0x4e, 0x53, 0x32, 0x00, 0x02, 0x00, 0x00, 0x00]
        );

        // `0x4e544` reads a word behind the result for this one alone.
        step[9..11].copy_from_slice(&0x33u16.to_le_bytes());
        assert_eq!(
            lgt_local_ens_record_response(&step).unwrap(),
            vec![0x45, 0x4e, 0x53, 0x33, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );

        step[9..11].copy_from_slice(&0x34u16.to_le_bytes());
        assert_eq!(
            lgt_local_ens_record_response(&step).unwrap(),
            vec![0x45, 0x4e, 0x53, 0x34, 0x00, 0x02, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn only_an_ens_record_this_knows_the_exchange_of_is_answered() {
        use super::lgt_local_ens_record_response;

        // A record that is not the length the builder writes.
        let mut short = blade_master_4_purchase_record();
        short.pop();
        assert_eq!(lgt_local_ens_record_response(&short), None);

        // A length that is not what the record carries.
        let mut mislength = blade_master_4_purchase_record();
        mislength[11] = 0x37;
        assert_eq!(lgt_local_ens_record_response(&mislength), None);

        // Another exchange of the same walk is answered under its own number,
        // and only 0x33 carries a word behind the result.
        let mut other = blade_master_4_purchase_record();
        other[9..11].copy_from_slice(&0x32u16.to_le_bytes());
        let reply = lgt_local_ens_record_response(&other).unwrap();
        assert_eq!(&reply[..3], b"ENS");
        assert_eq!(u16::from_le_bytes(reply[3..5].try_into().unwrap()), 0x32);
        assert_eq!(u16::from_le_bytes(reply[5..7].try_into().unwrap()) as usize, reply.len() - 7);
        assert_eq!(reply.len(), 9);

        // A subscriber number that is not digits.
        let mut lettered = blade_master_4_purchase_record();
        lettered[13] = b'x';
        assert_eq!(lgt_local_ens_record_response(&lettered), None);

        // And the bytes the builder fixes.
        let mut unmarked = blade_master_4_purchase_record();
        unmarked[6] = 0x12;
        assert_eq!(lgt_local_ens_record_response(&unmarked), None);
    }

    /// 드래곤하트2 writes the same record on starting up, and stalls on
    /// 기존에 저장되어 있는 데이터가 있는지 확인중입니다 until it is answered.
    #[test]
    fn the_record_dragon_heart_2_starts_up_with_is_answered_too() {
        use super::lgt_local_ens_record_response;

        let mut record = Vec::from(&b"ENSLGT"[..]);
        record.push(0x11);
        record.extend_from_slice(&0x7bu16.to_le_bytes());
        record.extend_from_slice(&0x26u16.to_le_bytes());
        record.extend_from_slice(&0x23u16.to_le_bytes());
        // The subscriber number in a fixed twenty-one bytes, then the handset
        // in ten.
        record.extend_from_slice(b"01083062925");
        record.extend_from_slice(&[0; 10]);
        record.extend_from_slice(b"Emulator");
        record.extend_from_slice(&[0, 0]);
        record.extend_from_slice(b"103");
        record.push(1);

        // The capture is 48 bytes and says 0x23 behind its thirteen-byte header.
        assert_eq!(record.len(), 48);
        assert_eq!(u16::from_le_bytes(record[11..13].try_into().unwrap()) as usize, record.len() - 13);

        // "ENS", the exchange back, the body length, and a result of zero -
        // which is the whole of what `0x24448` waits on before it dispatches.
        let reply = lgt_local_ens_record_response(&record).unwrap();
        assert_eq!(reply, [b'E', b'N', b'S', 0x26, 0x00, 0x02, 0x00, 0x00, 0x00]);

        assert_eq!(response(&record), Some(reply));
    }

    #[test]
    fn one_answer_covers_every_protocol_and_guesses_at_none() {
        // Each of the eleven, recognised by its own shape.
        assert!(response(&[0xff, 0xff, 0x06, 0x00, 0x20, 0x00]).is_some());
        assert!(response(b"CASH|0|demon|05590091|00029B60004|500|2034517541").is_some());
        assert!(response(&zenonia_purchase_request()).is_some());
        assert!(response(&legend_of_master_purchase_request()).is_some());
        assert!(response(&hero_lore_frame(1, 1, &[4])).is_some());
        assert!(response(&anima_purchase_request()).is_some());
        assert!(response(&wild_frontier_purchase_request()).is_some());
        assert!(response(&wild_frontier_2_purchase_request()).is_some());
        assert!(response(&inotia_shop_request()).is_some());
        assert!(response(&inotia_2_session_request(0x0000)).is_some());
        assert!(response(&ragnarok_violet_hello_block()).is_some());
        assert!(response(&blade_master_4_purchase_record()).is_some());

        // And nothing for a request that is none of them.
        assert_eq!(response(b"hello"), None);
        assert_eq!(response(&[0u8; 64]), None);
    }
    /// The opening message of 엘피스's online menu is answered with its own
    /// opcode and nothing behind it, which is what `0x488f0` needs to run the
    /// screen's next request.
    #[test]
    fn a_session_opening_is_answered_with_its_own_opcode_and_no_body() {
        let request = elpis_session_opening();

        let response = lgt_local_opcode_header_response(&request).unwrap();

        assert_eq!(response, vec![0x00, 0x05, 0x00, 0x00, 0x00]);
        assert_eq!(u16::from_be_bytes([response[0], response[1]]) as usize, response.len());

        // The signal the library's type 5 writes is the same kind of thing, and
        // `0x47e70` reads nothing out of the answer either.
        let response = lgt_local_opcode_header_response(&[0x00, 0x05, 0x00, 0x01, 0x00]).unwrap();
        assert_eq!(response, vec![0x00, 0x05, 0x00, 0x01, 0x00]);
    }

    /// 슈퍼액션히어로3 opens the same session with the same layout stopped at
    /// the subscriber's number, and is answered the same way.
    #[test]
    fn that_opening_is_answered_whether_or_not_it_carries_a_handset_model() {
        // The frame that title wrote, byte for byte.
        let mut request = vec![0x00, 0x49, 0x00, 0x00, 0x00];
        request.extend_from_slice(&1017u16.to_be_bytes());
        request.extend_from_slice(&[0x00, 0x00, 0x02, 0x05]);
        request.extend_from_slice(b"V.1.0.0");
        request.resize(5 + 6 + 20, 0);
        request.extend_from_slice(b"01062170215");
        request.resize(5 + 68, 0);
        assert_eq!(request.len(), 0x49);

        let response = lgt_local_opcode_header_response(&request).unwrap();
        assert_eq!(response, vec![0x00, 0x05, 0x00, 0x00, 0x00]);

        // Once its session is open it sends opcode 0x32, and 0x48c5c takes a
        // u32 out of that answer.
        let mut report = vec![0x00, 0x66, 0x00, 0x32, 0x00];
        report.resize(0x66, 0);
        let answer = lgt_local_opcode_header_response(&report).unwrap();
        assert_eq!(answer, vec![0x00, 0x09, 0x00, 0x32, 0x00, 0x00, 0x00, 0x00, 0x00]);

        // With nothing behind its header it is not that message.
        assert!(lgt_local_opcode_header_response(&[0x00, 0x05, 0x00, 0x32, 0x00]).is_none());

        // Shorter than the layout names is not that opening.
        let mut clipped = request.clone();
        clipped.truncate(5 + 67);
        clipped[1] = clipped.len() as u8;
        assert!(lgt_local_opcode_header_response(&clipped).is_none());

        // Neither is a service this opening never carries.
        let mut other_service = request;
        other_service[5..7].copy_from_slice(&1018u16.to_be_bytes());
        assert!(lgt_local_opcode_header_response(&other_service).is_none());
    }

    /// Each step of the purchase walk is answered with what its own reader takes
    /// out of the body, under the opcode that reader is registered at.
    #[test]
    fn a_purchase_walk_is_answered_a_step_at_a_time() {
        // `0x474a4`: the product code and a quantity, and `0x480c6` takes a u32.
        let order = lgt_local_opcode_header_response(&[0x00, 0x09, 0x00, 0x43, 0x00, 0x00, 0x29, 0x00, 0x01]).unwrap();
        assert_eq!(order[3], 0x43);
        assert_eq!(order.len(), 5 + 4);

        // `0x47604`: the amount, and `0x47fea` takes a u32 and eight bytes.
        let mut approval = vec![0x00, 0x0d, 0x00, 0xc9, 0x00];
        approval.extend_from_slice(&0x14u16.to_be_bytes());
        approval.extend_from_slice(&4000u32.to_be_bytes());
        approval.extend_from_slice(&1u16.to_be_bytes());
        let approved = lgt_local_opcode_header_response(&approval).unwrap();
        assert_eq!(approved[3], 0xca);
        assert_eq!(approved.len(), 5 + 4 + 8);

        // `0x47b04` sends those twelve back, and `0x47dd0` takes a u32.
        let mut confirm = vec![0x00, 0x11, 0x00, 0xcb, 0x00];
        confirm.extend_from_slice(&approved[5..]);
        let confirmed = lgt_local_opcode_header_response(&confirm).unwrap();
        assert_eq!(confirmed[3], 0xcc);
        assert_eq!(confirmed.len(), 5 + 4);

        // `0x47b5c` closes it out with the order, the code, the quantity and the
        // order's code, and `0x481d6` reads none of the answer's body.
        let mut settle = vec![0x00, 0x15, 0x00, 0x44, 0x00];
        settle.extend_from_slice(&confirmed[5..]);
        settle.extend_from_slice(&0x29u16.to_be_bytes());
        settle.extend_from_slice(&1u16.to_be_bytes());
        settle.extend_from_slice(&approved[9..]);
        let settled = lgt_local_opcode_header_response(&settle).unwrap();
        assert_eq!(settled[3], 0x44);
        assert_eq!(settled.len(), 5);

        for answer in [order, approved, confirmed, settled] {
            assert_eq!(u16::from_be_bytes([answer[0], answer[1]]) as usize, answer.len());
            assert_eq!((answer[2], answer[4]), (0, 0));
        }
    }

    /// 창세기전3 에피소드4's data-server opening, captured off its socket: a
    /// big-endian length and four little-endian words, then the number.
    fn genesis3_episode4_opening(subscriber: &[u8]) -> Vec<u8> {
        let mut request = Vec::new();
        request.extend_from_slice(&40u32.to_be_bytes());
        request.extend_from_slice(&1u32.to_le_bytes());
        request.extend_from_slice(&104u32.to_le_bytes());
        request.extend_from_slice(&34u32.to_le_bytes());
        request.extend_from_slice(&0u32.to_le_bytes());
        let mut number = alloc::vec![0u8; 20];
        number[..subscriber.len()].copy_from_slice(subscriber);
        request.extend_from_slice(&number);
        request
    }

    /// 에피소드4's poll takes two little-endian words off the front of the
    /// answer and wants the second to be the id it opened the message under.
    #[test]
    fn 에피소드4_is_answered_under_the_id_it_opened_with() {
        let request = genesis3_episode4_opening(b"01085300848");
        assert_eq!(request.len(), 40);

        let answer = lgt_local_genesis3_episode4_response(&request).unwrap();

        assert_eq!(u32::from_be_bytes([answer[0], answer[1], answer[2], answer[3]]) as usize, answer.len());

        // Both words carry the id, so the one the title compares is 1 whether
        // it lines its buffer up on the length or behind it.
        assert_eq!(u32::from_le_bytes([answer[4], answer[5], answer[6], answer[7]]), 1);
        assert_eq!(u32::from_le_bytes([answer[8], answer[9], answer[10], answer[11]]), 1);
        assert_eq!(answer.len(), 12);

        assert_eq!(response(&request), Some(answer));
    }

    /// Answering the opening moves the walk on, and what comes next is the
    /// header alone under an id of its own - so the answer takes the id off the
    /// frame in hand rather than the one the session opened with.
    #[test]
    fn 에피소드4_answers_the_step_after_the_opening_under_its_own_id() {
        // Captured off the socket once the opening was answered.
        let mut request = Vec::new();
        request.extend_from_slice(&12u32.to_be_bytes());
        request.extend_from_slice(&17u32.to_le_bytes());
        request.extend_from_slice(&1u32.to_le_bytes());

        let answer = lgt_local_genesis3_episode4_response(&request).unwrap();

        assert_eq!(u32::from_be_bytes([answer[0], answer[1], answer[2], answer[3]]) as usize, answer.len());
        assert_eq!(u32::from_le_bytes([answer[4], answer[5], answer[6], answer[7]]), 17);
        assert_eq!(u32::from_le_bytes([answer[8], answer[9], answer[10], answer[11]]), 17);

        assert_eq!(response(&request), Some(answer));
    }

    /// The opening is that one frame, and the episodes before it are not it.
    #[test]
    fn 에피소드4_answers_only_its_own_opening() {
        let request = genesis3_episode4_opening(b"01085300848");

        // A message the session does not open with.
        let mut other_message = request.clone();
        other_message[8..12].copy_from_slice(&105u32.to_le_bytes());
        assert!(lgt_local_genesis3_episode4_response(&other_message).is_none());

        // A length that is not the frame in hand.
        let mut ragged = request.clone();
        ragged[0..4].copy_from_slice(&39u32.to_be_bytes());
        assert!(lgt_local_genesis3_episode4_response(&ragged).is_none());

        // 에피소드2's opening declares its length the other way round and is
        // not this one to answer.
        let mut episode2 = alloc::vec![0xfa, 0xcb];
        episode2.extend_from_slice(&32u32.to_be_bytes());
        episode2.extend_from_slice(&100u32.to_be_bytes());
        episode2.resize(42, 0);
        assert!(lgt_local_genesis3_episode4_response(&episode2).is_none());
    }

    /// 아이뮤지션2's licence check, captured off its billing socket: the length,
    /// opcode `0x14`, and the body its sender at `0x1e3b2` lays out.
    fn imusician_licence_request() -> Vec<u8> {
        let mut request = vec![0x00, 0x00, 0x00, 0x14, 0x00];
        request.push(0x0c);
        request.extend_from_slice(&1064u16.to_be_bytes());
        let mut model = vec![0u8; 50];
        model[..8].copy_from_slice(b"Emulator");
        request.extend_from_slice(&model);
        request.push(0x03);
        let mut application = vec![0u8; 20];
        application[..8].copy_from_slice(b"00032548");
        request.extend_from_slice(&application);

        let whole = request.len() as u16;
        request[0..2].copy_from_slice(&whole.to_be_bytes());
        request
    }

    /// The licence check is answered the way `0x1d854` reads it - a verdict
    /// byte, a `u16` length, and that many bytes - under its own opcode.
    #[test]
    fn a_licence_check_is_answered_with_the_verdict_its_reader_takes() {
        let request = imusician_licence_request();
        assert_eq!(request.len(), 79);

        let answer = lgt_local_opcode_header_response(&request).unwrap();

        assert_eq!(u16::from_be_bytes([answer[0], answer[1]]) as usize, answer.len());
        assert_eq!((answer[2], answer[4]), (0, 0));
        assert_eq!(answer[3], 0x14);

        // The verdict, and a string of nothing behind its length. `0x1d854`
        // takes three bytes and then the length's worth, so this is the whole
        // of what it reads.
        assert_eq!(answer[5], 0);
        let text = u16::from_be_bytes([answer[6], answer[7]]) as usize;
        assert_eq!(text, 0);
        assert_eq!(answer.len(), 5 + 3 + text);

        assert_eq!(response(&request), Some(answer));
    }

    /// The licence check is that one body. A request under its opcode that is
    /// not the shape `0x1e3b2` writes is not a licence check to answer.
    #[test]
    fn a_licence_check_is_the_body_its_sender_writes() {
        let mut short = imusician_licence_request();
        short.truncate(short.len() - 1);
        let whole = short.len() as u16;
        short[0..2].copy_from_slice(&whole.to_be_bytes());
        assert!(lgt_local_opcode_header_response(&short).is_none());

        let mut long = imusician_licence_request();
        long.push(0);
        let whole = long.len() as u16;
        long[0..2].copy_from_slice(&whole.to_be_bytes());
        assert!(lgt_local_opcode_header_response(&long).is_none());
    }

    /// 던파귀검사편's authentication request is answered under the command its
    /// own reader is registered at, with the body that reader takes.
    #[test]
    fn the_authentication_this_title_waits_on_is_answered() {
        // The frame the title wrote, byte for byte, off the socket it opened
        // for 211.115.203.30.
        let request = [
            0x29, 0x00, 0x00, 0x00, 0xff, 0xff, 0x11, 0x27, 0x0b, 0x00, b'D', b'n', b'F', b'S', b'w', b'o', b'r', b'd', b'M', b'a', b'n', 0x12, 0x00,
            b'U', b's', b'e', b'r', b'A', b'u', b't', b'h', b'e', b'n', b't', b'i', b'c', b'a', b't', b'i', b'o', b'n',
        ];
        assert_eq!(request.len(), 0x29);

        let response = lgt_local_marked_command_response(&request).unwrap();

        // Its own length, the marker, the answer's command, and the three bytes
        // `0xe360` takes: a granted result and an empty message.
        assert_eq!(response, vec![0x0b, 0x00, 0x00, 0x00, 0xff, 0xff, 0x12, 0x27, 0x00, 0x00, 0x00]);
        assert_eq!(u32::from_le_bytes(response[0..4].try_into().unwrap()) as usize, response.len());

        // `0x70a0` hands `0xe3c0` a null pointer for a frame that declares
        // nothing past its header, which `0xe360` would read from.
        assert!(response.len() > 8);

        // 바람의나라 writes the same frame under its own name, and is answered
        // the same way.
        let mut baram = vec![0x23, 0x00, 0x00, 0x00, 0xff, 0xff, 0x11, 0x27, 0x05, 0x00];
        baram.extend_from_slice(b"Baram");
        baram.extend_from_slice(&[0x12, 0x00]);
        baram.extend_from_slice(b"UserAuthentication");
        assert_eq!(baram.len(), 0x23);
        assert_eq!(lgt_local_marked_command_response(&baram).unwrap(), response);

        // The screen that asks under the title's own name is the same request.
        let mut first_pass = Vec::from(&request[..21]);
        first_pass.extend_from_slice(&[0x0b, 0x00]);
        first_pass.extend_from_slice(b"DnFSwordMan");
        first_pass[0] = first_pass.len() as u8;
        assert_eq!(lgt_local_marked_command_response(&first_pass).unwrap(), response);
    }

    /// The step the title takes once authentication has gone through is
    /// answered under the command its own reader is registered at.
    #[test]
    fn the_step_past_that_title_s_authentication_is_answered_too() {
        // The pairs 0x7890 spells out, as the title wrote them.
        let pairs: &[u8] = b"  phonenum:01024417543 carrier:lgt platform:lgt_wipic app_name:DnFSwordMan sms:(null) ";
        let mut request = Vec::new();
        request.extend_from_slice(&((8 + 2 + pairs.len()) as u32).to_le_bytes());
        request.extend_from_slice(&[0xff, 0xff, 0xa0, 0x28]);
        request.extend_from_slice(&(pairs.len() as u16).to_le_bytes());
        request.extend_from_slice(pairs);

        let response = lgt_local_marked_command_response(&request).unwrap();

        // `0xe140` takes a result, eleven bytes it drops, and a message length
        // at [12..14] - so the answer is fourteen bytes behind its header.
        assert_eq!(response[0..8], [0x16, 0x00, 0x00, 0x00, 0xff, 0xff, 0xa1, 0x28]);
        assert_eq!(response.len(), 8 + 14);
        assert_eq!(u32::from_le_bytes(response[0..4].try_into().unwrap()) as usize, response.len());

        // 0 is the result `0xb234` closes the socket on here, unlike the
        // authentication answer, where 0 is the one it goes on from.
        assert_eq!(response[8], 1);
        assert_eq!(response[9..20], [0; 11]);
        assert_eq!(response[20..22], [0, 0]);

        // A string that does not end where the frame does is not that request.
        let mut overrun = request.clone();
        overrun[8] = 0xff;
        assert!(lgt_local_marked_command_response(&overrun).is_none());

        // Nor is a frame carrying a second string behind it.
        let mut trailing = request.clone();
        trailing.extend_from_slice(&[0x00, 0x00]);
        trailing[0] += 2;
        assert!(lgt_local_marked_command_response(&trailing).is_none());
    }

    /// The step a screen that opened its own connection takes - 세라샵 does, to
    /// buy an item - is answered under its own reader's command.
    #[test]
    fn the_step_a_screen_s_own_connection_takes_is_answered() {
        // The frame the title wrote off the socket 세라샵 opened.
        let mut request = vec![0x15, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00];
        request.extend_from_slice(b"01024417543");
        request.extend_from_slice(&1u16.to_le_bytes());
        assert_eq!(request.len(), 0x15);

        let response = lgt_local_marked_command_response(&request).unwrap();

        // `0xe084` takes a result, a message length, the message, and four
        // bytes past it - which it only reads for a granted result.
        assert_eq!(
            response,
            vec![0x0f, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(u32::from_le_bytes(response[0..4].try_into().unwrap()) as usize, response.len());

        // 0 is what `0xb234` goes on from here, where the step before it stops
        // on 0 - the two are not the same answer.
        assert_eq!(response[8], 0);

        // Thirteen bytes that are not a subscriber's number are some other
        // frame that happens to be under a command as plain as zero.
        let mut lettered = request.clone();
        lettered[8] = b'x';
        assert!(lgt_local_marked_command_response(&lettered).is_none());

        // Nor is a body of another length.
        let mut longer = request.clone();
        longer.push(0);
        longer[0] += 1;
        assert!(lgt_local_marked_command_response(&longer).is_none());
    }

    /// The errand a screen opened its connection for is answered under its own
    /// reader's command.
    #[test]
    fn the_errand_that_connection_was_opened_for_is_answered() {
        // What 0x7390 writes: a u32 code, then the byte it always puts behind.
        let request = [0x0d, 0x00, 0x00, 0x00, 0xff, 0xff, 0x20, 0x00, 0x42, 0x00, 0x00, 0x00, 0x1e];

        let response = lgt_local_marked_command_response(&request).unwrap();

        // `0xc098` reads a result, a message length and that many bytes, the
        // same three fields the authentication answer carries.
        assert_eq!(response, vec![0x0b, 0x00, 0x00, 0x00, 0xff, 0xff, 0x21, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(u32::from_le_bytes(response[0..4].try_into().unwrap()) as usize, response.len());

        // Five bytes that do not end the way 0x7390 ends them are some other
        // frame under a command this plain.
        let mut other = request;
        other[12] = 0x00;
        assert!(lgt_local_marked_command_response(&other).is_none());
    }

    /// A frame that is not that request is left alone, whether it is another
    /// title's or this one's own next step.
    #[test]
    fn only_that_title_s_authentication_request_is_answered() {
        let request = [
            0x29, 0x00, 0x00, 0x00, 0xff, 0xff, 0x11, 0x27, 0x0b, 0x00, b'D', b'n', b'F', b'S', b'w', b'o', b'r', b'd', b'M', b'a', b'n', 0x12, 0x00,
            b'U', b's', b'e', b'r', b'A', b'u', b't', b'h', b'e', b'n', b't', b'i', b'c', b'a', b't', b'i', b'o', b'n',
        ];

        // A length that is not the frame in hand.
        let mut short = request;
        short[0] = 0x28;
        assert!(lgt_local_marked_command_response(&short).is_none());

        // No marker.
        let mut unmarked = request;
        unmarked[4] = 0;
        assert!(lgt_local_marked_command_response(&unmarked).is_none());

        // `0x7890`'s later `0x50`, which this does not know the answer to yet.
        let mut next_step = request;
        next_step[6] = 0x50;
        next_step[7] = 0x00;
        assert!(lgt_local_marked_command_response(&next_step).is_none());

        // A name that is not one - the first string is only held to that.
        let mut unnamed = request;
        unnamed[10] = 0;
        assert!(lgt_local_marked_command_response(&unnamed).is_none());

        // A string that runs past the frame.
        let mut overrun = request;
        overrun[8] = 0xff;
        assert!(lgt_local_marked_command_response(&overrun).is_none());

        // A header with nothing behind it.
        assert!(lgt_local_marked_command_response(&[0x08, 0x00, 0x00, 0x00, 0xff, 0xff, 0x11, 0x27]).is_none());
    }

    /// 바이오크로니클's login is answered with the session its own reader keeps.
    #[test]
    fn the_login_that_title_waits_on_is_answered_with_a_session() {
        // The header off the socket, with the body the title wrote behind it.
        let mut request = vec![0x00, 0x00, 0x00, 0x7a, 0x76, 0x00, 0x00, 0x00];
        request.extend_from_slice(&0u32.to_le_bytes());
        request.extend_from_slice(&0u32.to_le_bytes());
        request.push(0);
        request.extend_from_slice(&123_456_789u32.to_le_bytes());
        request.extend_from_slice(&[0u8; 101]);
        assert_eq!(request.len(), 0x7a);

        let response = lgt_local_biochronicle_response(&request).unwrap();

        assert_eq!(
            response,
            vec![
                0x00, 0x00, 0x00, 0x15, 0x11, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x15, 0xcd, 0x5b, 0x07
            ]
        );

        // Both ends of the length agree, which is what the reader and the
        // queue its main loop walks each take.
        assert_eq!(u32::from_be_bytes(response[0..4].try_into().unwrap()) as usize, response.len());
        assert_eq!(u32::from_le_bytes(response[4..8].try_into().unwrap()) as usize, response.len() - 4);

        // The session is what `0x44368` keeps, and zero is what the title
        // already has.
        assert_ne!(u32::from_le_bytes(response[12..16].try_into().unwrap()), 0);
    }

    /// The step that title takes once it has a session is answered under its
    /// own reader's command.
    #[test]
    fn the_step_past_that_login_is_answered() {
        // The frame the title wrote with the session it had just been issued.
        let mut request = vec![0x00, 0x00, 0x00, 0x19, 0x15, 0x00, 0x00, 0x00];
        request.extend_from_slice(&0x36u32.to_le_bytes());
        request.extend_from_slice(&1u32.to_le_bytes());
        request.push(0);
        request.extend_from_slice(&123_456_789u32.to_le_bytes());
        request.extend_from_slice(&[0x3a, 0x9d, 0x4f, 0x7f]);
        assert_eq!(request.len(), 0x19);

        let response = lgt_local_biochronicle_response(&request).unwrap();

        // Command 0x37, the session it was asked under, and the two u32
        // `0x45a46` takes - the first of which has to be 0.
        assert_eq!(response[0..4], [0x00, 0x00, 0x00, 0x1d]);
        assert_eq!(u32::from_le_bytes(response[4..8].try_into().unwrap()) as usize, response.len() - 4);
        assert_eq!(u32::from_le_bytes(response[8..12].try_into().unwrap()), 0x37);
        assert_eq!(u32::from_le_bytes(response[12..16].try_into().unwrap()), 1);
        assert_eq!(response[21..], [0; 8]);
    }

    /// The frame that title repeats while it waits is not the login, and
    /// neither is anything else that is not shaped like one.
    #[test]
    fn only_that_title_s_login_is_answered() {
        let mut login = vec![0x00, 0x00, 0x00, 0x15, 0x11, 0x00, 0x00, 0x00];
        login.extend_from_slice(&0u32.to_le_bytes());
        login.extend_from_slice(&0u32.to_le_bytes());
        login.push(0);
        login.extend_from_slice(&123_456_789u32.to_le_bytes());
        assert!(lgt_local_biochronicle_response(&login).is_some());

        // Command 8, the one it repeats every three seconds - answered by a
        // reader that walks other players' sessions, not this.
        let mut keepalive = login.clone();
        keepalive[8] = 8;
        assert!(lgt_local_biochronicle_response(&keepalive).is_none());

        // Without the constant it is not this title's frame.
        let mut plain = login.clone();
        plain[17] = 0;
        assert!(lgt_local_biochronicle_response(&plain).is_none());

        // The two lengths have to agree with the frame and with each other.
        let mut mismatched = login.clone();
        mismatched[4] = 0x10;
        assert!(lgt_local_biochronicle_response(&mismatched).is_none());

        let mut short = login.clone();
        short[3] = 0x14;
        assert!(lgt_local_biochronicle_response(&short).is_none());

        assert!(lgt_local_biochronicle_response(&login[..20]).is_none());
    }

    /// 데스티니아's certificate request is answered with the bytes its own
    /// reader keeps.
    #[test]
    fn the_certificate_that_title_waits_on_is_answered() {
        // The frame the title wrote, byte for byte.
        let mut request = vec![0x47, 0x00, 0x0a, 0x01];
        request.extend_from_slice(b"01046119269\0");
        request.extend_from_slice(&[0u8; 16]);
        request.extend_from_slice(b"Emulator");
        request.extend_from_slice(&[0u8; 8]);
        request.extend_from_slice(b"1.0.1");
        request.extend_from_slice(&[0u8; 5]);
        request.extend_from_slice(&[0x38, 0x50, 0x00, 0x00, 0x01, 0x00, 0x60, 0x00, 0x00, 0xff, 0xff, 0x1f, 0x00]);
        assert_eq!(request.len(), 0x47);

        let response = lgt_local_destinia_response(&request).unwrap();

        // Its own length, the kind the request's reader is registered at, the
        // signed byte it stops on when negative, forty bytes and one more.
        assert_eq!(response.len(), 4 + 1 + 40 + 1);
        assert_eq!(u16::from_le_bytes(response[0..2].try_into().unwrap()) as usize, response.len());
        assert_eq!(u16::from_le_bytes(response[2..4].try_into().unwrap()), 0x010b);
        assert!((response[4] as i8) >= 0);
        assert_eq!(response[5..45], [0; 40]);
    }

    /// The socket that title opens next asks the other kind its reader knows.
    #[test]
    fn the_confirmation_that_title_asks_next_is_answered() {
        // The frame the second socket carried, byte for byte.
        let mut request = vec![0x14, 0x00, 0x00, 0x02];
        request.extend_from_slice(b"01046119269\0");
        request.extend_from_slice(&[0x38, 0x50, 0x00, 0x00]);
        assert_eq!(request.len(), 0x14);

        let response = lgt_local_destinia_response(&request).unwrap();

        // `0x7b74` reads nothing past the byte every reply starts with.
        assert_eq!(response, vec![0x05, 0x00, 0x01, 0x02, 0x00]);
        assert_eq!(u16::from_le_bytes(response[0..2].try_into().unwrap()) as usize, response.len());
    }

    /// A frame that is not one of those requests is left alone.
    #[test]
    fn only_that_title_s_certificate_request_is_answered() {
        let mut request = vec![0x47, 0x00, 0x0a, 0x01];
        request.extend_from_slice(b"01046119269");
        request.extend_from_slice(&[0u8; 56]);
        assert_eq!(request.len(), 0x47);
        assert!(lgt_local_destinia_response(&request).is_some());

        // A kind this does not know the reader of.
        let mut other_kind = request.clone();
        other_kind[2] = 0x0c;
        assert!(lgt_local_destinia_response(&other_kind).is_none());

        // The right kind at the wrong size is not that request either - the
        // two this knows are both fixed frames.
        let mut confirm_sized = request.clone();
        confirm_sized[2] = 0x00;
        confirm_sized[3] = 0x02;
        assert!(lgt_local_destinia_response(&confirm_sized).is_none());

        // A length that is not the frame in hand.
        let mut mislaid = request.clone();
        mislaid[0] = 0x46;
        assert!(lgt_local_destinia_response(&mislaid).is_none());

        // Where the subscriber's number goes, something that is not one.
        let mut lettered = request.clone();
        lettered[4] = b'x';
        assert!(lgt_local_destinia_response(&lettered).is_none());

        // And a frame of another size under the same kind.
        assert!(lgt_local_destinia_response(&request[..70]).is_none());
    }

    /// 블레이드마스터3's login is answered with the three fields its own reader
    /// takes.
    #[test]
    fn the_login_blademaster3_waits_on_is_answered() {
        // The frame the title wrote, byte for byte.
        let mut request = vec![0x14, 0x00, 0x01, 0x00];
        request.extend_from_slice(b"01031768576\0");
        request.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x68, 0x00, 0x00, 0x00, 0xba, 0x02, 0xde, 0x24]);
        assert_eq!(request.len(), 28);

        let response = lgt_local_blademaster3_response(&request).unwrap();

        // The `4` both of the states that read a length this way insist on,
        // the kind the request came under, and zeroes.
        assert_eq!(response, vec![0x04, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    /// A frame that is not that login is left alone.
    #[test]
    fn only_blademaster3_s_login_is_answered() {
        let mut request = vec![0x14, 0x00, 0x01, 0x00];
        request.extend_from_slice(b"01031768576\0");
        request.extend_from_slice(&[0u8; 12]);
        assert!(lgt_local_blademaster3_response(&request).is_some());

        // A kind this does not know the reader of.
        let mut other_kind = request.clone();
        other_kind[2] = 3;
        assert!(lgt_local_blademaster3_response(&other_kind).is_none());

        // A length that is not the one this login declares.
        let mut mislaid = request.clone();
        mislaid[0] = 0x12;
        assert!(lgt_local_blademaster3_response(&mislaid).is_none());

        // Where the subscriber's number goes, something that is not one.
        let mut lettered = request.clone();
        lettered[4] = b'x';
        assert!(lgt_local_blademaster3_response(&lettered).is_none());

        // And a frame of another size.
        assert!(lgt_local_blademaster3_response(&request[..27]).is_none());
    }

    /// A message this does not know the shape of is left alone rather than
    /// answered with a frame the title would read as a step it never took.
    #[test]
    fn only_the_messages_of_that_menu_whose_readers_are_known_are_answered() {
        let request = elpis_session_opening();

        // An opcode whose reader this has not followed.
        let mut other_opcode = request.clone();
        other_opcode[3] = 0x14;
        assert!(lgt_local_opcode_header_response(&other_opcode).is_none());

        // A service this has not seen the body of.
        let mut other_service = request.clone();
        other_service[5..7].copy_from_slice(&2000u16.to_be_bytes());
        assert!(lgt_local_opcode_header_response(&other_service).is_none());

        // A length that is not the bytes in hand.
        let mut ragged = request.clone();
        ragged[1] = 0x66;
        assert!(lgt_local_opcode_header_response(&ragged).is_none());

        // A step of the walk whose body is not the size its sender writes.
        assert!(lgt_local_opcode_header_response(&[0x00, 0x07, 0x00, 0x43, 0x00, 0x00, 0x29]).is_none());
    }

    /// The 44-byte record 오셔너스 writes when a CASH purchase is confirmed,
    /// captured off its billing socket. The tail is the handset's own model
    /// buffer, which is why an emulated handset's name is sitting in it.
    fn oceanus_purchase_request() -> Vec<u8> {
        let mut request = Vec::from(*b"GLSN");
        request.extend_from_slice(&812u32.to_le_bytes());
        request.extend_from_slice(&812u32.to_le_bytes());
        request.extend_from_slice(&[0; 12]);
        request.extend_from_slice(&[0x70, 0x60, 0xb0, 0x40]);
        request.extend_from_slice(b"Emulator\0\0010853");
        request
    }

    #[test]
    fn a_cash_purchase_is_answered_with_the_word_that_grants_it() {
        let request = oceanus_purchase_request();
        assert_eq!(request.len(), 44);

        let reply = lgt_local_oceanus_response(&request).unwrap();

        // Two little-endian words, the first of them the zero `[ctx+0x10]`
        // reads as the purchase having gone through.
        assert_eq!(reply.len(), 8);
        assert_eq!(u32::from_le_bytes([reply[0], reply[1], reply[2], reply[3]]), 0);
        assert_eq!(response(&request), Some(reply));
    }

    #[test]
    fn a_record_that_is_not_the_cash_purchase_is_left_alone() {
        // Another title's frame of the same length.
        let mut foreign = oceanus_purchase_request();
        foreign[..4].copy_from_slice(b"LGT\0");
        assert!(lgt_local_oceanus_response(&foreign).is_none());

        // The right tag against a service the title does not bill.
        let mut other_service = oceanus_purchase_request();
        other_service[4..8].copy_from_slice(&813u32.to_le_bytes());
        assert!(lgt_local_oceanus_response(&other_service).is_none());

        // The service repeated as something else.
        let mut mismatched = oceanus_purchase_request();
        mismatched[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(lgt_local_oceanus_response(&mismatched).is_none());

        // A record that is not the length the title writes.
        let mut short = oceanus_purchase_request();
        short.truncate(43);
        assert!(lgt_local_oceanus_response(&short).is_none());
    }

    /// The 20-byte record 오셔너스 writes once its purchase is granted,
    /// captured off its billing socket.
    fn oceanus_settled_request() -> Vec<u8> {
        let mut request = Vec::from(*b"GLSN");
        request.extend_from_slice(&48u32.to_le_bytes());
        request.extend_from_slice(&[0; 4]);
        request.extend_from_slice(&4u32.to_le_bytes());
        request.extend_from_slice(&(-1154i32).to_le_bytes());
        request
    }

    #[test]
    fn the_record_that_settles_a_purchase_is_answered_with_the_whole_walk() {
        let request = oceanus_settled_request();
        assert_eq!(request.len(), 20);

        let reply = lgt_local_oceanus_settled_response(&request).unwrap();

        // The twelve state 4 reads: a discarded `u32`, the message being
        // answered, and two `u16`s whose last being zero is what sends the
        // walk to state 6 rather than state 5.
        assert_eq!(&reply[..4], &[0u8; 4]);
        assert_eq!(&reply[8..12], &[0u8; 4]);
        assert_eq!(u16::from_le_bytes([reply[10], reply[11]]), 0);

        // The length state 6 reads, and a body of exactly that many bytes for
        // state 7 to take. Positive, or state 6 arms a read of nothing.
        let body = u32::from_le_bytes([reply[12], reply[13], reply[14], reply[15]]);
        assert!(body > 0);
        assert_eq!(reply.len(), 16 + body as usize);

        // The step it runs is the message it answers plus one, and 49 is
        // `0x21a9c` - the purchase being applied. Anything else leaves the
        // title with the charge and no item.
        let step = u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]) + 1;
        assert_eq!(step, 0x31);

        // The `u16` `0x2108c` reads out of that body and `0x21a9c` rereads.
        // Zero is the grant; `0xffff` would close the socket short of it and
        // `0x3e9` is the carrier's error.
        let outcome = u16::from_le_bytes([reply[16], reply[17]]);
        assert_eq!(outcome, 0);
        assert_ne!(outcome, 0xffff);
        assert_ne!(outcome, 0x3e9);

        assert_eq!(response(&request), Some(reply));
    }

    #[test]
    fn the_two_oceanus_records_do_not_answer_for_one_another() {
        // The purchase is not the settlement, and neither is the reverse.
        assert!(lgt_local_oceanus_settled_response(&oceanus_purchase_request()).is_none());
        assert!(lgt_local_oceanus_response(&oceanus_settled_request()).is_none());

        // The right tag and length against a message the title does not send.
        let mut other_message = oceanus_settled_request();
        other_message[4..8].copy_from_slice(&49u32.to_le_bytes());
        assert!(lgt_local_oceanus_settled_response(&other_message).is_none());
    }

    /// The hundred and three bytes 엘피스's menu opens with.
    fn elpis_session_opening() -> Vec<u8> {
        let mut request = vec![0x00, 0x00, 0x00, 0x00, 0x00];
        // The service, then the rest of what `0x64d28` lays out.
        request.extend_from_slice(&1036u16.to_be_bytes());
        request.resize(5 + 98, 0);
        let whole = request.len() as u16;
        request[0..2].copy_from_slice(&whole.to_be_bytes());
        request
    }

    /// 창세기전3 에피소드2's frame, as the title writes it: the magic, the
    /// payload's length with the header not counted, then the message.
    fn genesis3_episode2_frame(message: u32, body: &[u8]) -> Vec<u8> {
        let mut frame = alloc::vec![0xfau8, 0xcb];
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&message.to_be_bytes());
        frame.extend_from_slice(body);

        frame
    }

    /// The login the title sends before its first screen: subscriber and
    /// handset model as length-prefixed strings, then 102 and a zero byte.
    fn genesis3_episode2_login() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&11u32.to_be_bytes());
        body.extend_from_slice(b"01046119269");
        body.extend_from_slice(&8u32.to_be_bytes());
        body.extend_from_slice(b"Emulator");
        body.extend_from_slice(&102u32.to_be_bytes());
        body.push(0);

        genesis3_episode2_frame(100, &body)
    }

    #[test]
    fn 에피소드2_is_told_its_login_was_good() {
        let reply = response(&genesis3_episode2_login()).unwrap();

        // The magic, a word of payload, and the message that answers a login.
        assert_eq!(&reply[..2], &[0xfa, 0xcb]);
        assert_eq!(u32::from_be_bytes([reply[2], reply[3], reply[4], reply[5]]), 4);
        assert_eq!(u32::from_be_bytes([reply[6], reply[7], reply[8], reply[9]]), 101);

        // And the word `0x1f6ac` reads out of it: zero is the verdict the title
        // carries on from, and anything else is an error it draws instead.
        assert_eq!(u32::from_be_bytes([reply[10], reply[11], reply[12], reply[13]]), 0);
        assert_eq!(reply.len(), 14);
    }

    #[test]
    fn 에피소드2_is_answered_the_same_way_on_the_step_after_the_login() {
        let reply = response(&genesis3_episode2_frame(700, &[0, 0, 0, 1])).unwrap();

        assert_eq!(u32::from_be_bytes([reply[6], reply[7], reply[8], reply[9]]), 701);
        assert_eq!(u32::from_be_bytes([reply[10], reply[11], reply[12], reply[13]]), 0);
        assert_eq!(reply.len(), 14);
    }

    /// The word is what keeps the stream lined up. The title takes an answer's
    /// declared length out of what it has and then reads a word off it, so an
    /// answer that declares less than it is read for leaves the count short and
    /// every frame after it starts mid-word - which is where a one byte answer
    /// left it, three under and stopped on "FOUND CORRUPT DATA!!!".
    #[test]
    fn 에피소드2s_answers_are_as_long_as_they_are_read_for() {
        for request in [genesis3_episode2_login(), genesis3_episode2_frame(700, &[0, 0, 0, 1])] {
            let reply = response(&request).unwrap();
            let declared = u32::from_be_bytes([reply[2], reply[3], reply[4], reply[5]]) as usize;

            assert_eq!(declared, size_of::<u32>());
            assert_eq!(reply.len(), 10 + declared);
        }
    }

    /// The session's own keep-alive, which the title answers with an empty one
    /// of its own at `0x1efe4`. Answered the same way, so the link does not go
    /// quiet between the steps of the walk.
    #[test]
    fn 에피소드2s_keep_alive_is_answered_with_a_keep_alive() {
        let reply = response(&genesis3_episode2_frame(1, &[])).unwrap();

        assert_eq!(reply, genesis3_episode2_frame(1, &[]));
        assert_eq!(reply.len(), 10);
    }

    /// A frame has to carry the magic, declare its own length, and be a message
    /// this walk is made of.
    #[test]
    fn 에피소드2_answers_only_its_own_walk() {
        // A message the walk does not contain.
        assert!(lgt_local_genesis3_episode2_response(&genesis3_episode2_frame(102, &[0])).is_none());

        // A length that is not the frame in hand.
        let mut short = genesis3_episode2_login();
        short.pop();
        assert!(lgt_local_genesis3_episode2_response(&short).is_none());

        // Another title's magic.
        let mut foreign = genesis3_episode2_frame(100, &[0]);
        foreign[0] = 0xfb;
        assert!(lgt_local_genesis3_episode2_response(&foreign).is_none());

        // And nothing to read at all.
        assert!(lgt_local_genesis3_episode2_response(&[0xfa, 0xcb]).is_none());
    }
}
