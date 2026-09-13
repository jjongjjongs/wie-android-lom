# LGT AOT 클래스: 메서드 테이블이 없는 클래스의 오버라이드

## 문제

LGT의 AOT 컴파일된 애플리케이션 클래스는 **이름으로 불릴 일이 있을 때만**
메서드 테이블(이름/디스크립터/엔트리)을 싣는다. 난독화된 내부 클래스는 그
테이블이 아예 비어 있고, 그러면 우리 쪽에서 브리지할 메서드가 하나도 없다.

놈3(00015E3D) 로그가 그 모습이다.

```
Bridging application class f extends java/io/InputStream with 0 callable methods
LGT class at 0x14014a8 dispatches through 0x4d865600, 8 of 19 slots its own
...
throwing java exception: java/lang/AbstractMethodError Abstract read()I method called
GameCanvas.loadMenu() : java.lang.AbstractMethodError: Abstract read()I method called
```

`f`는 `java.io.InputStream`을 상속하고 19개 디스패치 슬롯 중 8개가 자기
것인데, 메서드 테이블은 비어 있다. `DataInputStream`이 이 스트림을 감싸고
`read()`를 부르면 추상 `InputStream.read()`로 떨어진다.

`run()V` 하나에 대해서는 이미 같은 처방이 있었다 (`dispatch_run_entry`,
서든어택 포켓의 로더 스레드 `k`). 이번에 그것을 일반화했다.

## 읽는 법

네이티브 링킹은 클래스가 **선언한 슬롯만 채우고 상속하는 슬롯은 0으로 둔다**
(`init.rs`의 vtable 합성이 `entry != 0`을 "자기 것"으로 세는 근거가 이것이다).
그래서:

1. 상속 체인을 위로 걸어 각 조상이 슬롯에 붙인 이름을 모은다.
   애플리케이션 조상은 자기 메서드 테이블에서 (`AppMember::Method`의 `slot`),
   플랫폼 조상은 추출된 메타데이터(`platform_metadata.rs`)에서 가져온다.
   슬롯 번호는 둘을 관통하는 하나의 수열이므로 그대로 겹쳐 쓰면 된다.
   가까운 조상이 이긴다 — 오버라이드와 같은 규칙.
2. 클래스 자신의 디스패치 테이블에서 그 슬롯을 읽는다
   (`metadata + 0x0c`, 길이는 `metadata + 0x26`).
3. 0이 아니고, **애플리케이션 이미지 안을 가리키며**, 클래스의 메서드 테이블에
   이미 그 이름/디스크립터가 없으면 — 브리지할 오버라이드다.

이미지 밖을 가리키는 엔트리는 플랫폼 자신의 코드고, JVM이 이미 갖고 있다.
0인 슬롯은 상속이고, JVM도 같이 상속한다.

놈3의 `f`에서 나온 것:

```
Bridged f.read()I         from dispatch slot 10 @ 0x1365d
Bridged f.read([BII)I     from dispatch slot 12 @ 0x1383d
Bridged f.skip(J)J        from dispatch slot 13 @ 0x139a1
Bridged f.available()I    from dispatch slot 14 @ 0x13c5d
Bridged f.close()V        from dispatch slot 15 @ 0x13e79
Bridged f.mark(I)V        from dispatch slot 16 @ 0x13d0d
Bridged f.reset()V        from dispatch slot 17 @ 0x13d8d
Bridged f.markSupported()Z from dispatch slot 18 @ 0x13e65
```

## 기존 특례와의 순서

`as_proto`는 (1) 메서드 테이블, (2) `dispatch_run`, (3) Seed1 `p.run`,
(4) `Card::paint` 별칭을 먼저 세우고, **그 다음에** 이 오버라이드를 이름/
디스크립터가 겹치지 않을 때만 더한다. 앞의 넷이 내린 결정은 그대로 남으므로
기존 타이틀의 동작은 바뀌지 않는다 — 없던 메서드가 생길 뿐이다.

---

# 컴파일된 코드가 잡을 수 있어야 하는 예외

## 문제

놈3는 세이브 파일을 이렇게 읽는다.

```
0x32338(name):
    0x328f8(name)  ->  경로를 만들고 FileSystem.isFile 을 묻는다
    없으면 0x32780 ->  movs r0, #0     ; null 을 돌려준다
```

부르는 쪽(`0x1ca42`)은 그 null을 **그대로** `new ByteArrayInputStream(...)`에
넘긴다. 실기라면 `NullPointerException`이 나고, 게임이 그것을 잡는다 —
그게 "아직 세이브가 없다" 경로다. 로그의
`GameCanvas.Caller() : 0:3:0:java.lang.NullPointerException`이 그 catch다.

첫 실행에서 `/a`, `/start`, `/nom` 세 번 모두 이 길을 간다.

두 군데가 이걸 막고 있었다.

## (1) 런타임이 예외 대신 패닉했다

`java_runtime`의 `ByteArrayInputStream::<init>`은 곧장 배열 길이를 읽는다.
null 참조는 JVM 안에서 `Option::unwrap()`이 되어 **Rust 패닉** — 게임이
잡으려던 자리에서 프로세스가 죽었다.

`wie_jvm_support`가 `RT_RUSTJAR` 프로토를 넘겨받을 때 이 두 생성자를
null 검사가 앞에 붙은 같은 본문으로 갈아끼운다 (`refuse_a_null_array`).

## (2) 생성자가 던진 예외를 fatal 로 바꿨다

`method_bridge::invoke`의 일반 메서드 경로는 던져진 예외를
`WieError::JavaException`으로 돌려주고, 디스패처가 그것을 컴파일된
세이브포인트 체인(= 컴파일된 `try`/`catch`)으로 태운다. 그런데 **생성자
경로만** `JvmSupport::to_wie_err`로 스택 트레이스를 문자열화한
`FatalError`를 만들었고, fatal은 타이틀을 끝낸다.

```
net.wie.WieError: Compiled r.run failed: Fatal error:
java.lang.NullPointerException: buf is null
	at java/io/ByteArrayInputStream.<init>([B)V
	at r.run()V
```

`thrown_or_fatal`로 두 생성자 경로를 일반 메서드와 같은 길에 올렸다.
핸들을 만들 수 없는 예외만 fatal로 남는다 — 컴파일된 코드가 이름 붙일 수
없는 것은 잡을 수도 없기 때문이다.

## 결과

세 고침이 다 있어야 놈3가 뜬다. 하나씩 보면:

| 상태 | 결과 |
|---|---|
| 셋 다 없음 | `AbstractMethodError` → 배열 null → **패닉** (에뮬레이터 종료) |
| 디스패치 브리지만 | `read()`는 되지만 null 은 그대로 → **패닉** |
| + null 검사 | NPE 는 나지만 fatal 로 바뀜 → `r.run` 스레드 사망, 0 프레임 |
| + 예외 라우팅 | 게임이 NPE 를 잡고 진행 — 타이틀 화면 → 게임플레이 |

---

# 컴파일된 메서드에 배열을 넘길 때

## 문제

`f.read(byte[], int, int)`이 브리지되자마자 `IndexOutOfBoundsException`이
났다. 놈3의 `GameCanvas.loadMenu()`가 그것을 잡고, 메뉴 233줄이 전부 빈
문자열이 된다.

```
Calling compiled f.read([BII)I at 0x1383d
→ java.lang.IndexOutOfBoundsException
GameCanvas.loadMenu() : java.lang.IndexOutOfBoundsException
```

원인은 로그 바로 윗줄에 있었다.

```
[B declares no dispatch table; using the fallback
```

`JavaHandles::insert`가 붙여준 것은 **일반 인스턴스 핸들**이다. `+0x08`이
가리키는 것은 플랫폼 필드 한 줄에 한 워드짜리 필드 블록이지, 배열 블록이
아니다. 그런데 `allocate_array`의 주석이 말하듯 —

> The count is read straight off that block for every bounds check the
> compiled code makes — `ldr r3, [r0]; cmp index, r3`

— 컴파일된 코드는 배열 길이를 그 블록의 첫 워드에서 읽는다. 필드 블록의
첫 워드는 길이가 아니므로 모든 접근이 범위를 벗어난다.

리턴값에는 이미 처방이 있었다 (`materialize_primitive_array_result`).
**인자에는 없었다.** 이제 컴파일된 호출 직전에 원시 배열 인자를 게스트
블록으로 미러링한다.

## 그런데 되받아쓰면 안 된다

미러링만 하면 `IndexOutOfBounds`는 사라지지만 읽어온 바이트가 전부 0이 된다.
컴파일된 `read`가 버퍼를 채우는 방법이 그 이유다.

```
System.arraycopy(this.buf, pos, b, off, len)
```

`b`는 이미 JVM 배열로 등록되어 있으므로 (`handles.get`이 찾는다) arraycopy는
**JVM 배열 자체**에 쓴다. 게스트 블록은 건드리지 않는다. 호출이 끝나고
게스트 블록(그대로 0)을 JVM 배열에 되받아쓰면, 방금 arraycopy가 한 일을
정확히 지운다.

그래서 되받아쓰기는 **게스트가 실제로 쓴 경우에만** 한다. 미러링할 때 넣은
바이트를 그대로 들고 있다가 호출 후 블록과 비교해서, 같으면 JVM 배열이
가진 것을 그대로 둔다.

| | 컴파일 코드가 직접 씀 | arraycopy 로 채움 |
|---|---|---|
| 게스트 블록 | 새 데이터 | 미러링한 그대로 |
| JVM 배열 | 낡음 | 새 데이터 |
| 되받아쓰기 | 한다 | **안 한다** |

## 놈3에서 달라진 것

`m.stxt`(233줄, 6854바이트)가 제대로 읽힌다. 길이는 원래도 맞았다 —
`readByte()`로 읽는 u16 길이 접두사는 한 바이트씩 오는 경로라 무사했고,
본문만 `read(byte[],off,len)`으로 와서 0이 되었다. 그래서 게임은 길이만
맞는 NUL 문자열 233개를 들고 있었다.

```
String.trim this="\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"   ← 고치기 전
String.trim this="[컬러스킨 구입]"                    ← 고친 뒤
```

화면으로는 타이틀의 GAMEVIL 로고, 메뉴 항목, `[조작방법]` 본문이 돌아왔다.

## 아직 남은 것

`[조작방법]` 화면에서 어떤 키를 눌러도 넘어가지 않는다. 키는 도착한다 —
`r.keyNotify(1, -5)`가 불리고 `true`를 돌려준다 — 이전 화면들(이용안내 →
타이틀 → 메뉴 → 조작방법)은 전부 같은 키로 넘어갔다. 키패드의 모든 키를
차례로, 한 번에 300틱씩 눌러도 화면이 한 픽셀도 바뀌지 않는다. 게임 스레드는
`Thread.sleep(100)` 루프를 돌며 다시 그리기만 한다.

## 화면 아래 24행

게임이 스스로 비워 둔 자리다. 시작할 때 이렇게 묻는다.

```
AnnunciatorComponent.getHeight() -> 24
Card.getHeight()
AnnunciatorComponent.getHeight() -> 24
```

그리고 320 − 24 = 296 을 자기 화면 높이로 삼아 그 안에 배치한다 —
`[조작방법]` 상자는 y 28..268, 296 안에서 위아래 28씩. 240x320 프레임버퍼의
남는 24행은 실기라면 핸드셋의 상태 표시줄이 차지하는 자리이고, 우리는 그것을
그리지 않으니 검게 남는다.
